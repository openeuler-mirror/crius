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
use std::process::{Output, Command, Stdio,};
use std::os::unix::process::CommandExt;
use std::unimplemented;

use thiserror::Error;
use log::{debug, error, info};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use nix::sys::stat::{
    mknod, makedev,
    SFlag, Mode,
};

use crate::proto::runtime::v1::{
    LinuxContainerResources,
    Capability, NamespaceOption, 
    NamespaceMode,
};
use crate::image::snapshotter::RootfsHandle;
use crate::runtime::shim_manager::ShimManager;
use crate::oci::spec::{
    Rlimit, Spec, Root, Process,
    LinuxResources, Linux, LinuxPids,
    Mount, Namespace as OciNamespace,
    User,
};
use crate::security::devices::DeviceMapping;
use crate::config::CgroupDriverConfig;
use crate::runtime::shim_manager::ShimConfig;
use crate::cgroup::{
    ResourceLimits, MemoryLimit, CpuLimit,
    to_oci_resources,
};
use crate::image::snapshotter::RootfsHandleKind;
use crate::storage::{
    StorageManager, RuntimeArtifactRecord
};
use crate::defaults::{
    DEFAULT_CONTAINER_CREATE_TIMEOUT_SECS,
    INTERNAL_CONTAINER_STATE_KEY,
    INTERNAL_CHECKPOINT_RESTORE_KEY,
    CRIO_LABELS_ANNOTATION,
    INTERNAL_UID_MAPPINGS_MOUNT_OPTION_PREFIX,
    INTERNAL_GID_MAPPINGS_MOUNT_OPTION_PREFIX,
};

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

impl From<crate::image::snapshotter::MountView> for PreparedRootfsMount {
    fn from(value: crate::image::snapshotter::MountView) -> Self {
        Self {
            key: value.key,
            mountpoint: value.mountpoint,
            readonly: value.readonly,
            handle: value.rootfs,
        }
    }
}

impl PreparedRootfsMount {
    fn internal_path(snapshot_key: String, mountpoint: PathBuf, readonly: bool) -> Self {
        Self {
            key: snapshot_key.clone(),
            mountpoint: mountpoint.clone(),
            readonly,
            handle: RootfsHandle::internal_path(
                snapshot_key.clone(),
                "container",
                snapshot_key,
                mountpoint,
                readonly,
            ),
        }
    }

    fn rootfs_path(&self) -> Result<&Path> {
        self.handle.rootfs_path().ok_or_else(|| {
            anyhow::anyhow!(
                "rootfs handle {:?} for snapshot {} has no rootfs path",
                self.handle.kind,
                self.key
            )
        })
    }

    fn mount_options(&self) -> Vec<String> {
        match self.handle.kind {
            RootfsHandleKind::InternalPath => {
                if self.readonly {
                    vec!["ro".to_string()]
                } else {
                    Vec::new()
                }
            }
            RootfsHandleKind::ExternalMountSpec => self
                .handle
                .mounts
                .iter()
                .flat_map(|mount| mount.options.clone())
                .collect(),
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
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

/// runc容器状态
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuncState {
    #[serde(rename = "ociVersion")]
    oci_version: String,
    id: String,
    status: String,
    pid: i32,
    bundle: String,
    rootfs: String,
    created: String,
    owner: String,
}

#[derive(Debug, Deserialize, Default)]
struct RuntimeStoredContainerState {
    tty: bool,
}

type UserNamespaceMappings = (
    Option<Vec<crate::oci::spec::IdMapping>>,
    Option<Vec<crate::oci::spec::IdMapping>>,
);

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
        crate::security::spec_patch::default_capabilities()
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

    /// 获取runc容器状态
    fn get_runc_state(&self, container_id: &str) -> Result<Option<RuncState>> {
        let output = self.run_command_output(&["state", container_id])?;

        if !output.status.success() {
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let state: RuncState =
            serde_json::from_str(&stdout).context("Failed to parse runc state")?;

        Ok(Some(state))
    }

    pub fn is_container_paused(&self, container_id: &str) -> Result<bool> {
        if let Some(ref shim_manager) = self.shim_manager {
            return Ok(matches!(
                shim_manager.status(container_id)?.state,
                crate::shim_rpc::TaskState::Paused
            ));
        }
        Ok(matches!(
            self.get_runc_state(container_id)?,
            Some(state) if state.status == "paused"
        ))
    }

    /// 获取容器的bundle目录
    fn bundle_path(&self, container_id: &str) -> PathBuf {
        self.root.join(container_id)
    }

    /// 获取容器的config.json路径
    fn config_path(&self, container_id: &str) -> PathBuf {
        self.bundle_path(container_id).join("config.json")
    }

    fn load_bundle_config_value(&self, container_id: &str) -> Result<Value> {
        let config_path = self.config_path(container_id);
        let raw = std::fs::read(&config_path)
            .with_context(|| format!("Failed to read OCI config {}", config_path.display()))?;
        serde_json::from_slice(&raw)
            .with_context(|| format!("Failed to parse OCI config {}", config_path.display()))
    }

    fn container_uses_terminal(&self, container_id: &str) -> Result<bool> {
        let config = self.load_bundle_config_value(container_id)?;
        let from_process = config
            .get("process")
            .and_then(|process| process.get("terminal"))
            .and_then(|value| value.as_bool());
        if let Some(tty) = from_process {
            return Ok(tty);
        }

        let from_internal_state = config
            .get("annotations")
            .and_then(|annotations| annotations.get(INTERNAL_CONTAINER_STATE_KEY))
            .and_then(|value| value.as_str())
            .and_then(|raw| serde_json::from_str::<RuntimeStoredContainerState>(raw).ok())
            .map(|state| state.tty)
            .unwrap_or(false);
        Ok(from_internal_state)
    }

    fn rootfs_cleanup_target_from_bundle(&self, container_id: &str) -> Option<PathBuf> {
        let spec = Spec::load(self.config_path(container_id)).ok()?;
        let root = spec.root?;
        self.rootfs_cleanup_target_from_path(container_id, Path::new(&root.path))
    }

    fn rootfs_cleanup_target_from_path(
        &self,
        container_id: &str,
        rootfs_path: &Path,
    ) -> Option<PathBuf> {
        let resolved_rootfs = if rootfs_path.is_absolute() {
            rootfs_path.to_path_buf()
        } else {
            self.bundle_path(container_id).join(rootfs_path)
        };
        let parent = resolved_rootfs.parent()?;
        if resolved_rootfs.file_name() == Some(std::ffi::OsStr::new("rootfs"))
            && parent.file_name() == Some(std::ffi::OsStr::new(container_id))
        {
            Some(parent.to_path_buf())
        } else {
            Some(resolved_rootfs)
        }
    }

    pub fn restore_attach_shim(&self, container_id: &str) -> Result<()> {
        let shim_manager = self
            .shim_manager
            .as_ref()
            .context("attach recovery requires shim-enabled runtime")?;
        if !self.container_uses_terminal(container_id)? {
            return Err(anyhow::anyhow!(
                "attach recovery is only supported for tty containers"
            ));
        }

        let bundle_path = self.bundle_path(container_id);
        if !bundle_path.exists() {
            return Err(anyhow::anyhow!(
                "bundle path is missing for attach recovery: {}",
                bundle_path.display()
            ));
        }
        let rootfs_path = self
            .rootfs_cleanup_target_from_bundle(container_id)
            .unwrap_or_else(|| bundle_path.join("rootfs"));

        shim_manager
            .create_task(
                container_id,
                &bundle_path,
                &rootfs_path,
                Some(container_id),
                Vec::new(),
                RootfsHandle::internal_path(
                    container_id.to_string(),
                    "container",
                    container_id.to_string(),
                    rootfs_path.clone(),
                    false,
                ),
            )
            .context("failed to restore shim for attach recovery")?;
        Ok(())
    }

    pub fn open_attach_stream(
        &self,
        container_id: &str,
        stdin: bool,
        stdout: bool,
        stderr: bool,
        tty: bool,
    ) -> Result<crate::shim_rpc::OpenAttachStreamResponse> {
        self.shim_manager
            .as_ref()
            .context("attach RPC stream requires shim-enabled runtime")?
            .open_attach_stream(container_id, stdin, stdout, stderr, tty)
    }

    pub fn close_attach_stream(&self, container_id: &str, stream_id: &str) -> Result<()> {
        self.shim_manager
            .as_ref()
            .context("attach RPC stream requires shim-enabled runtime")?
            .close_attach_stream(container_id, stream_id)
    }

    pub fn resize_attach_pty(
        &self,
        container_id: &str,
        stream_id: Option<&str>,
        width: u16,
        height: u16,
    ) -> Result<()> {
        self.shim_manager
            .as_ref()
            .context("attach RPC stream requires shim-enabled runtime")?
            .resize_attach_pty(container_id, stream_id, width, height)
    }

    pub fn shim_status(
        &self,
        container_id: &str,
    ) -> Result<Option<crate::shim_rpc::StatusResponse>> {
        let Some(shim_manager) = self.shim_manager.as_ref() else {
            return Ok(None);
        };
        if !shim_manager.task_socket_path(container_id).exists()
            && !shim_manager.is_shim_running(container_id)
        {
            return Ok(None);
        }
        shim_manager.status(container_id).map(Some)
    }

    pub fn restore_container_from_checkpoint(
        &self,
        container_id: &str,
        image_path: &Path,
        work_path: &Path,
    ) -> Result<()> {
        std::fs::create_dir_all(work_path).with_context(|| {
            format!(
                "Failed to create restore work directory {}",
                work_path.display()
            )
        })?;
        let bundle_path = self.bundle_path(container_id);

        self.restore_rootfs_snapshot(container_id, image_path)?;

        if let Some(ref shim_manager) = self.shim_manager {
            return shim_manager.restore_task(
                container_id,
                &bundle_path,
                image_path,
                work_path,
                &self.criu_path,
                self.no_pivot,
            );
        }

        let image_path = image_path.to_string_lossy().to_string();
        let work_path = work_path.to_string_lossy().to_string();
        let bundle_path = bundle_path.to_string_lossy().to_string();
        let mut restore_args = vec![
            "restore",
            "-d",
            "--image-path",
            image_path.as_str(),
            "--work-path",
            work_path.as_str(),
            "--bundle",
            bundle_path.as_str(),
        ];
        let criu_path = self.criu_path.to_string_lossy().to_string();
        if !criu_path.is_empty() {
            restore_args.push("--criu");
            restore_args.push(criu_path.as_str());
        }
        if self.no_pivot {
            restore_args.push("--no-pivot");
        }
        restore_args.push(container_id);
        self.runc_exec(&restore_args)
    }

    pub fn pause_container(&self, container_id: &str) -> Result<()> {
        if let Some(ref shim_manager) = self.shim_manager {
            return shim_manager.pause_task(container_id);
        }
        match self.get_runc_state(container_id)? {
            Some(state) if state.status == "paused" => Ok(()),
            Some(state) if state.status == "running" => self.runc_exec(&["pause", container_id]),
            Some(state) => Err(anyhow::anyhow!(
                "container {} cannot be paused from state {}",
                container_id,
                state.status
            )),
            None => Err(anyhow::anyhow!("container {} not found", container_id)),
        }
    }

    pub fn checkpoint_container(
        &self,
        container_id: &str,
        image_path: &Path,
        work_path: &Path,
    ) -> Result<()> {
        if let Some(ref shim_manager) = self.shim_manager {
            return shim_manager.checkpoint_task(container_id, image_path, work_path);
        }
        std::fs::create_dir_all(image_path).with_context(|| {
            format!(
                "Failed to create checkpoint image directory {}",
                image_path.display()
            )
        })?;
        std::fs::create_dir_all(work_path).with_context(|| {
            format!(
                "Failed to create checkpoint work directory {}",
                work_path.display()
            )
        })?;

        let image_path = image_path.to_string_lossy().to_string();
        let work_path = work_path.to_string_lossy().to_string();
        let mut checkpoint_args = vec![
            "checkpoint",
            "--file-locks",
            "--image-path",
            image_path.as_str(),
            "--work-path",
            work_path.as_str(),
            "--leave-running",
        ];
        let criu_path = self.criu_path.to_string_lossy().to_string();
        if !criu_path.is_empty() {
            checkpoint_args.push("--criu");
            checkpoint_args.push(criu_path.as_str());
        }
        checkpoint_args.push(container_id);
        self.runc_exec(&checkpoint_args)
    }

    pub fn resume_container(&self, container_id: &str) -> Result<()> {
        if let Some(ref shim_manager) = self.shim_manager {
            return shim_manager.resume_task(container_id);
        }
        match self.get_runc_state(container_id)? {
            Some(state) if state.status == "running" => Ok(()),
            Some(state) if state.status == "paused" => self.runc_exec(&["resume", container_id]),
            Some(state) => Err(anyhow::anyhow!(
                "container {} cannot be resumed from state {}",
                container_id,
                state.status
            )),
            None => Err(anyhow::anyhow!("container {} not found", container_id)),
        }
    }

    /// 获取容器 init 进程 PID
    pub fn container_pid(&self, container_id: &str) -> Result<Option<i32>> {
        if let Some(ref shim_manager) = self.shim_manager {
            return shim_manager.container_pid(container_id);
        }
        match self.get_runc_state(container_id)? {
            Some(state) if state.pid > 0 => Ok(Some(state.pid)),
            _ => Ok(None),
        }
    }

    pub fn bundle_path_for(&self, container_id: &str) -> PathBuf {
        self.bundle_path(container_id)
    }

    fn normalized_oom_score_adj(&self, preferred: i64) -> Result<i32> {
        let adjusted = if self.restrict_oom_score_adj {
            Self::restrict_oom_score_adj_floor(preferred)?
        } else {
            preferred
        };
        i32::try_from(adjusted)
            .with_context(|| format!("oom_score_adj {adjusted} does not fit in i32"))
    }

    fn apply_oom_score_adj_policy(&self, spec: &mut Spec, requested: Option<i64>) -> Result<()> {
        let preferred = requested.or_else(|| {
            spec.process
                .as_ref()
                .and_then(|process| process.oom_score_adj.map(i64::from))
        });
        let Some(preferred) = preferred else {
            return Ok(());
        };
        let process = spec
            .process
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("OCI spec is missing process configuration"))?;
        process.oom_score_adj = Some(self.normalized_oom_score_adj(preferred)?);
        Ok(())
    }

    pub fn enforce_oom_score_adj_policy(&self, spec: &mut Spec) -> Result<()> {
        self.apply_oom_score_adj_policy(spec, None)
    }

    fn checkpoint_restore_from_annotations(
        annotations: &[(String, String)],
    ) -> Option<RestoreCheckpointMetadata> {
        annotations
            .iter()
            .find(|(key, _)| key == INTERNAL_CHECKPOINT_RESTORE_KEY)
            .and_then(|(_, value)| serde_json::from_str::<RestoreCheckpointMetadata>(value).ok())
    }

    fn resolve_image_dir(&self, image_ref: &str) -> Result<PathBuf> {
        let images_dir = self.image_storage_root.join("images");
        if !images_dir.exists() {
            return Err(ImageAvailabilityError::NotPresentLocally {
                image_ref: image_ref.to_string(),
            }
            .into());
        }
        let entries = std::fs::read_dir(&images_dir)
            .with_context(|| format!("Failed to read images directory: {:?}", images_dir))?;

        for entry in entries {
            let entry = entry?;
            let image_dir = entry.path();
            if !image_dir.is_dir() {
                continue;
            }

            let metadata_path = image_dir.join("metadata.json");
            if !metadata_path.exists() {
                continue;
            }

            let metadata_bytes = std::fs::read(&metadata_path)
                .with_context(|| format!("Failed to read image metadata: {:?}", metadata_path))?;
            let metadata: serde_json::Value = serde_json::from_slice(&metadata_bytes)
                .with_context(|| format!("Failed to parse image metadata: {:?}", metadata_path))?;

            let image_id = metadata
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let repo_tags: Vec<String> = metadata
                .get("repo_tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if repo_tags.iter().any(|tag| tag == image_ref)
                || Self::image_id_matches(image_id, image_ref)
            {
                return Ok(image_dir);
            }
        }

        Err(ImageAvailabilityError::NotPresentLocally {
            image_ref: image_ref.to_string(),
        }
        .into())
    }

    fn normalize_image_id(id: &str) -> &str {
        id.strip_prefix("sha256:").unwrap_or(id)
    }

    fn image_id_matches(image_id: &str, candidate: &str) -> bool {
        if image_id == candidate {
            return true;
        }
        let normalized_image_id = Self::normalize_image_id(image_id);
        let normalized_candidate = Self::normalize_image_id(candidate);
        normalized_image_id == normalized_candidate
            || normalized_image_id.starts_with(normalized_candidate)
            || normalized_candidate.starts_with(normalized_image_id)
    }

    fn image_metadata(&self, image_ref: &str) -> Result<crate::image::ImageMeta> {
        let image_dir = self.resolve_image_dir(image_ref)?;
        let metadata_path = image_dir.join("metadata.json");
        let metadata_bytes = std::fs::read(&metadata_path)
            .with_context(|| format!("Failed to read image metadata: {:?}", metadata_path))?;
        serde_json::from_slice(&metadata_bytes)
            .with_context(|| format!("Failed to parse image metadata: {:?}", metadata_path))
    }

    fn normalized_image_volume_paths(&self, image_ref: &str) -> Result<Vec<PathBuf>> {
        let metadata = self.image_metadata(image_ref)?;
        let mut declared = Vec::new();
        for raw in metadata.declared_volumes {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path = Path::new(trimmed);
            if !path.is_absolute() {
                return Err(anyhow::anyhow!(
                    "image {} declares non-absolute volume path {}",
                    image_ref,
                    trimmed
                ));
            }
            if path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::CurDir
                        | std::path::Component::Prefix(_)
                )
            }) {
                return Err(anyhow::anyhow!(
                    "image {} declares invalid volume path {}",
                    image_ref,
                    trimmed
                ));
            }
            declared.push(path.to_path_buf());
        }
        declared.sort();
        declared.dedup();
        Ok(declared)
    }

    fn ensure_image_volume_directories(&self, image_ref: &str, rootfs: &Path) -> Result<()> {
        if self.image_volumes != ImageVolumesMode::Mkdir {
            return Ok(());
        }
        for volume_path in self.normalized_image_volume_paths(image_ref)? {
            let relative = volume_path
                .strip_prefix("/")
                .context("image volume path must stay absolute")?;
            let target = rootfs.join(relative);
            std::fs::create_dir_all(&target).with_context(|| {
                format!(
                    "Failed to create image-defined volume directory {}",
                    target.display()
                )
            })?;
        }
        Ok(())
    }

    fn ensure_minimum_rootfs_layout(&self, image_ref: &str, rootfs_dir: &Path) -> Result<()> {
        // Ensure minimum runtime paths exist for scratch-like images (e.g. pause).
        std::fs::create_dir_all(rootfs_dir.join("dev"))
            .context("Failed to create /dev in rootfs")?;
        std::fs::create_dir_all(rootfs_dir.join("proc"))
            .context("Failed to create /proc in rootfs")?;
        std::fs::create_dir_all(rootfs_dir.join("sys"))
            .context("Failed to create /sys in rootfs")?;

        self.ensure_image_volume_directories(image_ref, rootfs_dir)?;

        let dev_null = rootfs_dir.join("dev/null");
        if dev_null.exists() {
            let _ = std::fs::remove_file(&dev_null);
        }
        mknod(
            &dev_null,
            SFlag::S_IFCHR,
            Mode::from_bits_truncate(0o666),
            makedev(1, 3),
        )
        .context("Failed to create /dev/null char device in rootfs")?;
        Ok(())
    }

    fn prepare_rootfs_from_image(
        &self,
        image_ref: &str,
        rootfs_dir: &Path,
        container_id: &str,
    ) -> Result<PreparedRootfsMount> {
        use crate::image::snapshotter::{FilesystemSnapshotter, SnapshotMode, Snapshotter};

        let mode = match &self.rootfs_snapshotter {
            RootfsSnapshotter::InternalOverlayUntar => SnapshotMode::InternalOverlayUntar,
            RootfsSnapshotter::InternalCachedRootfs => SnapshotMode::InternalCachedRootfs,
            RootfsSnapshotter::External(name) => {
                return Err(anyhow::anyhow!(
                    "external snapshotter {name} has no mount spec provider attached"
                ))
            }
        };
        let snapshotter = FilesystemSnapshotter::new(
            mode,
            &self.image_storage_root,
            crate::image::metadata_store::FilesystemImageMetadataStore::new(
                &self.image_storage_root,
                Vec::new(),
                self.state_db_path.clone(),
            ),
            crate::image::content_store::FsContentStore::new_with_ledger(
                &self.image_storage_root,
                self.state_db_path.clone(),
            )?,
            self.state_db_path.clone(),
        );
        snapshotter.prepare(container_id, image_ref, rootfs_dir)?;
        self.ensure_minimum_rootfs_layout(image_ref, rootfs_dir)?;
        let mount = if self.state_db_path.is_some() {
            PreparedRootfsMount::from(snapshotter.mount(container_id)?)
        } else {
            PreparedRootfsMount::internal_path(
                container_id.to_string(),
                rootfs_dir.to_path_buf(),
                false,
            )
        };
        info!(
            "Prepared rootfs for container {} from image {} via snapshotter",
            container_id, image_ref
        );
        Ok(mount)
    }

    /// 分步创建：准备 rootfs（NRI 可在后续步骤介入 spec）。
    fn prepare_rootfs_mount(
        &self,
        container_id: &str,
        config: &ContainerConfig,
    ) -> Result<PreparedRootfsMount> {
        let checkpoint_restore = Self::checkpoint_restore_from_annotations(&config.annotations);
        let image_ref = checkpoint_restore
            .as_ref()
            .map(|restore| restore.image_ref.as_str())
            .unwrap_or(config.image.as_str());
        let mount = self
            .prepare_rootfs_from_image(image_ref, &config.rootfs, container_id)
            .context("Failed to prepare rootfs from image")?;
        self.ensure_mount_targets(&config.rootfs, &config.mounts)
            .context("Failed to prepare mount targets")?;
        Ok(mount)
    }

    fn ensure_mount_targets(&self, rootfs: &Path, mounts: &[MountConfig]) -> Result<()> {
        for mount in mounts {
            let relative = mount.destination.strip_prefix("/").with_context(|| {
                format!(
                    "mount destination must be absolute: {}",
                    mount.destination.display()
                )
            })?;
            let target = rootfs.join(relative);
            let source_meta = std::fs::metadata(&mount.source).with_context(|| {
                format!("failed to stat mount source {}", mount.source.display())
            })?;

            if source_meta.is_dir() {
                if target.exists() && !target.is_dir() {
                    return Err(anyhow::anyhow!(
                        "mount target {} already exists and is not a directory",
                        target.display()
                    ));
                }
                std::fs::create_dir_all(&target).with_context(|| {
                    format!(
                        "failed to create mount target directory {}",
                        target.display()
                    )
                })?;
                continue;
            }

            let parent = target.parent().ok_or_else(|| {
                anyhow::anyhow!("mount target {} has no parent directory", target.display())
            })?;
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create mount target parent directory {}",
                    parent.display()
                )
            })?;
            if target.exists() {
                if target.is_dir() {
                    return Err(anyhow::anyhow!(
                        "mount target {} already exists and is a directory",
                        target.display()
                    ));
                }
            } else {
                std::fs::File::create(&target).with_context(|| {
                    format!("failed to create mount target file {}", target.display())
                })?;
            }
        }

        Ok(())
    }

    /// 分步创建：准备 rootfs（NRI 可在后续步骤介入 spec）。
    pub fn prepare_rootfs(
        &self,
        container_id: &str,
        config: &ContainerConfig,
    ) -> Result<PreparedRootfsMount> {
        self.prepare_rootfs_mount(container_id, config)
    }

    fn merge_env_layers(
        &self,
        base: &[(String, String)],
        requested: &[(String, String)],
    ) -> Vec<(String, String)> {
        let mut merged = base.to_vec();
        for (key, value) in &self.default_env {
            if let Some((_, existing_value)) = merged
                .iter_mut()
                .find(|(existing_key, _)| existing_key == key)
            {
                *existing_value = value.clone();
            } else {
                merged.push((key.clone(), value.clone()));
            }
        }
        for (key, value) in requested {
            if let Some((_, existing_value)) = merged
                .iter_mut()
                .find(|(existing_key, _)| existing_key == key)
            {
                *existing_value = value.clone();
            } else {
                merged.push((key.clone(), value.clone()));
            }
        }
        merged
    }

    fn merged_env(&self, requested: &[(String, String)]) -> Vec<(String, String)> {
        self.merge_env_layers(&[], requested)
    }

    fn apply_default_ulimits(&self, process: &mut Process) {
        if !self.default_ulimits.is_empty() {
            process.rlimits = Some(self.default_ulimits.clone());
        }
    }

    fn linux_resources_to_oci(resources: &LinuxContainerResources) -> LinuxResources {
        let limits = ResourceLimits {
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
        };

        let mut oci_resources = to_oci_resources(&limits);
        if !resources.unified.is_empty() {
            oci_resources.unified = Some(resources.unified.clone());
        }
        if !resources.hugepage_limits.is_empty() {
            oci_resources.hugepage_limits = Some(
                resources
                    .hugepage_limits
                    .iter()
                    .map(|limit| crate::oci::spec::LinuxHugepageLimit {
                        page_size: limit.page_size.clone(),
                        limit: limit.limit,
                    })
                    .collect(),
            );
        }

        oci_resources
    }

    fn builtin_runtime_default_seccomp() -> crate::oci::spec::Seccomp {
        let architectures = match std::env::consts::ARCH {
            "x86_64" => vec![
                "SCMP_ARCH_X86_64".to_string(),
                "SCMP_ARCH_X86".to_string(),
                "SCMP_ARCH_X32".to_string(),
            ],
            "aarch64" => vec!["SCMP_ARCH_AARCH64".to_string(), "SCMP_ARCH_ARM".to_string()],
            "riscv64" => vec!["SCMP_ARCH_RISCV64".to_string()],
            "s390x" => vec!["SCMP_ARCH_S390X".to_string(), "SCMP_ARCH_S390".to_string()],
            _ => Vec::new(),
        };
        crate::oci::spec::Seccomp {
            default_action: "SCMP_ACT_ALLOW".to_string(),
            default_errno_ret: None,
            architectures: (!architectures.is_empty()).then_some(architectures),
            flags: None,
            listener_path: None,
            listener_metadata: None,
            syscalls: Some(vec![crate::oci::spec::SeccompSyscall {
                action: "SCMP_ACT_ERRNO".to_string(),
                names: vec![
                    "fsconfig".to_string(),
                    "fsmount".to_string(),
                    "fsopen".to_string(),
                    "fspick".to_string(),
                    "mount".to_string(),
                    "mount_setattr".to_string(),
                    "move_mount".to_string(),
                    "open_tree".to_string(),
                    "pivot_root".to_string(),
                    "setns".to_string(),
                    "umount2".to_string(),
                    "unshare".to_string(),
                ],
                args: None,
                errno_ret: Some(libc::EPERM as u32),
            }]),
        }
    }

    fn load_seccomp_profile(
        &self,
        profile: Option<&SeccompProfile>,
    ) -> Result<Option<crate::oci::spec::Seccomp>> {
        let mut seccomp: Option<crate::oci::spec::Seccomp> = match profile {
            None => None,
            Some(SeccompProfile::RuntimeDefault) => {
                if let Some(path) = self.default_seccomp_profile_path.as_ref() {
                    let content = std::fs::read_to_string(path).with_context(|| {
                        format!("Failed to read default seccomp profile from {:?}", path)
                    })?;
                    let seccomp = serde_json::from_str(&content).with_context(|| {
                        format!(
                            "Failed to parse default seccomp profile JSON from {:?}",
                            path
                        )
                    })?;
                    Some(seccomp)
                } else {
                    Some(Self::builtin_runtime_default_seccomp())
                }
            }
            Some(SeccompProfile::Unconfined) => None,
            Some(SeccompProfile::Localhost(path)) => {
                let content = std::fs::read_to_string(path)
                    .with_context(|| format!("Failed to read seccomp profile from {:?}", path))?;
                let seccomp = serde_json::from_str(&content).with_context(|| {
                    format!("Failed to parse seccomp profile JSON from {:?}", path)
                })?;
                Some(seccomp)
            }
        };

        Ok(seccomp)
    }

    fn build_security_patch(
        &self,
        config: &ContainerConfig,
        existing_cgroup_rules: &[crate::oci::spec::LinuxDeviceCgroup],
        seccomp: Option<crate::oci::spec::Seccomp>,
    ) -> Result<crate::security::spec_patch::SpecSecurityPatch> {
        crate::security::spec_patch::SpecSecurityPatch::from_input(
            crate::security::spec_patch::SpecPatchInput {
                privileged: config.privileged,
                tty: config.tty,
                readonly_rootfs: config.readonly_rootfs,
                no_new_privileges: config.no_new_privileges,
                apparmor_profile: config.apparmor_profile.as_deref(),
                selinux_label: config.selinux_label.as_deref(),
                seccomp,
                capabilities: config.capabilities.as_ref(),
                default_capabilities: &self.default_capabilities,
                add_inheritable_capabilities: self.add_inheritable_capabilities,
                requested_devices: &config.devices,
                additional_devices: &self.additional_devices,
                existing_cgroup_rules,
                allowed_devices: &self.allowed_devices,
                device_ownership_from_security_context: self.device_ownership_from_security_context,
                user: config.user.as_deref(),
                run_as_group: config.run_as_group,
                privileged_without_host_devices: self.privileged_without_host_devices,
                privileged_without_host_devices_all_devices_allowed: self
                    .privileged_without_host_devices_all_devices_allowed,
                rootless: false,
                cgroup_devices_enabled: !self.disable_cgroup,
                disable_proc_mount: self.disable_proc_mount,
                masked_paths: &config.masked_paths,
                readonly_paths: &config.readonly_paths,
            },
        )
    }

    fn oci_cgroups_path(cgroup_parent: Option<&str>, container_id: &str) -> Option<String> {
        let parent = cgroup_parent
            .map(str::trim)
            .filter(|parent| !parent.is_empty())?;
        if parent.ends_with(".slice") {
            let systemd_slice = Path::new(parent)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(parent);
            Some(format!("{systemd_slice}:crius:{container_id}"))
        } else {
            Some(parent.to_string())
        }
    }

    fn merged_sysctls(&self, requested: &HashMap<String, String>) -> HashMap<String, String> {
        let mut merged = self.default_sysctls.clone();
        for (key, value) in requested {
            merged.insert(key.clone(), value.clone());
        }
        merged
    }

    fn requested_oom_score_adj(config: &ContainerConfig) -> Option<i64> {
        config
            .linux_resources
            .as_ref()
            .map(|resources| resources.oom_score_adj)
            .filter(|oom_score_adj| *oom_score_adj != 0)
    }

    fn prefixed_bind_mount_source(&self, source: &Path) -> PathBuf {
        if self.bind_mount_prefix.as_os_str().is_empty() {
            return source.to_path_buf();
        }

        if source.is_absolute() {
            let suffix = source.strip_prefix(Path::new("/")).unwrap_or(source);
            self.bind_mount_prefix.join(suffix)
        } else {
            self.bind_mount_prefix.join(source)
        }
    }

    fn apply_timezone_mount(&self, mounts: &mut Vec<Mount>, env: &mut Vec<String>) -> Result<()> {
        let timezone = self.timezone.trim();
        if timezone.is_empty()
            || mounts
                .iter()
                .any(|mount| mount.destination == "/etc/localtime")
        {
            return Ok(());
        }

        let source = if timezone == "Local" {
            PathBuf::from("/etc/localtime")
        } else {
            Path::new("/usr/share/zoneinfo").join(timezone)
        };
        if !source.exists() {
            return Err(anyhow::anyhow!(
                "configured timezone source does not exist: {}",
                source.display()
            ));
        }

        mounts.push(Mount {
            destination: "/etc/localtime".to_string(),
            source: Some(
                self.prefixed_bind_mount_source(&source)
                    .display()
                    .to_string(),
            ),
            mount_type: Some("bind".to_string()),
            options: Some(vec![
                "bind".to_string(),
                "ro".to_string(),
                "nodev".to_string(),
                "nosuid".to_string(),
                "noexec".to_string(),
            ]),
        });

        if timezone != "Local" && !env.iter().any(|entry| entry.starts_with("TZ=")) {
            env.push(format!("TZ={timezone}"));
        }

        Ok(())
    }

    fn spec_from_restore_template(
        &self,
        container_id: &str,
        config: &ContainerConfig,
        restore: &RestoreCheckpointMetadata,
    ) -> Result<Spec> {
        let mut spec: Spec = serde_json::from_value(restore.oci_config.clone())
            .context("Failed to parse OCI config from checkpoint artifact")?;

        spec.root = Some(Root {
            path: config.rootfs.to_string_lossy().to_string(),
            readonly: None,
        });
        spec.hostname = config.hostname.clone().or(spec.hostname);

        if let Some(process) = spec.process.as_mut() {
            process.terminal = Some(config.tty);
            if let Some(working_dir) = config.working_dir.as_ref() {
                process.cwd = working_dir.to_string_lossy().to_string();
            }
            if !config.command.is_empty() {
                let mut args = config.command.clone();
                args.extend(config.args.clone());
                process.args = args;
            }
            let merged_env = self.merged_env(&config.env);
            if !merged_env.is_empty() {
                process.env = Some(
                    merged_env
                        .iter()
                        .map(|(key, value)| format!("{}={}", key, value))
                        .collect(),
                );
            }
            self.apply_default_ulimits(process);
        }

        let mut existing_cgroup_rules = Vec::new();
        if !self.disable_cgroup {
            if let Some(resources) = config
                .linux_resources
                .as_ref()
                .map(Self::linux_resources_to_oci)
            {
                existing_cgroup_rules = resources.devices.clone().unwrap_or_default();
            }
        }
        let seccomp = self.load_seccomp_profile(
            config.seccomp_profile.as_ref(),
        )?;
        let security_patch = self.build_security_patch(config, &existing_cgroup_rules, seccomp)?;
        security_patch.apply_root(
            spec.root
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("OCI restore spec is missing root configuration"))?,
        );
        if let Some(process) = spec.process.as_mut() {
            security_patch.apply_process(process);
        }

        let linux = spec.linux.get_or_insert(Linux {
            namespaces: None,
            uid_mappings: None,
            gid_mappings: None,
            devices: None,
            net_devices: None,
            cgroups_path: None,
            resources: None,
            rootfs_propagation: None,
            seccomp: None,
            sysctl: None,
            mount_label: None,
            masked_paths: None,
            readonly_paths: None,
            intel_rdt: None,
        });
        linux.namespaces = Some(self.build_namespaces(config));
        linux.cgroups_path = if self.disable_cgroup {
            None
        } else {
            Self::oci_cgroups_path(config.cgroup_parent.as_deref(), container_id)
        };
        let merged_sysctls = self.merged_sysctls(&config.sysctls);
        if !merged_sysctls.is_empty() {
            linux.sysctl = Some(merged_sysctls);
        }
        if !self.disable_cgroup {
            if let Some(resources) = config
                .linux_resources
                .as_ref()
                .map(Self::linux_resources_to_oci)
            {
                linux.resources = Some(resources);
            }
            if let Some(limit) = config.pids_limit {
                linux
                    .resources
                    .get_or_insert(LinuxResources {
                        network: None,
                        pids: None,
                        memory: None,
                        cpu: None,
                        block_io: None,
                        hugepage_limits: None,
                        devices: None,
                        intel_rdt: None,
                        unified: None,
                    })
                    .pids = Some(LinuxPids { limit });
            }
        }
        security_patch.apply_linux(linux);
        self.apply_oom_score_adj_policy(&mut spec, Self::requested_oom_score_adj(config))?;
        let process_env = spec
            .process
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("OCI restore spec is missing process configuration"))?
            .env
            .get_or_insert_with(Vec::new);
        let mounts = spec.mounts.get_or_insert_with(Vec::new);
        self.apply_timezone_mount(mounts, process_env)?;

        let mut annotations = spec.annotations.unwrap_or_default();
        annotations.insert(
            "org.opencontainers.image.ref.name".to_string(),
            restore.image_ref.clone(),
        );
        Self::insert_label_annotation(&mut annotations, &config.labels)?;
        for (key, value) in &config.annotations {
            annotations.insert(key.clone(), value.clone());
        }
        spec.annotations = Some(annotations);

        Ok(spec)
    }

    fn insert_label_annotation(
        annotations: &mut std::collections::HashMap<String, String>,
        labels: &[(String, String)],
    ) -> Result<()> {
        if labels.is_empty() {
            return Ok(());
        }
        let encoded = serde_json::to_string(
            &labels
                .iter()
                .cloned()
                .collect::<std::collections::HashMap<_, _>>(),
        )
        .context("Failed to encode container labels as annotation")?;
        annotations.insert(CRIO_LABELS_ANNOTATION.to_string(), encoded);
        Ok(())
    }

    fn host_namespace_path(ns_type: &str) -> Option<String> {
        let path = PathBuf::from(format!("/proc/1/ns/{}", ns_type));
        path.exists().then(|| path.to_string_lossy().to_string())
    }

    fn target_namespace_path(&self, target_container_id: &str, ns_type: &str) -> Option<String> {
        if target_container_id.is_empty() {
            return None;
        }

        let pid = self.container_pid(target_container_id).ok().flatten()?;
        (pid > 0).then(|| format!("/proc/{}/ns/{}", pid, ns_type))
    }

    fn build_namespaces(&self, config: &ContainerConfig) -> Vec<OciNamespace> {
        let mut namespaces = Spec::default_namespaces();
        let options = config.namespace_options.as_ref();

        for namespace in &mut namespaces {
            match namespace.ns_type.as_str() {
                "network" => {
                    if let Some(path) = &config.namespace_paths.network {
                        namespace.path = Some(path.to_string_lossy().to_string());
                    } else if matches!(
                        options.map(|o| o.network),
                        Some(mode) if mode == NamespaceMode::Node as i32
                    ) {
                        namespace.path = Self::host_namespace_path("net");
                    }
                }
                "pid" => {
                    if let Some(path) = &config.namespace_paths.pid {
                        namespace.path = Some(path.to_string_lossy().to_string());
                    } else if matches!(
                        options.map(|o| o.pid),
                        Some(mode) if mode == NamespaceMode::Node as i32
                    ) {
                        namespace.path = Self::host_namespace_path("pid");
                    } else if matches!(
                        options.map(|o| o.pid),
                        Some(mode) if mode == NamespaceMode::Target as i32
                    ) {
                        namespace.path =
                            options.and_then(|o| self.target_namespace_path(&o.target_id, "pid"));
                    }
                }
                "ipc" => {
                    if let Some(path) = &config.namespace_paths.ipc {
                        namespace.path = Some(path.to_string_lossy().to_string());
                    } else if matches!(
                        options.map(|o| o.ipc),
                        Some(mode) if mode == NamespaceMode::Node as i32
                    ) {
                        namespace.path = Self::host_namespace_path("ipc");
                    } else if matches!(
                        options.map(|o| o.ipc),
                        Some(mode) if mode == NamespaceMode::Target as i32
                    ) {
                        namespace.path =
                            options.and_then(|o| self.target_namespace_path(&o.target_id, "ipc"));
                    }
                }
                "uts" => {
                    if let Some(path) = &config.namespace_paths.uts {
                        namespace.path = Some(path.to_string_lossy().to_string());
                    }
                }
                _ => {}
            }
        }

        namespaces
    }

    /// 分步创建：构建 pristine OCI spec。
    pub fn build_spec(&self, container_id: &str, config: &ContainerConfig) -> Result<Spec> {
        let checkpoint_restore = Self::checkpoint_restore_from_annotations(&config.annotations);
        if let Some(checkpoint_restore) = checkpoint_restore.as_ref() {
            self.spec_from_restore_template(container_id, config, checkpoint_restore)
                .context("Failed to create OCI spec from checkpoint artifact")
        } else {
            self.create_spec(config, container_id)
                .context("Failed to create OCI spec")
        }
    }

    fn base_spec_template(&self) -> Spec {
        self.base_runtime_spec
            .clone()
            .unwrap_or_else(|| Spec::new("1.0.2"))
    }

    fn image_runtime_defaults(&self, image_ref: &str) -> Result<ImageRuntimeDefaults> {
        if image_ref.trim().is_empty() {
            return Ok(ImageRuntimeDefaults::default());
        }

        let metadata = match self.image_metadata(image_ref) {
            Ok(metadata) => metadata,
            Err(err) => {
                if matches!(
                    err.downcast_ref::<ImageAvailabilityError>(),
                    Some(ImageAvailabilityError::NotPresentLocally { .. })
                ) {
                    return Ok(ImageRuntimeDefaults::default());
                }
                return Err(err);
            }
        };
        let env = metadata
            .config_env
            .iter()
            .filter_map(|entry| {
                let (key, value) = entry.split_once('=').unwrap_or((entry.as_str(), ""));
                let key = key.trim();
                (!key.is_empty()).then(|| (key.to_string(), value.to_string()))
            })
            .collect();

        Ok(ImageRuntimeDefaults {
            env,
            entrypoint: metadata.config_entrypoint,
            cmd: metadata.config_cmd,
            working_dir: metadata.config_working_dir.map(PathBuf::from),
        })
    }

    fn build_user(config: &ContainerConfig) -> Option<User> {
        let additional_gids = if config.supplemental_groups.is_empty() {
            None
        } else {
            Some(config.supplemental_groups.clone())
        };

        let Some(user) = config.user.as_ref() else {
            if config.run_as_group.is_none() && additional_gids.is_none() {
                return None;
            }
            return Some(User {
                uid: 0,
                gid: config.run_as_group.unwrap_or(0),
                additional_gids,
                username: None,
            });
        };

        if let Ok(uid) = user.parse::<u32>() {
            Some(User {
                uid,
                gid: config.run_as_group.unwrap_or(uid),
                additional_gids,
                username: None,
            })
        } else {
            Some(User {
                uid: 0,
                gid: config.run_as_group.unwrap_or(0),
                additional_gids,
                username: Some(user.clone()),
            })
        }
    }

    fn should_reject_absent_mount_source(&self, source: &Path) -> bool {
        self.absent_mount_sources_to_reject
            .iter()
            .any(|entry| entry == source)
    }

    fn resolve_bind_mount_source(
        &self,
        source: &Path,
        policy: MissingMountSourcePolicy,
    ) -> Result<Option<PathBuf>> {
        let prefixed = self.prefixed_bind_mount_source(source);
        if !prefixed.exists() {
            if self.should_reject_absent_mount_source(&prefixed) {
                return Err(anyhow::anyhow!(
                    "cannot mount {}: path does not exist and is configured to be rejected",
                    prefixed.display()
                ));
            }
            match policy {
                MissingMountSourcePolicy::Reject => {
                    return Err(anyhow::anyhow!(
                        "cannot mount {}: path does not exist",
                        prefixed.display()
                    ));
                }
                MissingMountSourcePolicy::Ignore => return Ok(None),
                MissingMountSourcePolicy::CreateDirectory => {
                    std::fs::create_dir_all(&prefixed).with_context(|| {
                        format!(
                            "failed to create missing bind mount source directory {}",
                            prefixed.display()
                        )
                    })?;
                }
            }
        }

        std::fs::canonicalize(&prefixed)
            .with_context(|| format!("failed to resolve mount source {}", prefixed.display()))
            .map(Some)
    }

    fn decode_mountinfo_path(raw: &str) -> PathBuf {
        let mut decoded = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                let mut octal = String::new();
                for _ in 0..3 {
                    if let Some(next) = chars.peek().copied() {
                        if ('0'..='7').contains(&next) {
                            octal.push(next);
                            chars.next();
                            continue;
                        }
                    }
                    break;
                }
                if octal.len() == 3 {
                    if let Ok(value) = u8::from_str_radix(&octal, 8) {
                        decoded.push(value as char);
                        continue;
                    }
                }
                decoded.push('\\');
                decoded.push_str(&octal);
                continue;
            }
            decoded.push(ch);
        }
        PathBuf::from(decoded)
    }

    fn is_path_within_mountpoint(path: &Path, mount_point: &Path) -> bool {
        path == mount_point || path.starts_with(mount_point)
    }

    fn mount_source_propagation_for_path_from_mountinfo(
        mountinfo: &str,
        path: &Path,
    ) -> Result<Option<(bool, bool)>> {
        let canonical = std::fs::canonicalize(path)
            .with_context(|| format!("failed to resolve mount source {}", path.display()))?;
        let mut best_match: Option<(usize, bool, bool)> = None;

        for line in mountinfo.lines() {
            let fields: Vec<&str> = line.split(' ').collect();
            let Some(separator_index) = fields.iter().position(|field| *field == "-") else {
                continue;
            };
            if separator_index < 6 || fields.len() <= separator_index + 3 {
                continue;
            }

            let mount_point = Self::decode_mountinfo_path(fields[4]);
            if !Self::is_path_within_mountpoint(&canonical, &mount_point) {
                continue;
            }

            let mut shared = false;
            let mut slave = false;
            for field in &fields[6..separator_index] {
                if field.starts_with("shared:") {
                    shared = true;
                } else if field.starts_with("master:") {
                    slave = true;
                }
            }

            let score = mount_point.components().count();
            if best_match
                .as_ref()
                .map(|(best_score, _, _)| score > *best_score)
                .unwrap_or(true)
            {
                best_match = Some((score, shared, slave));
            }
        }

        Ok(best_match.map(|(_, shared, slave)| (shared, slave)))
    }

    fn validate_mount_propagation_from_mountinfo(
        source: &Path,
        destination: &Path,
        propagation: MountPropagationMode,
        mountinfo: &str,
    ) -> std::result::Result<(), MountSemanticsError> {
        let Some((shared, slave)) =
            Self::mount_source_propagation_for_path_from_mountinfo(mountinfo, source).map_err(
                |err| MountSemanticsError::MountPropagationInspectionFailed {
                    source_path: source.to_path_buf(),
                    message: err.to_string(),
                },
            )?
        else {
            return Err(MountSemanticsError::MountPropagationInspectionFailed {
                source_path: source.to_path_buf(),
                message: "mount point not found in /proc/self/mountinfo".to_string(),
            });
        };

        match propagation {
            MountPropagationMode::Private => Ok(()),
            MountPropagationMode::HostToContainer if shared || slave => Ok(()),
            MountPropagationMode::HostToContainer => Err(
                MountSemanticsError::HostToContainerPropagationRequiresSharedOrSlave {
                    source_path: source.to_path_buf(),
                    destination: destination.to_path_buf(),
                },
            ),
            MountPropagationMode::Bidirectional if shared => Ok(()),
            MountPropagationMode::Bidirectional => Err(
                MountSemanticsError::BidirectionalPropagationRequiresShared {
                    source_path: source.to_path_buf(),
                    destination: destination.to_path_buf(),
                },
            ),
        }
    }

    fn validate_mount_propagation(
        &self,
        source: &Path,
        destination: &Path,
        propagation: MountPropagationMode,
    ) -> std::result::Result<(), MountSemanticsError> {
        if matches!(propagation, MountPropagationMode::Private) {
            return Ok(());
        }

        let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").map_err(|err| {
            MountSemanticsError::MountPropagationInspectionFailed {
                source_path: source.to_path_buf(),
                message: err.to_string(),
            }
        })?;
        Self::validate_mount_propagation_from_mountinfo(
            source,
            destination,
            propagation,
            &mountinfo,
        )
    }

    fn validate_mount_semantics(
        &self,
        mounts: &[MountConfig],
        mount_label: Option<&str>,
    ) -> std::result::Result<(), MountSemanticsError> {
        let needs_rro_support = mounts.iter().any(|mount| mount.recursive_read_only);
        let needs_idmap_support = mounts
            .iter()
            .any(|mount| !mount.uid_mappings.is_empty() || !mount.gid_mappings.is_empty());
        let features =
            (needs_rro_support || needs_idmap_support).then(|| self.probe_runtime_features());

        for mount in mounts {
            let Some(source) = self
                .resolve_bind_mount_source(&mount.source, mount.missing_source_policy)
                .map_err(|_| MountSemanticsError::MissingSource {
                    source_path: self.prefixed_bind_mount_source(&mount.source),
                    destination: mount.destination.clone(),
                })?
            else {
                continue;
            };

            if mount.selinux_relabel && mount_label.is_none() {
                return Err(MountSemanticsError::SelinuxRelabelRequiresMountLabel {
                    destination: mount.destination.clone(),
                });
            }

            self.validate_mount_propagation(&source, &mount.destination, mount.propagation)?;

            if mount.recursive_read_only {
                if !source.is_dir() {
                    return Err(MountSemanticsError::RecursiveReadOnlyRequiresDirectory {
                        source_path: source,
                        destination: mount.destination.clone(),
                    });
                }
                let supported = features
                    .as_ref()
                    .map(|probe| probe.available && probe.recursive_read_only_mounts)
                    .unwrap_or(false);
                if !supported {
                    return Err(MountSemanticsError::RecursiveReadOnlyUnsupported {
                        destination: mount.destination.clone(),
                    });
                }
            }

            if !mount.uid_mappings.is_empty() || !mount.gid_mappings.is_empty() {
                let supported = features
                    .as_ref()
                    .map(|probe| probe.available && probe.idmap_mounts)
                    .unwrap_or(false);
                if !supported {
                    return Err(MountSemanticsError::IdmapMountUnsupported {
                        destination: mount.destination.clone(),
                    });
                }
            }
        }

        Ok(())
    }

    fn parse_default_mounts_file(&self) -> Result<Vec<(PathBuf, PathBuf)>> {
        let Some(path) = self.default_mounts_file.as_ref() else {
            return Ok(Vec::new());
        };
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read default mounts file {}", path.display()))?;
        let mut mounts = Vec::new();
        for (index, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((src, dst)) = line.split_once(':') else {
                return Err(anyhow::anyhow!(
                    "invalid default mounts entry at {}:{}; expected /SRC:/DST",
                    path.display(),
                    index + 1
                ));
            };
            let src = PathBuf::from(src.trim());
            let dst = PathBuf::from(dst.trim());
            if !src.is_absolute() || !dst.is_absolute() {
                return Err(anyhow::anyhow!(
                    "default mounts entry at {}:{} must use absolute /SRC:/DST paths",
                    path.display(),
                    index + 1
                ));
            }
            mounts.push((src, dst));
        }
        Ok(mounts)
    }

    fn relabel_mount_source(&self, source: &Path, mount_label: &str) -> Result<()> {
        let mut command = Command::new("chcon");
        if source.is_dir() {
            command.arg("-R");
        }
        let output = command
            .arg(mount_label)
            .arg(source)
            .output()
            .with_context(|| format!("failed to relabel mount source {}", source.display()))?;
        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("status={}", output.status)
        };
        Err(anyhow::anyhow!(
            "failed to relabel mount source {}: {}",
            source.display(),
            detail
        ))
    }

    fn bind_mount_options_for_source(
        &self,
        source: &Path,
        mount: &MountConfig,
    ) -> Result<Vec<String>> {
        let recursive = source.is_dir();
        let mut options = if recursive {
            vec!["rbind".to_string()]
        } else {
            vec!["bind".to_string()]
        };
        options.push(mount.propagation.option().to_string());
        options.push(if mount.read_only { "ro" } else { "rw" }.to_string());
        if mount.recursive_read_only {
            options.push("rro".to_string());
        }
        if !mount.uid_mappings.is_empty() {
            options.push(format!(
                "{INTERNAL_UID_MAPPINGS_MOUNT_OPTION_PREFIX}{}",
                serde_json::to_string(&mount.uid_mappings)
                    .context("failed to encode mount uid mappings")?
            ));
        }
        if !mount.gid_mappings.is_empty() {
            options.push(format!(
                "{INTERNAL_GID_MAPPINGS_MOUNT_OPTION_PREFIX}{}",
                serde_json::to_string(&mount.gid_mappings)
                    .context("failed to encode mount gid mappings")?
            ));
        }
        Ok(options)
    }

    fn build_bind_mount(
        &self,
        mount: &MountConfig,
        mount_label: Option<&str>,
    ) -> Result<Option<Mount>> {
        let Some(source) =
            self.resolve_bind_mount_source(&mount.source, mount.missing_source_policy)?
        else {
            return Ok(None);
        };

        if mount.selinux_relabel {
            let label = mount_label.ok_or_else(|| {
                anyhow::anyhow!(
                    "mount {} requests SELinux relabel but the container has no SELinux mount label",
                    mount.destination.display()
                )
            })?;
            self.relabel_mount_source(&source, label)?;
        }

        Ok(Some(Mount {
            destination: mount.destination.to_string_lossy().to_string(),
            source: Some(source.to_string_lossy().to_string()),
            mount_type: Some("bind".to_string()),
            options: Some(self.bind_mount_options_for_source(&source, mount)?),
        }))
    }

    fn image_volume_state_dir(&self, container_id: &str) -> PathBuf {
        self.root.join("image-volumes").join(container_id)
    }

    fn image_volume_mounts(
        &self,
        container_id: &str,
        config: &ContainerConfig,
    ) -> Result<Vec<Mount>> {
        if self.image_volumes != ImageVolumesMode::Bind {
            return Ok(Vec::new());
        }

        let overridden_destinations: HashSet<PathBuf> = config
            .mounts
            .iter()
            .map(|mount| mount.destination.clone())
            .collect();
        let state_dir = self.image_volume_state_dir(container_id);
        let mut mounts = Vec::new();
        for (index, destination) in self
            .normalized_image_volume_paths(&config.image)?
            .into_iter()
            .enumerate()
        {
            if overridden_destinations.contains(&destination) {
                continue;
            }
            let source = state_dir.join(format!("volume-{index}"));
            std::fs::create_dir_all(&source).with_context(|| {
                format!(
                    "Failed to create image-defined volume source {}",
                    source.display()
                )
            })?;
            mounts.push(Mount {
                destination: destination.display().to_string(),
                source: Some(source.display().to_string()),
                mount_type: Some("bind".to_string()),
                options: Some(vec![
                    "private".to_string(),
                    "bind".to_string(),
                    "rw".to_string(),
                ]),
            });
        }
        Ok(mounts)
    }

    fn build_mounts(
        &self,
        container_id: &str,
        config: &ContainerConfig,
        keep_hugepages_mount: bool,
    ) -> Result<Vec<Mount>> {
        self.validate_mount_semantics(&config.mounts, config.selinux_label.as_deref())
            .map_err(anyhow::Error::from)?;
        let mut extra_mounts: Vec<Mount> = self
            .parse_default_mounts_file()?
            .into_iter()
            .filter_map(|(source, destination)| {
                self.build_bind_mount(
                    &MountConfig {
                        source,
                        destination,
                        read_only: false,
                        missing_source_policy: MissingMountSourcePolicy::Ignore,
                        selinux_relabel: false,
                        propagation: MountPropagationMode::Private,
                        recursive_read_only: false,
                        uid_mappings: Vec::new(),
                        gid_mappings: Vec::new(),
                        requested_image: None,
                        image_sub_path: None,
                    },
                    config.selinux_label.as_deref(),
                )
                .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        extra_mounts.extend(self.image_volume_mounts(container_id, config)?);
        for mount in &config.mounts {
            if let Some(custom_mount) =
                self.build_bind_mount(mount, config.selinux_label.as_deref())?
            {
                extra_mounts.retain(|existing| existing.destination != custom_mount.destination);
                extra_mounts.push(custom_mount);
            }
        }

        let mut overridden_destinations: HashSet<String> = extra_mounts
            .iter()
            .map(|mount| mount.destination.clone())
            .collect();
        if Self::host_ipc_enabled(config) {
            overridden_destinations.insert("/dev/shm".to_string());
            overridden_destinations.insert("/dev/mqueue".to_string());
        }

        let mut mounts: Vec<Mount> = Spec::default_mounts()
            .into_iter()
            .filter(|mount| {
                !overridden_destinations.contains(&mount.destination)
                    && mount.destination != "/sys/fs/cgroup"
                    && (keep_hugepages_mount || mount.destination != "/dev/hugepages")
            })
            .collect();

        if Self::host_ipc_enabled(config) {
            if let Some(dev_shm) = self.build_bind_mount(
                &MountConfig {
                    source: PathBuf::from("/dev/shm"),
                    destination: PathBuf::from("/dev/shm"),
                    read_only: false,
                    missing_source_policy: MissingMountSourcePolicy::Ignore,
                    selinux_relabel: false,
                    propagation: MountPropagationMode::Private,
                    recursive_read_only: false,
                    uid_mappings: Vec::new(),
                    gid_mappings: Vec::new(),
                    requested_image: None,
                    image_sub_path: None,
                },
                config.selinux_label.as_deref(),
            )? {
                mounts.push(dev_shm);
            }
            if let Some(dev_mqueue) = self.build_bind_mount(
                &MountConfig {
                    source: PathBuf::from("/dev/mqueue"),
                    destination: PathBuf::from("/dev/mqueue"),
                    read_only: false,
                    missing_source_policy: MissingMountSourcePolicy::Ignore,
                    selinux_relabel: false,
                    propagation: MountPropagationMode::Private,
                    recursive_read_only: false,
                    uid_mappings: Vec::new(),
                    gid_mappings: Vec::new(),
                    requested_image: None,
                    image_sub_path: None,
                },
                config.selinux_label.as_deref(),
            )? {
                mounts.push(dev_mqueue);
            }
        }

        mounts.extend(extra_mounts);
        Ok(mounts)
    }

    fn host_ipc_enabled(config: &ContainerConfig) -> bool {
        matches!(
            config.namespace_options.as_ref().map(|options| options.ipc),
            Some(mode) if mode == NamespaceMode::Node as i32
        )
    }

    fn merge_hook_lists(
        base: &mut Option<Vec<crate::oci::spec::Hook>>,
        extra: Option<Vec<crate::oci::spec::Hook>>,
    ) {
        let Some(extra) = extra else {
            return;
        };
        base.get_or_insert_with(Vec::new).extend(extra);
    }

    fn merge_hooks(
        mut base: crate::oci::spec::Hooks,
        extra: crate::oci::spec::Hooks,
    ) -> crate::oci::spec::Hooks {
        Self::merge_hook_lists(&mut base.prestart, extra.prestart);
        Self::merge_hook_lists(&mut base.create_runtime, extra.create_runtime);
        Self::merge_hook_lists(&mut base.create_container, extra.create_container);
        Self::merge_hook_lists(&mut base.start_container, extra.start_container);
        Self::merge_hook_lists(&mut base.poststart, extra.poststart);
        Self::merge_hook_lists(&mut base.poststop, extra.poststop);
        base
    }

    fn parse_hooks_file(path: &Path) -> Result<Option<crate::oci::spec::Hooks>> {
        let raw = std::fs::read(path)
            .with_context(|| format!("Failed to read OCI hooks file {}", path.display()))?;
        if let Ok(spec) = serde_json::from_slice::<crate::oci::spec::Spec>(&raw) {
            if spec.hooks.is_some() {
                return Ok(spec.hooks);
            }
        }
        let hooks = serde_json::from_slice::<crate::oci::spec::Hooks>(&raw)
            .with_context(|| format!("Failed to parse OCI hooks file {}", path.display()))?;
        Ok(Some(hooks))
    }

    fn load_configured_hooks(&self) -> Result<Option<crate::oci::spec::Hooks>> {
        let mut selected = std::collections::BTreeMap::<String, PathBuf>::new();
        for dir in &self.hooks_dirs {
            if !dir.exists() {
                continue;
            }
            if !dir.is_dir() {
                return Err(anyhow::anyhow!(
                    "configured hooks directory is not a directory: {}",
                    dir.display()
                ));
            }
            for entry in std::fs::read_dir(dir)
                .with_context(|| format!("Failed to read hooks directory {}", dir.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json")
                {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    selected.insert(name.to_string(), path);
                }
            }
        }

        let mut merged: Option<crate::oci::spec::Hooks> = None;
        for path in selected.into_values() {
            if let Some(hooks) = Self::parse_hooks_file(&path)? {
                merged = Some(match merged.take() {
                    Some(existing) => Self::merge_hooks(existing, hooks),
                    None => hooks,
                });
            }
        }
        Ok(merged)
    }

    fn build_user_namespace_mappings(config: &ContainerConfig) -> Result<UserNamespaceMappings> {
        let Some(userns) = config
            .namespace_options
            .as_ref()
            .and_then(|options| options.userns_options.as_ref())
        else {
            return Ok((None, None));
        };

        match userns.mode {
            mode if mode == NamespaceMode::Node as i32 => {
                if !userns.uids.is_empty() || !userns.gids.is_empty() {
                    return Err(anyhow::anyhow!(
                        "user namespace mode NODE must not include uid/gid mappings"
                    ));
                }
                Ok((None, None))
            }
            mode if mode == NamespaceMode::Pod as i32 => {
                if userns.uids.is_empty() || userns.gids.is_empty() {
                    return Err(anyhow::anyhow!(
                        "user namespace mode POD requires both uid and gid mappings"
                    ));
                }

                let uid_mappings = userns
                    .uids
                    .iter()
                    .map(|mapping| crate::oci::spec::IdMapping {
                        container_id: mapping.container_id,
                        host_id: mapping.host_id,
                        size: mapping.length,
                    })
                    .collect();
                let gid_mappings = userns
                    .gids
                    .iter()
                    .map(|mapping| crate::oci::spec::IdMapping {
                        container_id: mapping.container_id,
                        host_id: mapping.host_id,
                        size: mapping.length,
                    })
                    .collect();
                Ok((Some(uid_mappings), Some(gid_mappings)))
            }
            other => Err(anyhow::anyhow!("unsupported user namespace mode {}", other)),
        }
    }

    fn merge_rootfs_propagation(
        current: Option<&str>,
        requested: Option<&str>,
    ) -> Option<&'static str> {
        fn rank(value: &str) -> u8 {
            match value {
                "rshared" => 2,
                "rslave" => 1,
                _ => 0,
            }
        }

        match (current, requested) {
            (Some(existing), Some(candidate)) => {
                if rank(existing) >= rank(candidate) {
                    match existing {
                        "rshared" => Some("rshared"),
                        "rslave" => Some("rslave"),
                        _ => None,
                    }
                } else {
                    match candidate {
                        "rshared" => Some("rshared"),
                        "rslave" => Some("rslave"),
                        _ => None,
                    }
                }
            }
            (Some(existing), None) => match existing {
                "rshared" => Some("rshared"),
                "rslave" => Some("rslave"),
                _ => None,
            },
            (None, Some(candidate)) => match candidate {
                "rshared" => Some("rshared"),
                "rslave" => Some("rslave"),
                _ => None,
            },
            (None, None) => None,
        }
    }

    /// 创建OCI配置
    fn create_spec(&self, config: &ContainerConfig, container_id: &str) -> Result<Spec> {
        let mut spec = self.base_spec_template();
        let image_defaults = self.image_runtime_defaults(&config.image)?;

        // 设置root配置
        spec.root = Some(Root {
            path: config.rootfs.to_string_lossy().to_string(),
            readonly: None,
        });

        // 设置进程配置
        let mut args = if !config.command.is_empty() {
            let mut resolved = config.command.clone();
            if !config.args.is_empty() {
                resolved.extend(config.args.clone());
            }
            resolved
        } else {
            let mut resolved = image_defaults.entrypoint.clone();
            if !config.args.is_empty() {
                resolved.extend(config.args.clone());
            } else if !image_defaults.cmd.is_empty() {
                resolved.extend(image_defaults.cmd.clone());
            }
            resolved
        };

        // 如果没有命令，使用默认shell
        if args.is_empty() {
            args = vec!["sh".to_string()];
        }

        // 转换环境变量为字符串格式
        let env: Vec<String> = self
            .merge_env_layers(&image_defaults.env, &config.env)
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        let existing_process = spec.process.clone();
        let mut process = existing_process.unwrap_or(Process {
            terminal: None,
            user: None,
            args: Vec::new(),
            env: None,
            cwd: "/".to_string(),
            capabilities: None,
            rlimits: None,
            oom_score_adj: None,
            scheduler: None,
            no_new_privileges: None,
            apparmor_profile: None,
            selinux_label: None,
            io_priority: None,
        });
        process.terminal = Some(config.tty);
        process.user = Self::build_user(config);
        process.args = args;
        process.env = if env.is_empty() { None } else { Some(env) };
        process.cwd = config
            .working_dir
            .as_ref()
            .or(image_defaults.working_dir.as_ref())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        self.apply_default_ulimits(&mut process);
        spec.process = Some(process);

        self.apply_oom_score_adj_policy(&mut spec, Self::requested_oom_score_adj(config))?;

        // 设置主机名
        spec.hostname = config.hostname.clone();

        // 设置挂载点
        let keep_hugepages_mount = std::env::var("CRIUS_ENABLE_HUGEPAGES_MOUNT")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let mut mounts = self.build_mounts(container_id, config, keep_hugepages_mount)?;
        let process_env = spec
            .process
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("OCI spec is missing process configuration"))?
            .env
            .get_or_insert_with(Vec::new);
        self.apply_timezone_mount(&mut mounts, process_env)?;
        spec.mounts = Some(mounts);
        if let Some(hooks) = self.load_configured_hooks()? {
            spec.hooks = Some(match spec.hooks.take() {
                Some(existing) => Self::merge_hooks(existing, hooks),
                None => hooks,
            });
        }

        let mut resources = config
            .linux_resources
            .as_ref()
            .map(Self::linux_resources_to_oci)
            .unwrap_or_else(|| LinuxResources {
                network: None,
                pids: None,
                memory: None,
                cpu: None,
                block_io: None,
                hugepage_limits: None,
                devices: None,
                intel_rdt: None,
                unified: None,
            });

        if let Some(limit) = config.pids_limit {
            resources.pids = Some(LinuxPids { limit });
        }

        let seccomp = self.load_seccomp_profile(
            config.seccomp_profile.as_ref(),
        )?;
        let security_patch = self.build_security_patch(
            config,
            resources.devices.as_deref().unwrap_or(&[]),
            seccomp,
        )?;
        security_patch.apply_root(
            spec.root
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("OCI spec is missing root configuration"))?,
        );
        security_patch.apply_process(
            spec.process
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("OCI spec is missing process configuration"))?,
        );

        let mut namespaces = self.build_namespaces(config);
        let (uid_mappings, gid_mappings) = Self::build_user_namespace_mappings(config)?;
        if uid_mappings.is_some()
            && !namespaces
                .iter()
                .any(|namespace| namespace.ns_type == "user")
        {
            namespaces.push(OciNamespace {
                ns_type: "user".to_string(),
                path: None,
            });
        }

        let mut linux = spec.linux.unwrap_or(Linux {
            namespaces: None,
            uid_mappings: None,
            gid_mappings: None,
            devices: None,
            net_devices: None,
            cgroups_path: None,
            resources: None,
            rootfs_propagation: None,
            seccomp: None,
            sysctl: None,
            mount_label: None,
            masked_paths: None,
            readonly_paths: None,
            intel_rdt: None,
        });
        linux.namespaces = Some(namespaces);
        linux.uid_mappings = uid_mappings;
        linux.gid_mappings = gid_mappings;
        linux.cgroups_path = if self.disable_cgroup {
            None
        } else {
            Self::oci_cgroups_path(config.cgroup_parent.as_deref(), container_id)
        };
        linux.resources = if self.disable_cgroup {
            None
        } else {
            Some(resources)
        };
        let merged_sysctls = self.merged_sysctls(&config.sysctls);
        linux.sysctl = if merged_sysctls.is_empty() {
            None
        } else {
            Some(merged_sysctls)
        };
        linux.rootfs_propagation = Self::merge_rootfs_propagation(
            linux.rootfs_propagation.as_deref(),
            Self::desired_rootfs_propagation(&config.mounts),
        )
        .map(ToString::to_string);
        security_patch.apply_linux(&mut linux);
        spec.linux = Some(linux);
        self.apply_oom_score_adj_policy(&mut spec, Self::requested_oom_score_adj(config))?;

        // 设置注解
        let mut annotations = spec.annotations.unwrap_or_default();
        annotations.insert(
            "org.opencontainers.image.ref.name".to_string(),
            config.image.clone(),
        );
        Self::insert_label_annotation(&mut annotations, &config.labels)?;
        for (k, v) in &config.annotations {
            annotations.insert(k.clone(), v.clone());
        }
        spec.annotations = Some(annotations);

        Ok(spec)
    }

    fn desired_rootfs_propagation(mounts: &[MountConfig]) -> Option<&'static str> {
        mounts.iter().fold(None, |current, mount| {
            Self::merge_rootfs_propagation(current, mount.propagation.required_rootfs_propagation())
        })
    }
   
   fn save_spec_for_bundle(&self, spec: &Spec, path: PathBuf) -> Result<()> {
        let mut value =
            serde_json::to_value(spec).context("Failed to encode OCI spec for bundle write")?;
        self.inject_mount_extension_fields(&mut value)?;
        let json =
            serde_json::to_string_pretty(&value).context("Failed to serialize OCI bundle JSON")?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write OCI bundle {}", path.display()))?;
        Ok(())
    }

    fn inject_mount_extension_fields(&self, spec_value: &mut Value) -> Result<()> {
        let Some(mounts) = spec_value.get_mut("mounts").and_then(Value::as_array_mut) else {
            return Ok(());
        };

        for mount in mounts {
            let Some(options) = mount.get_mut("options").and_then(Value::as_array_mut) else {
                continue;
            };

            let mut filtered_options = Vec::with_capacity(options.len());
            let mut uid_mappings = None;
            let mut gid_mappings = None;
            for option in options.iter() {
                let Some(raw) = option.as_str() else {
                    continue;
                };
                if let Some(encoded) = raw.strip_prefix(INTERNAL_UID_MAPPINGS_MOUNT_OPTION_PREFIX) {
                    uid_mappings = Some(
                        serde_json::from_str::<Vec<crate::oci::spec::IdMapping>>(encoded)
                            .context("Failed to decode internal mount uid mappings")?,
                    );
                    continue;
                }
                if let Some(encoded) = raw.strip_prefix(INTERNAL_GID_MAPPINGS_MOUNT_OPTION_PREFIX) {
                    gid_mappings = Some(
                        serde_json::from_str::<Vec<crate::oci::spec::IdMapping>>(encoded)
                            .context("Failed to decode internal mount gid mappings")?,
                    );
                    continue;
                }
                filtered_options.push(Value::String(raw.to_string()));
            }
            *options = filtered_options;

            if let Some(uid_mappings) = uid_mappings {
                mount["uidMappings"] = serde_json::to_value(uid_mappings)
                    .context("Failed to encode mount uid mappings")?;
            }
            if let Some(gid_mappings) = gid_mappings {
                mount["gidMappings"] = serde_json::to_value(gid_mappings)
                    .context("Failed to encode mount gid mappings")?;
            }
        }

        Ok(())
    }

    fn ledger_storage(&self) -> Result<Option<StorageManager>> {
        match self.state_db_path.as_ref() {
            Some(path) if !path.as_os_str().is_empty() => Ok(Some(StorageManager::new(path)?)),
            _ => Ok(None),
        }
    }

   /// 创建bundle目录结构
    fn create_bundle(&self, container_id: &str, rootfs: &Path, spec: &Spec) -> Result<()> {
        let bundle_path = self.bundle_path(container_id);

        // 创建bundle目录
        std::fs::create_dir_all(&bundle_path).context("Failed to create bundle directory")?;

        // 保存config.json
        self.save_spec_for_bundle(spec, self.config_path(container_id))?;

        // 当前运行时使用 OCI spec.root.path 作为 rootfs 来源，bundle 内不再强制准备 rootfs 目录。
        let _ = rootfs;

        if let Some(mut storage) = self.ledger_storage()? {
            storage.replace_runtime_artifacts(
                "container",
                container_id,
                &[
                    RuntimeArtifactRecord {
                        owner_kind: "container".to_string(),
                        owner_id: container_id.to_string(),
                        artifact_kind: "bundle".to_string(),
                        path: bundle_path.display().to_string(),
                        state: "active".to_string(),
                        runtime_handler: None,
                        runtime_root: Some(self.root.display().to_string()),
                    },
                    RuntimeArtifactRecord {
                        owner_kind: "container".to_string(),
                        owner_id: container_id.to_string(),
                        artifact_kind: "rootfs".to_string(),
                        path: rootfs.display().to_string(),
                        state: "active".to_string(),
                        runtime_handler: None,
                        runtime_root: Some(self.root.display().to_string()),
                    },
                ],
            )?;
        }

        info!(
            "Created bundle for container {} at {:?}",
            container_id, bundle_path
        );
        Ok(())
    }

    /// 分步创建：落盘 bundle（config.json + bundle 目录）。
    pub fn write_bundle(&self, container_id: &str, rootfs: &Path, spec: &Spec) -> Result<()> {
        self.create_bundle(container_id, rootfs, spec)
    }

    pub fn create_task_from_prepared_bundle(
        &self,
        container_id: &str,
        rootfs_mount: PreparedRootfsMount,
    ) -> Result<()> {
        if let Some(ref shim_manager) = self.shim_manager {
            let bundle_path = self.bundle_path(container_id);
            let rootfs_path = rootfs_mount.rootfs_path()?.to_path_buf();
            shim_manager.create_task(
                container_id,
                &bundle_path,
                &rootfs_path,
                Some(&rootfs_mount.key),
                rootfs_mount.mount_options(),
                rootfs_mount.handle,
            )?;
        }
        Ok(())
    }

    /// 分步创建：从 bundle 读取 OCI spec。
    pub fn load_spec(&self, container_id: &str) -> Result<Spec> {
        Spec::load(self.config_path(container_id))
            .map_err(|e| anyhow::anyhow!("Failed to load OCI spec for {}: {}", container_id, e))
    }

    pub(crate) fn validate_mount_requests(
        &self,
        config: &ContainerConfig,
    ) -> std::result::Result<(), MountSemanticsError> {
        self.validate_mount_semantics(&config.mounts, config.selinux_label.as_deref())
    }

    pub(crate) fn daemon_oom_score_adj() -> Result<i64> {
        let raw = std::fs::read_to_string("/proc/self/oom_score_adj")
            .context("could not get the daemon oom_score_adj")?;
        raw.trim()
            .parse::<i64>()
            .context("could not get the daemon oom_score_adj")
    }

    pub(crate) fn restrict_oom_score_adj_floor(preferred: i64) -> Result<i64> {
        let current = Self::daemon_oom_score_adj()?;
        Ok(preferred.max(current))
    }

    fn restore_rootfs_snapshot(&self, container_id: &str, image_path: &Path) -> Result<()> {
        let snapshot_path = image_path.join("rootfs.tar");
        if !snapshot_path.exists() {
            return Ok(());
        }

        let config = self.load_bundle_config_value(container_id)?;
        let rootfs_path = config
            .get("root")
            .and_then(|root| root.get("path"))
            .and_then(|path| path.as_str())
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "container {} bundle config is missing root.path for restore",
                    container_id
                )
            })?;

        if rootfs_path.exists() {
            std::fs::remove_dir_all(&rootfs_path).with_context(|| {
                format!(
                    "Failed to clear restore rootfs directory {}",
                    rootfs_path.display()
                )
            })?;
        }
        std::fs::create_dir_all(&rootfs_path).with_context(|| {
            format!(
                "Failed to recreate restore rootfs directory {}",
                rootfs_path.display()
            )
        })?;

        let output = Command::new("tar")
            .arg("-xf")
            .arg(&snapshot_path)
            .arg("-C")
            .arg(&rootfs_path)
            .output()
            .with_context(|| {
                format!(
                    "Failed to restore rootfs snapshot from {}",
                    snapshot_path.display()
                )
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                format!("status={}", output.status)
            } else {
                stderr
            };
            return Err(anyhow::anyhow!(
                "Failed to extract rootfs snapshot {}: {}",
                snapshot_path.display(),
                detail
            ));
        }

        Ok(())
    }

    /// 执行runc命令并检查状态（用于start/stop/run等动作类命令）
    /// 注意：不能对`runc run -d`使用output()，否则可能因后台子进程继承pipe导致阻塞。
    fn runc_exec(&self, args: &[&str]) -> Result<()> {
        debug!(
            "Executing (status): {} {}",
            self.runtime_path.display(),
            args.join(" ")
        );

        let status = self
            .runtime_command()
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Failed to execute runc command")?;

        if !status.success() {
            let detail = self
                .run_command_output(args)
                .ok()
                .and_then(|out| {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    if stderr.is_empty() {
                        None
                    } else {
                        Some(stderr)
                    }
                })
                .unwrap_or_else(|| format!("status={}", status));
            error!("runc command failed: {}", detail);
            return Err(anyhow::anyhow!("runc command failed: {}", detail));
        }

        Ok(())
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

impl MountPropagationMode {
    fn option(self) -> &'static str {
        match self {
            Self::Private => "rprivate",
            Self::HostToContainer => "rslave",
            Self::Bidirectional => "rshared",
        }
    }

    fn required_rootfs_propagation(self) -> Option<&'static str> {
        match self {
            Self::Private => None,
            Self::HostToContainer => Some("rslave"),
            Self::Bidirectional => Some("rshared"),
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RestoreCheckpointMetadata {
    checkpoint_location: String,
    checkpoint_image_path: String,
    oci_config: serde_json::Value,
    image_ref: String,
}

#[derive(Debug, Error)]
pub enum ImageAvailabilityError {
    #[error("image {image_ref} is not available locally; pull it before creating the container")]
    NotPresentLocally { image_ref: String },
    #[error("image {image_ref} has no unpackable layers in {image_dir}")]
    NoLayers {
        image_ref: String,
        image_dir: PathBuf,
    },
}

#[derive(Debug, Default)]
struct ImageRuntimeDefaults {
    env: Vec<(String, String)>,
    entrypoint: Vec<String>,
    cmd: Vec<String>,
    working_dir: Option<PathBuf>,
}
