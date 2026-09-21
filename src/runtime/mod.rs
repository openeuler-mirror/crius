/*
Copyright 2026 KylinSoft  Co., Ltd.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/


pub mod backend;
pub mod shim_manager;
pub mod runc_backend;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::process::{Output, Command,};
use std::os::unix::process::CommandExt;
use std::unimplemented;

use thiserror::Error;
use log::debug;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::proto::runtime::v1::{
    LinuxContainerResources,
    Capability, NamespaceOption, 
};
use crate::image::snapshotter::RootfsHandle;
use crate::runtime::shim_manager::ShimManager;
use crate::oci::spec::{Rlimit, Spec};
use crate::security::devices::DeviceMapping;
use crate::config::CgroupDriverConfig;
use crate::runtime::shim_manager::ShimConfig;
use crate::cgroup::{ResourceLimits, MemoryLimit, CpuLimit};
use crate::defaults::DEFAULT_CONTAINER_CREATE_TIMEOUT_SECS;

pub trait ContainerRuntime {
    /// 创建容器
    fn create_container(&self, container_id: &str, config: &ContainerConfig) -> Result<String>;

    /// 启动容器
    fn start_container(&self, container_id: &str) -> Result<()>;

    /// 停止容器
    fn stop_container(&self, container_id: &str, timeout: Option<u32>) -> Result<()>;

    /// 删除容器
    fn remove_container(&self, container_id: &str) -> Result<()>;

    /// 获取容器状态
    fn container_status(&self, container_id: &str) -> Result<ContainerStatus>;

    /// 重新打开容器日志
    fn reopen_container_log(&self, container_id: &str) -> Result<()>;

    /// 在容器中执行命令
    fn exec_in_container(&self, container_id: &str, command: &[String], tty: bool) -> Result<i32>;

    /// 更新容器资源限制
    fn update_container_resources(
        &self,
        container_id: &str,
        resources: &LinuxContainerResources,
    ) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFeatureProbe {
    pub available: bool,
    pub idmap_mounts: bool,
    pub recursive_read_only_mounts: bool,
    pub checkpoint_restore: bool,
    pub reopen_log: bool,
    pub exec_tty: bool,
    pub cgroup: bool,
    pub rootless: bool,
    pub shim_rpc: bool,
    pub mount_options: Vec<String>,
    pub oci_version_min: Option<String>,
    pub oci_version_max: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreparedRootfsMount {
    pub key: String,
    pub mountpoint: PathBuf,
    pub readonly: bool,
    pub handle: RootfsHandle,
}

#[derive(Debug, Error)]
pub enum MountSemanticsError {
    #[error("mount {destination} source {source_path} does not exist")]
    MissingSource {
        source_path: PathBuf,
        destination: PathBuf,
    },
    #[error(
        "mount {destination} requests SELinux relabel but the container has no SELinux mount label"
    )]
    SelinuxRelabelRequiresMountLabel { destination: PathBuf },
    #[error(
        "mount {destination} requests recursive read-only but runtime does not advertise rro mount option support"
    )]
    RecursiveReadOnlyUnsupported { destination: PathBuf },
    #[error(
        "mount {destination} requests idmapped mount but runtime does not advertise idmap mount support"
    )]
    IdmapMountUnsupported { destination: PathBuf },
    #[error(
        "mount {destination} requests recursive read-only for non-directory source {source_path}"
    )]
    RecursiveReadOnlyRequiresDirectory {
        source_path: PathBuf,
        destination: PathBuf,
    },
    #[error(
        "mount {destination} requests bidirectional propagation but source {source_path} is not a shared mount"
    )]
    BidirectionalPropagationRequiresShared {
        source_path: PathBuf,
        destination: PathBuf,
    },
    #[error(
        "mount {destination} requests host-to-container propagation but source {source_path} is neither a shared nor slave mount"
    )]
    HostToContainerPropagationRequiresSharedOrSlave {
        source_path: PathBuf,
        destination: PathBuf,
    },
    #[error("failed to inspect mount propagation for {source_path}: {message}")]
    MountPropagationInspectionFailed {
        source_path: PathBuf,
        message: String,
    },
}

impl MountSemanticsError {
    pub(crate) fn to_status(&self) -> tonic::Status {
        match self {
            Self::MissingSource {
                source_path,
                destination,
            } => tonic::Status::failed_precondition(
                format!(
                    "mount {} source {} does not exist",
                    destination.display(),
                    source_path.display()
                ),
            ),
            Self::SelinuxRelabelRequiresMountLabel { destination } => {
                tonic::Status::failed_precondition(format!(
                    "mount {} requests SELinux relabel but SELinux mount labeling is unavailable",
                    destination.display()
                ))
            }
            Self::RecursiveReadOnlyUnsupported { destination } => {
                tonic::Status::failed_precondition(format!(
                    "mount {} requests recursive_read_only but the selected runtime does not support it",
                    destination.display()
                ))
            }
            Self::IdmapMountUnsupported { destination } => tonic::Status::failed_precondition(
                format!(
                    "mount {} requests uidMappings/gidMappings but the selected runtime does not support idmapped mounts",
                    destination.display()
                ),
            ),
            Self::RecursiveReadOnlyRequiresDirectory {
                source_path,
                destination,
            } => {
                tonic::Status::invalid_argument(format!(
                    "mount {} source {} must be a directory when recursive_read_only=true",
                    destination.display(),
                    source_path.display()
                ))
            }
            Self::BidirectionalPropagationRequiresShared {
                source_path,
                destination,
            } => {
                tonic::Status::failed_precondition(format!(
                    "mount {} source {} must be a shared mount for bidirectional propagation",
                    destination.display(),
                    source_path.display()
                ))
            }
            Self::HostToContainerPropagationRequiresSharedOrSlave {
                source_path,
                destination,
            } => {
                tonic::Status::failed_precondition(format!(
                    "mount {} source {} must be a shared or slave mount for host-to-container propagation",
                    destination.display(),
                    source_path.display()
                ))
            }
            Self::MountPropagationInspectionFailed {
                source_path,
                message,
            } => {
                tonic::Status::failed_precondition(format!(
                    "failed to inspect mount propagation for {}: {}",
                    source_path.display(),
                    message
                ))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ImageVolumesMode {
    Mkdir,
    Bind,
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootfsSnapshotter {
    InternalOverlayUntar,
    InternalCachedRootfs,
    External(String),
}


/// 使用 runc 作为容器运行时
#[derive(Debug, Clone)]
pub struct RuncRuntime {
    runtime_path: PathBuf,
    runtime_config_path: PathBuf,
    root: PathBuf,
    image_storage_root: PathBuf,
    state_db_path: Option<PathBuf>,
    shim_manager: Option<Arc<ShimManager>>,
    default_env: Vec<(String, String)>,
    default_capabilities: Vec<String>,
    default_sysctls: HashMap<String, String>,
    default_ulimits: Vec<Rlimit>,
    allowed_devices: HashSet<PathBuf>,
    additional_devices: Vec<DeviceMapping>,
    device_ownership_from_security_context: bool,
    privileged_without_host_devices: bool,
    privileged_without_host_devices_all_devices_allowed: bool,
    add_inheritable_capabilities: bool,
    base_runtime_spec: Option<Spec>,
    default_mounts_file: Option<PathBuf>,
    hooks_dirs: Vec<PathBuf>,
    absent_mount_sources_to_reject: Vec<PathBuf>,
    image_volumes: ImageVolumesMode,
    rootfs_snapshotter: RootfsSnapshotter,
    container_create_timeout_secs: u32,
    container_stop_timeout_secs: u32,
    criu_path: PathBuf,
    restrict_oom_score_adj: bool,
    bind_mount_prefix: PathBuf,
    disable_cgroup: bool,
    // rootless: crate::rootless::EffectiveRootlessConfig,
    default_seccomp_profile_path: Option<PathBuf>,
    exec_cpu_affinity: String,
    no_pivot: bool,
    no_new_keyring: bool,
    disable_proc_mount: bool,
    timezone: String,
    cgroup_driver: CgroupDriverConfig,
}

impl RuncRuntime {
    fn default_capabilities() -> Vec<String> {
        unimplemented!()
    }

    pub fn with_shim_and_image_storage(
        runtime_path: PathBuf,
        root: PathBuf,
        image_storage_root: PathBuf,
        shim_config: ShimConfig,
    ) -> Self {
        let no_pivot = shim_config.no_pivot;
        let runtime_config_path = shim_config.runtime_config_path.clone();
        let state_db_path = (!shim_config.state_db_path.as_os_str().is_empty())
            .then(|| shim_config.state_db_path.clone());
        let shim_manager = Arc::new(ShimManager::new(shim_config));
        Self {
            runtime_path,
            runtime_config_path,
            root,
            image_storage_root,
            state_db_path,
            shim_manager: Some(shim_manager),
            default_env: Vec::new(),
            default_capabilities: Self::default_capabilities(),
            default_sysctls: HashMap::new(),
            default_ulimits: Vec::new(),
            allowed_devices: HashSet::new(),
            additional_devices: Vec::new(),
            device_ownership_from_security_context: false,
            privileged_without_host_devices: false,
            privileged_without_host_devices_all_devices_allowed: false,
            add_inheritable_capabilities: false,
            base_runtime_spec: None,
            default_mounts_file: None,
            hooks_dirs: Vec::new(),
            absent_mount_sources_to_reject: Vec::new(),
            image_volumes: ImageVolumesMode::Mkdir,
            rootfs_snapshotter: RootfsSnapshotter::InternalOverlayUntar,
            container_create_timeout_secs: DEFAULT_CONTAINER_CREATE_TIMEOUT_SECS,
            container_stop_timeout_secs: 30,
            criu_path: PathBuf::new(),
            restrict_oom_score_adj: false,
            bind_mount_prefix: PathBuf::new(),
            disable_cgroup: false,
            // rootless: crate::rootless::EffectiveRootlessConfig::disabled(),
            default_seccomp_profile_path: None,
            exec_cpu_affinity: String::new(),
            no_pivot,
            no_new_keyring: false,
            disable_proc_mount: false,
            timezone: String::new(),
            cgroup_driver: CgroupDriverConfig::Cgroupfs,
        }
    }

    pub fn runtime_root(&self) -> &Path {
        &self.root
    }

    pub fn runtime_path(&self) -> &Path {
        &self.runtime_path
    }

    pub fn runtime_config_path(&self) -> &Path {
        &self.runtime_config_path
    }

    pub fn probe_runtime_features(&self) -> RuntimeFeatureProbe {
        let output = match self.run_command_output(&["features"]) {
            Ok(output) => output,
            Err(err) => {
                return RuntimeFeatureProbe {
                    error: Some(format!("failed to execute runtime features command: {err}")),
                    ..Default::default()
                };
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("status={}", output.status)
            };
            return RuntimeFeatureProbe {
                error: Some(format!("runtime features command failed: {detail}")),
                ..Default::default()
            };
        }

        let parsed = match serde_json::from_slice::<OciRuntimeFeaturesDocument>(&output.stdout) {
            Ok(parsed) => parsed,
            Err(err) => {
                return RuntimeFeatureProbe {
                    error: Some(format!("failed to parse runtime features output: {err}")),
                    ..Default::default()
                };
            }
        };

        if parsed.oci_version_min.trim().is_empty() || parsed.oci_version_max.trim().is_empty() {
            return RuntimeFeatureProbe {
                error: Some("runtime features structure is not valid".to_string()),
                ..Default::default()
            };
        }

        let idmap_mounts = parsed
            .linux
            .as_ref()
            .and_then(|linux| linux.mount_extensions.as_ref())
            .and_then(|extensions| extensions.idmap.as_ref())
            .and_then(|feature| feature.enabled)
            .unwrap_or(false);
        let recursive_read_only_mounts = parsed.mount_options.iter().any(|option| option == "rro");

        RuntimeFeatureProbe {
            available: true,
            idmap_mounts,
            recursive_read_only_mounts,
            checkpoint_restore: true,
            reopen_log: self.shim_manager.is_some(),
            exec_tty: true,
            cgroup: !self.disable_cgroup,
            rootless: false,
            shim_rpc: self.shim_manager.is_some(),
            mount_options: parsed.mount_options,
            oci_version_min: Some(parsed.oci_version_min),
            oci_version_max: Some(parsed.oci_version_max),
            error: None,
        }
    }

     /// 执行runc命令并返回输出（仅用于需要解析stdout的查询类命令）
    fn run_command_output(&self, args: &[&str]) -> Result<Output> {
        debug!(
            "Executing: {} {}",
            self.runtime_path.display(),
            args.join(" ")
        );

        let output = self
            .runtime_command()
            .args(args)
            .output()
            .context("Failed to execute runc command")?;

        Ok(output)
    }
    
    fn runtime_command(&self) -> Command {
        let mut cmd = Command::new(&self.runtime_path);
        if self.cgroup_driver == CgroupDriverConfig::Systemd {
            cmd.arg("--systemd-cgroup");
        }
        if !self.runtime_config_path.as_os_str().is_empty() {
            cmd.arg("--config").arg(&self.runtime_config_path);
        }
        cmd
    }

    pub fn cgroup_driver(&self) -> CgroupDriverConfig {
        self.cgroup_driver
    }

    pub(crate) fn apply_exec_cpu_affinity_to_std_command(
        command: &mut Command,
        cpu: Option<usize>,
    ) {
        let Some(cpu) = cpu else {
            return;
        };
        unsafe {
            command.pre_exec(move || {
                let mut set = nix::sched::CpuSet::new();
                set.set(cpu)
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
                nix::sched::sched_setaffinity(nix::unistd::Pid::from_raw(0), &set)
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
                Ok(())
            });
        }
    }

    /// 将 CRI LinuxContainerResources 转换为 ResourceLimits
    pub(crate) fn cri_to_limits(resources: &LinuxContainerResources) -> ResourceLimits {
        ResourceLimits {
            cpu: Some(CpuLimit {
                shares: (resources.cpu_shares > 0).then_some(resources.cpu_shares as u64),
                quota: (resources.cpu_quota > 0).then_some(resources.cpu_quota),
                period: (resources.cpu_period > 0).then_some(resources.cpu_period as u64),
                realtime_runtime: None,
                realtime_period: None,
                cpus: (!resources.cpuset_cpus.is_empty()).then(|| resources.cpuset_cpus.clone()),
                mems: (!resources.cpuset_mems.is_empty()).then(|| resources.cpuset_mems.clone()),
            }),
            memory: Some(MemoryLimit {
                limit: (resources.memory_limit_in_bytes > 0)
                    .then_some(resources.memory_limit_in_bytes),
                reservation: None,
                swap: (resources.memory_swap_limit_in_bytes > 0)
                    .then_some(resources.memory_swap_limit_in_bytes),
                kernel: None,
                kernel_tcp: None,
                swappiness: None,
                disable_oom_killer: None,
                use_hierarchy: None,
            }),
            blkio: None,
            network: None,
            pids: None,
        }
    }

    pub fn is_container_paused(&self, container_id: &str) -> Result<bool> {
        unimplemented!()
    }

    pub fn restore_attach_shim(&self, container_id: &str) -> Result<()> {
        unimplemented!()
    }

    pub fn open_attach_stream(
        &self,
        container_id: &str,
        stdin: bool,
        stdout: bool,
        stderr: bool,
        tty: bool,
    ) -> Result<crate::shim_rpc::OpenAttachStreamResponse> {
        unimplemented!()
    }

    pub fn close_attach_stream(&self, container_id: &str, stream_id: &str) -> Result<()> {
        unimplemented!()
    }

    pub fn resize_attach_pty(
        &self,
        container_id: &str,
        stream_id: Option<&str>,
        width: u16,
        height: u16,
    ) -> Result<()> {
        unimplemented!()
    }

    pub fn shim_status(&self, container_id: &str,) -> Result<Option<crate::shim_rpc::StatusResponse>> {
        unimplemented!()
    }

    pub fn restore_container_from_checkpoint(
        &self,
        container_id: &str,
        image_path: &Path,
        work_path: &Path,
    ) -> Result<()> {
        unimplemented!()
    }

    pub fn pause_container(&self, container_id: &str) -> Result<()> {
        unimplemented!()
    }

    pub fn checkpoint_container(
        &self,
        container_id: &str,
        image_path: &Path,
        work_path: &Path,
    ) -> Result<()> {
        unimplemented!()
    }

    pub fn resume_container(&self, container_id: &str) -> Result<()> {
        unimplemented!()
    }

    /// 获取容器 init 进程 PID
    pub fn container_pid(&self, container_id: &str) -> Result<Option<i32>> {
        unimplemented!()
    }

    pub fn bundle_path_for(&self, container_id: &str) -> PathBuf {
        unimplemented!()
    }

    pub fn enforce_oom_score_adj_policy(&self, spec: &mut Spec) -> Result<()> {
        unimplemented!()
    }

    /// 分步创建：准备 rootfs（NRI 可在后续步骤介入 spec）。
    pub fn prepare_rootfs(
        &self,
        container_id: &str,
        config: &ContainerConfig,
    ) -> Result<PreparedRootfsMount> {
        unimplemented!()
    }

    /// 分步创建：构建 pristine OCI spec。
    pub fn build_spec(&self, container_id: &str, config: &ContainerConfig) -> Result<Spec> {
        unimplemented!()
    }

   
    pub fn write_bundle(&self, container_id: &str, rootfs: &Path, spec: &Spec) -> Result<()>{
        unimplemented!()
    }

    pub fn create_task_from_prepared_bundle(&self, container_id: &str,  rootfs: PreparedRootfsMount,) -> Result<()> {
        unimplemented!()
    }

    /// 分步创建：从 bundle 读取 OCI spec。
    pub fn load_spec(&self, container_id: &str) -> Result<Spec> {
        unimplemented!()
    }

    pub fn validate_mount_requests(&self, config: &ContainerConfig,) -> std::result::Result<(), MountSemanticsError> {
        unimplemented!()
    }

    pub fn restrict_oom_score_adj_floor(oom_score_adj: i64) -> Result<i64> {
        unimplemented!()
    }
}

impl ContainerRuntime for RuncRuntime {
    fn create_container(&self, container_id: &str, config: &ContainerConfig) -> Result<String> {
        unimplemented!()
    }

    fn start_container(&self, container_id: &str) -> Result<()> {
        unimplemented!()
    }

    fn stop_container(&self, container_id: &str, timeout: Option<u32>) -> Result<()> {
        unimplemented!()
    }

    fn remove_container(&self, container_id: &str) -> Result<()> {
        unimplemented!()
    }

    fn container_status(&self, container_id: &str) -> Result<ContainerStatus> {
        unimplemented!()
    }

    fn reopen_container_log(&self, container_id: &str) -> Result<()> {
        unimplemented!()
    }

    fn exec_in_container(&self, container_id: &str, command: &[String], tty: bool) -> Result<i32> {
        unimplemented!()
    }

    fn update_container_resources(
        &self,
        container_id: &str,
        resources: &LinuxContainerResources,
    ) -> Result<()> {
        unimplemented!()
    }
}

#[derive(Debug, Deserialize)]
struct OciRuntimeFeaturesDocument {
    #[serde(rename = "ociVersionMin", default)]
    oci_version_min: String,
    #[serde(rename = "ociVersionMax", default)]
    oci_version_max: String,
    #[serde(rename = "mountOptions", default)]
    mount_options: Vec<String>,
    linux: Option<OciRuntimeFeaturesLinux>,
}

#[derive(Debug, Deserialize)]
struct OciRuntimeFeaturesLinux {
    #[serde(rename = "mountExtensions")]
    mount_extensions: Option<OciRuntimeMountExtensions>,
}

#[derive(Debug, Deserialize)]
struct OciRuntimeMountExtensions {
    idmap: Option<OciRuntimeFeatureToggle>,
}

#[derive(Debug, Deserialize)]
struct OciRuntimeFeatureToggle {
    enabled: Option<bool>,
}

/// Seccomp 配置来源
#[derive(Debug, Clone)]
pub enum SeccompProfile {
    RuntimeDefault,
    Unconfined,
    Localhost(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingMountSourcePolicy {
    Ignore,
    Reject,
    CreateDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountPropagationMode {
    Private,
    HostToContainer,
    Bidirectional,
}

/// 挂载点配置
#[derive(Debug, Clone)]
pub struct MountConfig {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub read_only: bool,
    pub missing_source_policy: MissingMountSourcePolicy,
    pub selinux_relabel: bool,
    pub propagation: MountPropagationMode,
    pub recursive_read_only: bool,
    pub uid_mappings: Vec<crate::oci::spec::IdMapping>,
    pub gid_mappings: Vec<crate::oci::spec::IdMapping>,
    pub requested_image: Option<String>,
    pub image_sub_path: Option<String>,
}

/// 容器配置
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub name: String,
    pub image: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub working_dir: Option<PathBuf>,
    pub mounts: Vec<MountConfig>,
    pub labels: Vec<(String, String)>,
    pub annotations: Vec<(String, String)>,
    pub cdi_devices: Vec<String>,
    pub privileged: bool,
    pub user: Option<String>,
    pub run_as_group: Option<u32>,
    pub supplemental_groups: Vec<u32>,
    pub hostname: Option<String>,
    pub tty: bool,
    pub stdin: bool,
    pub stdin_once: bool,
    pub log_path: Option<PathBuf>,
    pub readonly_rootfs: bool,
    // pub seccomp_notifier: Option<SeccompNotifierConfig>,
    pub pids_limit: Option<i64>,
    pub no_new_privileges: Option<bool>,
    pub apparmor_profile: Option<String>,
    pub selinux_label: Option<String>,
    pub seccomp_profile: Option<SeccompProfile>,
    pub capabilities: Option<Capability>,
    pub cgroup_parent: Option<String>,
    pub sysctls: HashMap<String, String>,
    pub namespace_options: Option<NamespaceOption>,
    pub namespace_paths: NamespacePaths,
    pub linux_resources: Option<LinuxContainerResources>,
    pub devices: Vec<DeviceMapping>,
    pub masked_paths: Vec<String>,
    pub readonly_paths: Vec<String>,
    pub rootfs: PathBuf,
}

/// 命名空间路径覆盖
#[derive(Debug, Clone, Default)]
pub struct NamespacePaths {
    pub network: Option<PathBuf>,
    pub pid: Option<PathBuf>,
    pub ipc: Option<PathBuf>,
    pub uts: Option<PathBuf>,
}

/// 容器状态
#[derive(Debug, Clone, PartialEq)]
pub enum ContainerStatus {
    Created,
    Running,
    Stopped(i32), // 退出码
    Unknown,
}