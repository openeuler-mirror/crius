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


pub mod service;
pub mod container_handlers;
pub mod status;
pub mod streaming_handlers;
pub mod stats;
pub mod events;
pub mod annotations;
pub mod state_model;
pub mod responses;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{unimplemented, vec};
use std::sync::Arc;
use std::time::Instant;

use tonic::Status;

use crate::proto::runtime::v1::ContainerState;
use crate::config::{Config, CgroupDriverConfig};
use crate::server::service::{
    RuntimeServiceConfig, RuntimeServiceImpl,
    ContainerCreateDeadline,
};
use crate::runtime::backend::RuntimeBackend;
use crate::runtime::shim_manager::ShimConfig;
use crate::runtime::SeccompProfile;
use crate::server::state_model::{
    StoredSecurityProfile, CgroupResourceSupport,
    StoredLinuxResources,
};

impl RuntimeServiceConfig {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let runtime_name = config.runtime.runtime_type.clone();
        let runtime_config = config.runtime.resolved_runtimes().expect("runtime handler failed");
        Self {
            root_dir: PathBuf::from(&config.root),
            runtime: runtime_name,
            runtime_handlers: vec![],
            runtime_configs: runtime_config,
            runtime_root: PathBuf::from(&config.runtime.root),
            log_dir: PathBuf::from(&config.logging.dir),
            runtime_path: PathBuf::from(&config.runtime.runtime_path),
            runtime_config_path: PathBuf::from(&config.runtime.runtime_config_path),
            image_root: PathBuf::from(&config.image.root),
            image_driver: config.image.driver.clone(),
            image_global_auth_file: PathBuf::from(&config.image.global_auth_file),
            image_namespaced_auth_dir: PathBuf::from(&config.image.namespaced_auth_dir),
            image_default_transport: config.image.default_transport.clone(),
            image_short_name_mode: config.image.short_name_mode.clone(),
            image_pull_progress_timeout: config.image.pull_progress_timeout,
            image_max_concurrent_downloads: config.image.max_concurrent_downloads,
            image_pull_retry_count: config.image.pull_retry_count,
            image_registry_config_dir: PathBuf::from(&config.image.registry_config_dir),
            image_decryption_keys_path: PathBuf::from(&config.image.decryption_keys_path),
            image_decryption_decoder_path: config.image.decryption_decoder_path.clone(),
            image_decryption_keyprovider_config: PathBuf::from(
                &config.image.decryption_keyprovider_config,
            ),
            image_additional_artifact_stores: config
                .image
                .additional_artifact_stores
                .iter()
                .map(PathBuf::from)
                .collect(),
            image_signature_policy: PathBuf::from(&config.image.signature_policy),
            image_signature_policy_dir: PathBuf::from(&config.image.signature_policy_dir),
            image_storage_options: config.image.storage_options.clone(),
            image_volumes: config.image.image_volumes.clone(),
            image_pinned_images: {
                let mut pinned = config.image.pinned_images.clone();
                if !config.runtime.pause_image.trim().is_empty() {
                    pinned.push(config.runtime.pause_image.clone());
                }
                pinned.sort();
                pinned.dedup();
                pinned
            },
            image_big_files_temporary_dir: PathBuf::from(&config.image.big_files_temporary_dir),
            image_oci_artifact_mount_support: config.image.oci_artifact_mount_support,
            workloads: config.runtime.workloads.clone(),
            enable_pod_events: config.api.enable_pod_events,
            included_pod_metrics: config.api.included_pod_metrics.clone(),
            stats_collection_period: config.api.stats_collection_period,
            pod_sandbox_metrics_collection_period: config.api.pod_sandbox_metrics_collection_period,
            grpc_max_send_msg_size: config.api.grpc_max_send_msg_size,
            grpc_max_recv_msg_size: config.api.grpc_max_recv_msg_size,
            default_env: vec![],
            default_capabilities: config
                .runtime
                .default_capabilities
                .iter()
                .map(|capability| {
                    let upper = capability.trim().to_ascii_uppercase();
                    if upper.starts_with("CAP_") {
                        upper
                    } else {
                        format!("CAP_{upper}")
                    }
                })
                .collect(),
            default_sysctls: HashMap::new(),
            allowed_devices: vec![],
            device_ownership_from_security_context: config
                .runtime
                .device_ownership_from_security_context,
            add_inheritable_capabilities: config.runtime.add_inheritable_capabilities,
            default_mounts_file: PathBuf::from(&config.runtime.default_mounts_file),
            hooks_dir: config.runtime.hooks_dir.iter().map(PathBuf::from).collect(),
            absent_mount_sources_to_reject: config
                .runtime
                .absent_mount_sources_to_reject
                .iter()
                .map(PathBuf::from)
                .collect(),
            disable_proc_mount: config.runtime.disable_proc_mount,
            timezone: config.runtime.timezone.clone(),
            attach_socket_dir: PathBuf::from(&config.runtime.attach_socket_dir),
            container_exits_dir: PathBuf::from(&config.runtime.container_exits_dir),
            clean_shutdown_file: PathBuf::from(&config.runtime.clean_shutdown_file),
            container_stop_timeout: config.runtime.container_stop_timeout,
            version_file: PathBuf::from(&config.runtime.version_file),
            version_file_persist: PathBuf::from(&config.runtime.version_file_persist),
            internal_wipe: config.runtime.internal_wipe,
            internal_repair: config.runtime.internal_repair,
            bind_mount_prefix: PathBuf::from(&config.runtime.bind_mount_prefix),
            disable_cgroup: config.runtime.disable_cgroup,
            tolerate_missing_hugetlb_controller: false,
            separate_pull_cgroup: config.runtime.separate_pull_cgroup.clone(),
            seccomp_profile: PathBuf::new(),
            privileged_seccomp_profile: String::new(),
            unset_seccomp_profile: String::new(),
            apparmor_default_profile: String::new(),
            disable_apparmor: false,
            enable_selinux: false,
            selinux_category_range: 0,
            hostnetwork_disable_selinux: false,
            uid_mappings: None,
            gid_mappings: None,
            minimum_mappable_uid: config.runtime.minimum_mappable_uid,
            minimum_mappable_gid: config.runtime.minimum_mappable_gid,
            io_uid: config.runtime.io_uid,
            io_gid: config.runtime.io_gid,
            pids_limit: config.runtime.pids_limit,
            infra_ctr_cpuset: config.runtime.infra_ctr_cpuset.clone(),
            shared_cpuset: config.runtime.shared_cpuset.clone(),
            exec_cpu_affinity: config.runtime.exec_cpu_affinity.clone(),
            irqbalance_config_file: PathBuf::from(&config.runtime.irqbalance_config_file),
            irqbalance_config_restore_file: config.runtime.irqbalance_config_restore_file.clone(),
            read_only: config.runtime.read_only,
            no_pivot: config.runtime.no_pivot,
            no_new_keyring: config.runtime.no_new_keyring,
            pause_image: config.runtime.pause_image.clone(),
            pause_command: config.runtime.pause_command.clone(),
            drop_infra_ctr: config.runtime.drop_infra_ctr,
            cni_config: config.network.cni_config(),
            local_cni_config: config.network.local_cni_config(),
            cgroup_driver: config.runtime.cgroup_driver.map(|driver| driver.as_proto()),
            exec_sync_io_drain_timeout: config.api.exec_sync_io_drain_timeout,
            max_container_log_line_size: config.logging.max_container_log_line_size,
            log_to_journald: config.runtime.log_to_journald,
            no_sync_log: config.runtime.no_sync_log,
            restrict_oom_score_adj: config.runtime.restrict_oom_score_adj,
            enable_unprivileged_ports: config.runtime.enable_unprivileged_ports,
            enable_unprivileged_icmp: config.runtime.enable_unprivileged_icmp,
            shim: ShimConfig {
            shim_path: PathBuf::from(&config.runtime.shim_path),
            runtime_config_path: PathBuf::from(&config.runtime.runtime_config_path),
            monitor_cgroup: config.runtime.monitor_cgroup.clone(),
            work_dir: PathBuf::from(&config.runtime.shim_dir),
            attach_socket_dir: PathBuf::from(&config.runtime.attach_socket_dir),
            container_exits_dir: PathBuf::from(&config.runtime.container_exits_dir),
            io_uid: config.runtime.io_uid,
            io_gid: config.runtime.io_gid,
            monitor_env: config.runtime.monitor_env.clone(),
            debug: config.runtime.shim_debug,
            log_to_journald: config.runtime.log_to_journald,
            no_sync_log: config.runtime.no_sync_log,
            no_pivot: config.runtime.no_pivot,
            no_new_keyring: config.runtime.no_new_keyring,
            systemd_cgroup: matches!(
                config.runtime.cgroup_driver,
                Some(CgroupDriverConfig::Systemd)
            ),
            runtime_path: PathBuf::from(&config.runtime.runtime_path),
            max_container_log_line_size: config.logging.max_container_log_line_size,
            state_db_path: PathBuf::from(&config.root).join("crius.db"),
        },
        config_path: Some(config_path),
        }
    }
}


impl RuntimeServiceImpl {
    fn cgroup_support_flags() -> CgroupResourceSupport {
        Self::cgroup_support_flags_for_root(Path::new("/sys/fs/cgroup"))
    }

    fn cgroup_support_flags_for_root(root: &Path) -> CgroupResourceSupport {
        let is_v2 = root.join("cgroup.controllers").exists();

        let swap = if is_v2 {
            root.join("memory.swap.max").exists()
        } else {
            root.join("memory")
                .join("memory.memsw.limit_in_bytes")
                .exists()
        };

        let hugetlb = if is_v2 {
            std::fs::read_dir(root)
                .ok()
                .into_iter()
                .flat_map(|entries| entries.filter_map(Result::ok))
                .map(|entry| entry.file_name())
                .filter_map(|name| name.into_string().ok())
                .any(|name| name.starts_with("hugetlb.") && name.ends_with(".max"))
        } else {
            root.join("hugetlb").exists()
        };

        let memory_kernel = if is_v2 {
            false
        } else {
            root.join("memory")
                .join("memory.kmem.limit_in_bytes")
                .exists()
        };
        let memory_kernel_tcp = if is_v2 {
            false
        } else {
            root.join("memory")
                .join("memory.kmem.tcp.limit_in_bytes")
                .exists()
        };
        let memory_swappiness = if is_v2 {
            false
        } else {
            root.join("memory").join("memory.swappiness").exists()
        };
        let memory_disable_oom_killer = if is_v2 {
            false
        } else {
            root.join("memory").join("memory.oom_control").exists()
        };
        let memory_use_hierarchy = if is_v2 {
            false
        } else {
            root.join("memory").join("memory.use_hierarchy").exists()
        };
        let cpu_realtime = if is_v2 {
            false
        } else {
            root.join("cpu").join("cpu.rt_runtime_us").exists()
                && root.join("cpu").join("cpu.rt_period_us").exists()
        };

        CgroupResourceSupport {
            swap,
            hugetlb,
            memory_kernel,
            memory_kernel_tcp,
            memory_swappiness,
            memory_disable_oom_killer,
            memory_use_hierarchy,
            cpu_realtime,
            blockio: true,
            rdt: true,
        }
    }

    async fn runtime_for_container_request(
        &self,
        container_id: &str,
    ) -> Result<Arc<dyn RuntimeBackend>, Status> {
        let annotations = {
            let containers = self.containers.lock().await;
            containers
                .get(container_id)
                .map(|container| container.annotations.clone())
        };

        if let Some(annotations) = annotations {
            if let Ok(runtime) = self.runtime.runtime_for_annotations_map(&annotations) {
                return Ok(runtime);
            }
        }

        self.runtime
            .runtime_for_container(container_id)
            .map_err(|e| {
                Status::internal(format!(
                    "Failed to resolve runtime for container {}: {}",
                    container_id, e
                ))
            })
    }
    
    async fn runtime_namespace_path_for_container(
        &self,
        runtime_container_id: &str,
        namespace: &str,
    ) -> Result<Option<PathBuf>, Status> {
        if runtime_container_id.is_empty() {
            return Ok(None);
        }

        let runtime = self
            .runtime_for_container_request(runtime_container_id)
            .await?;
        let container_id = runtime_container_id.to_string();
        let pid = tokio::task::spawn_blocking(move || {
            runtime.task_controller().container_pid(&container_id)
        })
        .await
        .map_err(|e| Status::internal(format!("Failed to spawn blocking task: {}", e)))?
        .map_err(|e| {
            Status::internal(format!(
                "Failed to query container PID for {}: {}",
                runtime_container_id, e
            ))
        })?;

        Ok(pid.map(|pid| PathBuf::from(format!("/proc/{}/ns/{}", pid, namespace))))
    }

    async fn resolve_container_id(&self, requested_id: &str) -> Result<String, Status> {
        if let Ok(removed) = self.removed_container_ids.lock() {
            if removed.contains(requested_id) {
                return Err(Status::not_found("Container not found"));
            }
        }
        let containers = self.containers.lock().await;
        if containers.contains_key(requested_id) {
            return Ok(requested_id.to_string());
        }

        let matches: Vec<String> = containers
            .keys()
            .filter(|id| id.starts_with(requested_id))
            .cloned()
            .collect();

        match matches.len() {
            0 => Err(Status::not_found("Container not found")),
            1 => Ok(matches[0].clone()),
            _ => Err(Status::invalid_argument(format!(
                "ambiguous container id prefix: {}",
                requested_id
            ))),
        }
    }

    async fn runtime_namespace_path_for_target(
        &self,
        requested_container_id: &str,
        namespace: &str,
    ) -> Result<Option<PathBuf>, Status> {
        if requested_container_id.is_empty() {
            return Ok(None);
        }

        let resolved_id = self.resolve_container_id(requested_container_id).await?;
        self.runtime_namespace_path_for_container(&resolved_id, namespace)
            .await
    }

    fn security_availability() -> crate::security::SecurityManager {
        crate::security::SecurityManager::new()
    }

    fn effective_apparmor_profile_from_proto(
        &self,
        profile: Option<&crate::proto::runtime::v1::SecurityProfile>,
        deprecated_profile: &str,
        privileged: bool,
    ) -> Result<Option<String>, Status> {
        let security = Self::security_availability();
        unimplemented!()
    }

    #[allow(deprecated)]
    fn legacy_linux_container_apparmor_profile(
        security: Option<&crate::proto::runtime::v1::LinuxContainerSecurityContext>,
    ) -> &str {
        security
            .map(|security| security.apparmor_profile.as_str())
            .unwrap_or("")
    }

    fn effective_selinux_label_from_proto(
        &self,
        options: Option<&crate::proto::runtime::v1::SeLinuxOption>,
        host_network: bool,
        auto_level_seed: Option<&str>,
    ) -> Option<String> {
        let security = Self::security_availability();
        unimplemented!()
    }

    fn effective_seccomp_profile_from_proto(
        &self,
        profile: Option<&crate::proto::runtime::v1::SecurityProfile>,
        deprecated_profile: &str,
        privileged: bool,
    ) -> Option<SeccompProfile> {
        unimplemented!()
    }

    fn effective_stored_seccomp_profile_from_proto(
        &self,
        profile: Option<&crate::proto::runtime::v1::SecurityProfile>,
        deprecated_profile: &str,
        privileged: bool,
    ) -> Option<StoredSecurityProfile> {
        unimplemented!()
    }

    #[allow(deprecated)]
    fn legacy_linux_container_seccomp_profile_path(
        security: Option<&crate::proto::runtime::v1::LinuxContainerSecurityContext>,
    ) -> &str {
        security
            .map(|ctx| ctx.seccomp_profile_path.as_str())
            .unwrap_or("")
    }

    fn validate_hugetlb_limits_with_flags(
        hugepage_limits_present: bool,
        support: CgroupResourceSupport,
        tolerate_missing_hugetlb_controller: bool,
        operation: &str,
    ) -> Result<(), Status> {
        if hugepage_limits_present && !support.hugetlb && !tolerate_missing_hugetlb_controller {
            return Err(Status::failed_precondition(format!(
                "hugetlb controller is missing; {} includes hugepage limits. Set runtime.tolerate_missing_hugetlb_controller = true to ignore this error",
                operation
            )));
        }
        Ok(())
    }

    fn validate_stored_hugetlb_limits_with_flags(
        resources: Option<&StoredLinuxResources>,
        support: CgroupResourceSupport,
        tolerate_missing_hugetlb_controller: bool,
        operation: &str,
    ) -> Result<(), Status> {
        Self::validate_hugetlb_limits_with_flags(
            resources
                .map(|resources| !resources.hugepage_limits.is_empty())
                .unwrap_or(false),
            support,
            tolerate_missing_hugetlb_controller,
            operation,
        )
    }

    fn sanitize_stored_runtime_resources_with_policy(
        resources: &mut StoredLinuxResources,
        support: CgroupResourceSupport,
        tolerate_missing_hugetlb_controller: bool,
    ) {
        if !support.swap {
            resources.memory_swap_limit_in_bytes = 0;
        }
        if !support.memory_kernel {
            resources.memory_kernel_limit_in_bytes = None;
        }
        if !support.memory_kernel_tcp {
            resources.memory_kernel_tcp_limit_in_bytes = None;
        }
        if !support.memory_swappiness {
            resources.memory_swappiness = None;
        }
        if !support.memory_disable_oom_killer {
            resources.memory_disable_oom_killer = None;
        }
        if !support.memory_use_hierarchy {
            resources.memory_use_hierarchy = None;
        }
        if !support.cpu_realtime {
            resources.cpu_realtime_runtime = None;
            resources.cpu_realtime_period = None;
        }
        if !support.hugetlb && tolerate_missing_hugetlb_controller {
            resources.hugepage_limits.clear();
        }
        if !support.blockio {
            resources.blockio_class = None;
        }
        if !support.rdt {
            resources.rdt_class = None;
        }
    }

    pub(super) fn container_create_deadline_for_handler(
        &self,
        runtime_handler: &str,
    ) -> ContainerCreateDeadline {
        let timeout_secs = self
            .runtime
            .container_create_timeout_for_handler(runtime_handler);
        ContainerCreateDeadline {
            timeout_secs,
            deadline: Instant::now() + std::time::Duration::from_secs(timeout_secs as u64),
        }
    }

    fn sanitize_spec_runtime_resources_with_policy(
        spec: &mut crate::oci::spec::Spec,
        support: CgroupResourceSupport,
        tolerate_missing_hugetlb_controller: bool,
    ) {
        let Some(linux) = spec.linux.as_mut() else {
            return;
        };
        if !support.rdt {
            linux.intel_rdt = None;
        }
        let Some(resources) = linux.resources.as_mut() else {
            return;
        };

        if let Some(memory) = resources.memory.as_mut() {
            if !support.swap {
                memory.swap = None;
            }
            if !support.memory_kernel {
                memory.kernel = None;
            }
            if !support.memory_kernel_tcp {
                memory.kernel_tcp = None;
            }
            if !support.memory_swappiness {
                memory.swappiness = None;
            }
            if !support.memory_disable_oom_killer {
                memory.disable_oom_killer = None;
            }
            if !support.memory_use_hierarchy {
                memory.use_hierarchy = None;
            }
        }

        if let Some(cpu) = resources.cpu.as_mut() {
            if !support.cpu_realtime {
                cpu.realtime_runtime = None;
                cpu.realtime_period = None;
            }
        }

        if !support.hugetlb && tolerate_missing_hugetlb_controller {
            resources.hugepage_limits = None;
        }

        if !support.blockio {
            resources.block_io = None;
        }

        if !support.rdt {
            resources.intel_rdt = None;
        }
    }

    fn now_nanos() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64
    }

    fn container_reason_message(runtime_state: i32, exit_code: i32) -> (String, String) {
        match runtime_state {
            x if x == ContainerState::ContainerCreated as i32 => (
                "Created".to_string(),
                "container has been created but not started".to_string(),
            ),
            x if x == ContainerState::ContainerRunning as i32 => {
                ("Running".to_string(), "container is running".to_string())
            }
            x if x == ContainerState::ContainerExited as i32 => {
                let reason = if exit_code == 0 {
                    "Completed"
                } else if exit_code == -1 {
                    "Error"
                } else if exit_code == 137 {
                    "OOMKilled"
                } else {
                    "Error"
                };
                (
                    reason.to_string(),
                    if exit_code == -1 {
                        "container exited with unknown exit code".to_string()
                    } else {
                        format!("container exited with code {}", exit_code)
                    },
                )
            }
            _ => (
                "Unknown".to_string(),
                "runtime state could not be determined".to_string(),
            ),
        }
    }

    fn checkpoint_bundle_path(&self, container_id: &str) -> PathBuf {
        self.runtime
            .bundle_path_for_container(container_id)
            .unwrap_or_else(|_| self.config.runtime_root.join(container_id))
    }

    fn checkpoint_config_path(&self, container_id: &str) -> PathBuf {
        self.checkpoint_bundle_path(container_id)
            .join("config.json")
    }

    async fn runtime_container_pid_checked(&self, container_id: &str) -> Option<i32> {
        let runtime = self
            .runtime_for_container_request(container_id)
            .await
            .ok()?;
        let container_id = container_id.to_string();
        tokio::task::spawn_blocking(move || runtime.task_controller().container_pid(&container_id))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
    }

    fn normalize_timestamp_nanos(ts: i64) -> i64 {
        // Backward-compatible normalization: old records may still be seconds.
        if ts > 0 && ts < 1_000_000_000_000 {
            ts.saturating_mul(1_000_000_000)
        } else {
            ts
        }
    }
}