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


use std::sync::Arc;
use std::unimplemented;
use tokio::sync::Mutex;
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc as StdArc, Mutex as StdMutex};

use tonic::{Response, Status};
use anyhow::Context;

use crate::proto::runtime::v1::runtime_service_server::RuntimeService;
use crate::proto::runtime::v1::*;

use crate::image::{ImageServiceOptions, ImageServiceImpl};
use crate::service::InternalServices;
use crate::storage::persistence::{PersistenceManager, PersistenceConfig};
use crate::server::state_model::StoredNamespaceOptions;
use crate::runtime::backend::RuntimeBackend;
use crate::runtime::shim_manager::ShimConfig;
use crate::runtime::RuncRuntime;
use crate::runtime::runc_backend::RuncBackend;
use crate::network::CniConfig;
use crate::server::state_model::{
    StoredRuntimeNetworkConfig, StoredLinuxResources,
};
use crate::defaults::{
    CRIO_RUNTIME_HANDLER_ANNOTATION,
    CONTAINERD_RUNTIME_HANDLER_ANNOTATION,
};

/// 运行时配置
#[derive(Debug, Clone)]
pub struct RuntimeServiceConfig {
    pub root_dir: PathBuf,
    pub runtime: String,
    pub runtime_handlers: Vec<String>,
    pub runtime_configs: HashMap<String, crate::config::ResolvedRuntimeHandlerConfig>,
    pub runtime_root: PathBuf,
    pub log_dir: PathBuf,
    pub runtime_path: PathBuf,
    pub runtime_config_path: PathBuf,
    pub image_root: PathBuf,
    pub image_driver: String,
    pub image_global_auth_file: PathBuf,
    pub image_namespaced_auth_dir: PathBuf,
    pub image_default_transport: String,
    pub image_short_name_mode: String,
    pub image_pull_progress_timeout: std::time::Duration,
    pub image_max_concurrent_downloads: usize,
    pub image_pull_retry_count: u32,
    pub image_registry_config_dir: PathBuf,
    pub image_decryption_keys_path: PathBuf,
    pub image_decryption_decoder_path: String,
    pub image_decryption_keyprovider_config: PathBuf,
    pub image_additional_artifact_stores: Vec<PathBuf>,
    pub image_signature_policy: PathBuf,
    pub image_signature_policy_dir: PathBuf,
    pub image_storage_options: Vec<String>,
    // pub image_external_snapshotters: HashMap<String, crate::config::ExternalSnapshotterConfig>,
    pub image_volumes: String,
    pub image_pinned_images: Vec<String>,
    pub image_big_files_temporary_dir: PathBuf,
    pub image_oci_artifact_mount_support: bool,
    pub workloads: HashMap<String, crate::config::RuntimeWorkloadConfig>,
    pub enable_pod_events: bool,
    pub included_pod_metrics: Vec<String>,
    pub stats_collection_period: u64,
    pub pod_sandbox_metrics_collection_period: u64,
    pub grpc_max_send_msg_size: u32,
    pub grpc_max_recv_msg_size: u32,
    // pub metrics_enable: bool,
    // pub metrics_host: String,
    // pub metrics_port: u16,
    // pub metrics_socket_path: PathBuf,
    // pub metrics_enable_tls: bool,
    // pub metrics_tls_cert_file: PathBuf,
    // pub metrics_tls_key_file: PathBuf,
    // pub metrics_tls_ca_file: PathBuf,
    // pub metrics_tls_min_version: String,
    // pub metrics_tls_cipher_suites: Vec<String>,
    // pub metrics_collectors: Vec<String>,
    // pub tracing_enable: bool,
    // pub tracing_endpoint: String,
    // pub tracing_sampling_rate_per_million: u32,
    // pub monitor_env: Vec<String>,
    // pub monitor_cgroup: String,
    pub default_env: Vec<(String, String)>,
    pub default_capabilities: Vec<String>,
    pub default_sysctls: HashMap<String, String>,
    // pub default_ulimits: Vec<crate::oci::spec::Rlimit>,
    pub allowed_devices: Vec<PathBuf>,
    // pub additional_devices: Vec<crate::runtime::DeviceMapping>,
    pub device_ownership_from_security_context: bool,
    pub add_inheritable_capabilities: bool,
    // pub base_runtime_spec: Option<crate::oci::spec::Spec>,
    pub default_mounts_file: PathBuf,
    pub hooks_dir: Vec<PathBuf>,
    pub absent_mount_sources_to_reject: Vec<PathBuf>,
    pub disable_proc_mount: bool,
    pub timezone: String,
    pub attach_socket_dir: PathBuf,
    pub container_exits_dir: PathBuf,
    pub clean_shutdown_file: PathBuf,
    pub container_stop_timeout: u32,
    pub version_file: PathBuf,
    pub version_file_persist: PathBuf,
    // pub criu_path: PathBuf,
    // pub criu_image_path: PathBuf,
    // pub criu_work_path: PathBuf,
    // pub enable_criu_support: bool,
    pub internal_wipe: bool,
    pub internal_repair: bool,
    pub bind_mount_prefix: PathBuf,
    pub disable_cgroup: bool,
    pub tolerate_missing_hugetlb_controller: bool,
    pub separate_pull_cgroup: String,
    pub seccomp_profile: PathBuf,
    pub privileged_seccomp_profile: String,
    pub unset_seccomp_profile: String,
    pub apparmor_default_profile: String,
    pub disable_apparmor: bool,
    pub enable_selinux: bool,
    pub selinux_category_range: u32,
    pub hostnetwork_disable_selinux: bool,
    pub uid_mappings: Option<Vec<crate::proto::runtime::v1::IdMapping>>,
    pub gid_mappings: Option<Vec<crate::proto::runtime::v1::IdMapping>>,
    pub minimum_mappable_uid: i64,
    pub minimum_mappable_gid: i64,
    pub io_uid: u32,
    pub io_gid: u32,
    pub pids_limit: i64,
    pub infra_ctr_cpuset: String,
    pub shared_cpuset: String,
    pub exec_cpu_affinity: String,
    pub irqbalance_config_file: PathBuf,
    pub irqbalance_config_restore_file: String,
    pub read_only: bool,
    pub no_pivot: bool,
    pub no_new_keyring: bool,
    pub pause_image: String,
    pub pause_command: String,
    pub drop_infra_ctr: bool,
    pub cni_config: CniConfig,
    pub local_cni_config: CniConfig,
    pub cgroup_driver: Option<CgroupDriver>,
    pub exec_sync_io_drain_timeout: std::time::Duration,
    pub max_container_log_line_size: usize,
    pub log_to_journald: bool,
    pub no_sync_log: bool,
    pub restrict_oom_score_adj: bool,
    pub enable_unprivileged_ports: bool,
    pub enable_unprivileged_icmp: bool,
    // pub rootless: crate::rootless::EffectiveRootlessConfig,
    pub shim: ShimConfig,
    // pub streaming: crate::streaming::StreamingConfig,
    pub config_path: Option<PathBuf>,
}

/// 运行时配置
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub root_dir: PathBuf,
    pub runtime: String,
    pub runtime_handlers: Vec<String>,
    pub runtime_configs: HashMap<String, crate::config::ResolvedRuntimeHandlerConfig>,
    pub runtime_root: PathBuf,
    pub log_dir: PathBuf,
    pub runtime_path: PathBuf,
    pub runtime_config_path: PathBuf,
    pub image_root: PathBuf,
    pub image_driver: String,
    pub image_global_auth_file: PathBuf,
    pub image_namespaced_auth_dir: PathBuf,
    pub image_default_transport: String,
    pub image_short_name_mode: String,
    pub image_pull_progress_timeout: std::time::Duration,
    pub image_max_concurrent_downloads: usize,
    pub image_pull_retry_count: u32,
    pub image_registry_config_dir: PathBuf,
    pub image_decryption_keys_path: PathBuf,
    pub image_decryption_decoder_path: String,
    pub image_decryption_keyprovider_config: PathBuf,
    pub image_additional_artifact_stores: Vec<PathBuf>,
    pub image_signature_policy: PathBuf,
    pub image_signature_policy_dir: PathBuf,
    pub image_storage_options: Vec<String>,
    // pub image_external_snapshotters: HashMap<String, crate::config::ExternalSnapshotterConfig>,
    pub image_volumes: String,
    pub image_pinned_images: Vec<String>,
    pub image_big_files_temporary_dir: PathBuf,
    pub image_oci_artifact_mount_support: bool,
    pub workloads: HashMap<String, crate::config::RuntimeWorkloadConfig>,
    pub enable_pod_events: bool,
    pub included_pod_metrics: Vec<String>,
    pub stats_collection_period: u64,
    pub pod_sandbox_metrics_collection_period: u64,
    pub grpc_max_send_msg_size: u32,
    pub grpc_max_recv_msg_size: u32,
    pub metrics_enable: bool,
    pub metrics_host: String,
    pub metrics_port: u16,
    pub metrics_socket_path: PathBuf,
    pub metrics_enable_tls: bool,
    pub metrics_tls_cert_file: PathBuf,
    pub metrics_tls_key_file: PathBuf,
    pub metrics_tls_ca_file: PathBuf,
    pub metrics_tls_min_version: String,
    pub metrics_tls_cipher_suites: Vec<String>,
    pub metrics_collectors: Vec<String>,
    pub tracing_enable: bool,
    pub tracing_endpoint: String,
    pub tracing_sampling_rate_per_million: u32,
    pub monitor_env: Vec<String>,
    pub monitor_cgroup: String,
    pub default_env: Vec<(String, String)>,
    pub default_capabilities: Vec<String>,
    pub default_sysctls: HashMap<String, String>,
    pub default_ulimits: Vec<crate::oci::spec::Rlimit>,
    pub allowed_devices: Vec<PathBuf>,
    // pub additional_devices: Vec<crate::runtime::DeviceMapping>,
    pub device_ownership_from_security_context: bool,
    pub add_inheritable_capabilities: bool,
    pub base_runtime_spec: Option<crate::oci::spec::Spec>,
    pub default_mounts_file: PathBuf,
    pub hooks_dir: Vec<PathBuf>,
    pub absent_mount_sources_to_reject: Vec<PathBuf>,
    pub disable_proc_mount: bool,
    pub timezone: String,
    pub attach_socket_dir: PathBuf,
    pub container_exits_dir: PathBuf,
    pub clean_shutdown_file: PathBuf,
    pub container_stop_timeout: u32,
    pub version_file: PathBuf,
    pub version_file_persist: PathBuf,
    pub criu_path: PathBuf,
    pub criu_image_path: PathBuf,
    pub criu_work_path: PathBuf,
    pub enable_criu_support: bool,
    pub internal_wipe: bool,
    pub internal_repair: bool,
    pub bind_mount_prefix: PathBuf,
    pub disable_cgroup: bool,
    pub tolerate_missing_hugetlb_controller: bool,
    pub separate_pull_cgroup: String,
    pub seccomp_profile: PathBuf,
    pub privileged_seccomp_profile: String,
    pub unset_seccomp_profile: String,
    pub apparmor_default_profile: String,
    pub disable_apparmor: bool,
    pub enable_selinux: bool,
    pub selinux_category_range: u32,
    pub hostnetwork_disable_selinux: bool,
    pub uid_mappings: Option<Vec<crate::proto::runtime::v1::IdMapping>>,
    pub gid_mappings: Option<Vec<crate::proto::runtime::v1::IdMapping>>,
    pub minimum_mappable_uid: i64,
    pub minimum_mappable_gid: i64,
    pub io_uid: u32,
    pub io_gid: u32,
    pub pids_limit: i64,
    pub infra_ctr_cpuset: String,
    pub shared_cpuset: String,
    pub exec_cpu_affinity: String,
    pub irqbalance_config_file: PathBuf,
    pub irqbalance_config_restore_file: String,
    pub read_only: bool,
    pub no_pivot: bool,
    pub no_new_keyring: bool,
    pub pause_image: String,
    pub pause_command: String,
    pub drop_infra_ctr: bool,
    pub cni_config: CniConfig,
    pub local_cni_config: CniConfig,
    pub cgroup_driver: Option<CgroupDriver>,
    pub exec_sync_io_drain_timeout: std::time::Duration,
    pub max_container_log_line_size: usize,
    pub log_to_journald: bool,
    pub no_sync_log: bool,
    pub restrict_oom_score_adj: bool,
    pub enable_unprivileged_ports: bool,
    pub enable_unprivileged_icmp: bool,
    pub shim: ShimConfig,
    // pub streaming: crate::streaming::StreamingConfig,
    pub config_path: Option<PathBuf>,
}



#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeReloadableConfig {
    pub pause_image: String,
    pub pinned_images: Vec<String>,
    pub registry_config_dir: PathBuf,
    pub global_auth_file: PathBuf,
    pub namespaced_auth_dir: PathBuf,
    pub signature_policy: PathBuf,
    pub signature_policy_dir: PathBuf,
    pub decryption_keys_path: PathBuf,
    pub decryption_decoder_path: String,
    pub decryption_keyprovider_config: PathBuf,
    pub seccomp_profile: PathBuf,
    pub apparmor_default_profile: String,
    pub cni_config_dirs: Vec<PathBuf>,
    pub cni_conf_template: Option<PathBuf>,
    pub cni_max_conf_num: usize,
    pub cni_default_network_name: Option<String>,
}

impl RuntimeReloadableConfig {
    pub fn with_cni_config(&self, base: &crate::network::CniConfig) -> crate::network::CniConfig {
        let mut config = base.clone();
        config.set_config_dirs(self.cni_config_dirs.clone());
        config.set_plugin_dirs(base.plugin_dirs().to_vec());
        config.set_max_conf_num(self.cni_max_conf_num);
        config.set_default_network_name(self.cni_default_network_name.clone());
        config.set_conf_template(self.cni_conf_template.clone());
        config
    }

    pub fn from_runtime_config(config: &RuntimeServiceConfig) -> Self {
        Self {
            pause_image: config.pause_image.clone(),
            pinned_images: config.image_pinned_images.clone(),
            registry_config_dir: config.image_registry_config_dir.clone(),
            global_auth_file: config.image_global_auth_file.clone(),
            namespaced_auth_dir: config.image_namespaced_auth_dir.clone(),
            signature_policy: config.image_signature_policy.clone(),
            signature_policy_dir: config.image_signature_policy_dir.clone(),
            decryption_keys_path: config.image_decryption_keys_path.clone(),
            decryption_decoder_path: config.image_decryption_decoder_path.clone(),
            decryption_keyprovider_config: config.image_decryption_keyprovider_config.clone(),
            seccomp_profile: config.seccomp_profile.clone(),
            apparmor_default_profile: config.apparmor_default_profile.clone(),
            cni_config_dirs: config.cni_config.config_dirs().to_vec(),
            cni_conf_template: config.cni_config.conf_template().map(Path::to_path_buf),
            cni_max_conf_num: config.cni_config.max_conf_num(),
            cni_default_network_name: config
                .cni_config
                .default_network_name()
                .map(ToOwned::to_owned),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct NameRegistry {
    ids_by_name: HashMap<String, String>,
    names_by_id: HashMap<String, String>,
}

impl NameRegistry {
    pub(super) fn reserve(&mut self, name: &str, id: &str) -> Result<(), String> {
        match self.ids_by_name.get(name) {
            Some(existing_id) if existing_id == id => {
                self.names_by_id.insert(id.to_string(), name.to_string());
                Ok(())
            }
            Some(existing_id) => Err(existing_id.clone()),
            None => {
                if let Some(previous_name) =
                    self.names_by_id.insert(id.to_string(), name.to_string())
                {
                    self.ids_by_name.remove(&previous_name);
                }
                self.ids_by_name.insert(name.to_string(), id.to_string());
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RuntimeReloadState {
    pub last_reload_at_unix_millis: Option<i64>,
    pub last_reload_source: Option<String>,
    pub last_reload_fields: Vec<String>,
    pub last_reload_error: Option<String>,
    pub watcher_active: bool,
    pub watcher_status: RuntimeReloadWatcherStatus,
    pub watcher_backoff_count: u32,
    pub watcher_next_retry_unix_millis: Option<i64>,
    pub watcher_last_error: Option<String>,
    pub config_file_watch: bool,
    pub cni_watch_dirs: Vec<String>,
    pub last_cni_watch_at_unix_millis: Option<i64>,
    pub last_cni_watch_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeReloadWatcherStatus {
    #[default]
    Stopped,
    Running,
    Backoff,
    Error,
}

#[derive(Clone)]
pub struct RuntimeServiceImpl {
    pub(super) containers: Arc<Mutex<HashMap<String, Container>>>,
    pub(super) pod_sandboxes: Arc<Mutex<HashMap<String, crate::proto::runtime::v1::PodSandbox>>>,
    pub(super) container_names: StdArc<StdMutex<NameRegistry>>,
    pub(super) pod_names: StdArc<StdMutex<NameRegistry>>,
    pub(super) removed_container_ids: StdArc<StdMutex<HashSet<String>>>,
    pub(super) removed_pod_sandbox_ids: StdArc<StdMutex<HashSet<String>>>,
    pub(super) config: RuntimeServiceConfig,
    pub(super) runtime: RuntimeRegistry,
    pub(super) image_service: ImageServiceImpl,
    pub(super) internal_services: crate::service::InternalServices,
    pub(super) shim_work_dir: PathBuf,
    pub(super) attach_socket_dir: PathBuf,
    pub(super) container_exits_dir: PathBuf,
    pub(super) clean_shutdown_file: PathBuf,
    pub(super) last_startup_clean_shutdown: StdArc<StdMutex<Option<bool>>>,
    pub(super) runtime_network_config: Arc<Mutex<Option<crate::proto::runtime::v1::NetworkConfig>>>,
    pub(super) reloadable_config: StdArc<StdMutex<RuntimeReloadableConfig>>,
    pub(super) reload_state: StdArc<StdMutex<RuntimeReloadState>>,
}

impl RuntimeServiceImpl {
    pub fn new(config: RuntimeServiceConfig, ) -> Self {
        let containers = Arc::new(Mutex::new(HashMap::new()));
        let pod_sandboxes = Arc::new(Mutex::new(HashMap::new()));
        let container_names = StdArc::new(StdMutex::new(NameRegistry::default()));
        let pod_names = StdArc::new(StdMutex::new(NameRegistry::default()));
        let container_create_timeouts = config.runtime_configs
            .iter()
            .map(|(handler, config)| (handler.clone(), config.container_create_timeout))
            .collect();
        let shim_work_dir = config.runtime_root.join("shims");
        let resolved_shim_work_dir = shim_work_dir;

        let runtimes: HashMap<String, Arc<dyn RuntimeBackend>> = if let Some(
            runtimes,
        ) =
            None
        {
            runtimes
        } else {
            config
                    .runtime_configs
                    .iter()
                    .map(|(handler, runtime_config)| {
                        let mut shim_config = config.shim.clone();
                        shim_config.work_dir = resolved_shim_work_dir.clone();
                        shim_config.attach_socket_dir = config.attach_socket_dir.clone();
                        shim_config.container_exits_dir = config.container_exits_dir.clone();
                        shim_config.shim_path = PathBuf::from(&runtime_config.monitor_path);
                        shim_config.runtime_config_path =
                            PathBuf::from(runtime_config.runtime_config_path.as_str());
                        shim_config.monitor_cgroup = runtime_config.monitor_cgroup.clone();
                        shim_config.io_uid = config.io_uid;
                        shim_config.io_gid = config.io_gid;
                        shim_config.runtime_path = PathBuf::from(&runtime_config.runtime_path);
                        shim_config.monitor_env = runtime_config.monitor_env.clone();
                        shim_config.no_sync_log = config.no_sync_log;
                        shim_config.no_new_keyring = config.no_new_keyring;
                        shim_config.systemd_cgroup =
                            config.cgroup_driver == Some(CgroupDriver::Systemd);
                        let backend: Arc<dyn RuntimeBackend> = match runtime_config
                            .backend
                            .as_str()
                        {
                            "wasm-direct" => {
                                unimplemented!()
                            }
                            "" | "runc" => {
                                let mut runtime = RuncRuntime::with_shim_and_image_storage(
                                    PathBuf::from(&runtime_config.runtime_path),
                                    PathBuf::from(&runtime_config.runtime_root),
                                    config.image_root.clone(),
                                    shim_config,
                                );
                                Arc::new(RuncBackend::new(runtime))
                            }
                            other => {
                                log::warn!(
                                    "runtime handler {} requested backend {}; falling back to runc-compatible backend construction",
                                    handler,
                                    other
                                );
                                unimplemented!()
                            }
                        };
                        (handler.clone(), backend)
                    })
                    .collect()
        };
        let runtime = RuntimeRegistry::new(config.runtime.clone(), runtimes, container_create_timeouts);
        let image_service = ImageServiceImpl::new_with_options(ImageServiceOptions {
            storage_path: config.image_root.clone(),
            ledger_db_path: Some(config.root_dir.join("crius.db")),
            storage_driver: config.image_driver.clone(),
            storage_options: config.image_storage_options.clone(),
            global_auth_file: (!config.image_global_auth_file.as_os_str().is_empty())
                .then(|| config.image_global_auth_file.clone()),
            namespaced_auth_dir: (!config.image_namespaced_auth_dir.as_os_str().is_empty())
                .then(|| config.image_namespaced_auth_dir.clone()),
            default_transport: config.image_default_transport.clone(),
            short_name_mode: config.image_short_name_mode.clone(),
            pull_progress_timeout: config.image_pull_progress_timeout,
            max_concurrent_downloads: config.image_max_concurrent_downloads,
            pull_retry_count: config.image_pull_retry_count,
            registry_config_dir: (!config.image_registry_config_dir.as_os_str().is_empty())
                .then(|| config.image_registry_config_dir.clone()),
            decryption_keys_path: (!config.image_decryption_keys_path.as_os_str().is_empty())
                .then(|| config.image_decryption_keys_path.clone()),
            decryption_decoder_path: config.image_decryption_decoder_path.clone(),
            decryption_keyprovider_config: (!config
                .image_decryption_keyprovider_config
                .as_os_str()
                .is_empty())
            .then(|| config.image_decryption_keyprovider_config.clone()),
            additional_artifact_stores: config.image_additional_artifact_stores.clone(),
            pinned_image_patterns: config.image_pinned_images.clone(),
            signature_policy: (!config.image_signature_policy.as_os_str().is_empty())
                .then(|| config.image_signature_policy.clone()),
            signature_policy_dir: (!config.image_signature_policy_dir.as_os_str().is_empty())
                .then(|| config.image_signature_policy_dir.clone()),
            big_files_temporary_dir: (!config.image_big_files_temporary_dir.as_os_str().is_empty())
                .then(|| config.image_big_files_temporary_dir.clone()),
            separate_pull_cgroup: config.separate_pull_cgroup.clone(),
            cgroup_driver: match config.cgroup_driver {
                Some(CgroupDriver::Systemd) => crate::config::CgroupDriverConfig::Systemd,
                _ => crate::config::CgroupDriverConfig::Cgroupfs,
            },
            disable_cgroup: config.disable_cgroup,
        })
        .expect("Failed to initialize image service");
        let persistence_config = PersistenceConfig {
            db_path: config.root_dir.join("crius.db"),
            enable_recovery: true,
            auto_save_interval: 30,
        };
        let persistence = PersistenceManager::new(persistence_config)
            .expect("Failed to create persistence manager");
        let persistence = Arc::new(Mutex::new(persistence));
        let (events, _) = tokio::sync::broadcast::channel(256);
        let internal_services = InternalServices::new(
            crate::service::event::EventService::from_sender(events.clone())
               .with_ledger(persistence.clone()),
        );
        let runtime_network_config = Self::load_runtime_network_config(&config.root_dir)
            .unwrap_or_else(|e| {
                log::warn!(
                    "Failed to load runtime network config from {}: {}",
                    config.root_dir.display(),
                    e
                );
                None
            });
        let reloadable_config = StdArc::new(StdMutex::new(
            RuntimeReloadableConfig::from_runtime_config(&config),
        ));
        let reload_state = StdArc::new(StdMutex::new(RuntimeReloadState {
            config_file_watch: config.config_path.is_some(),
            cni_watch_dirs: config
                .cni_config
                .config_dirs()
                .iter()
                .map(|dir| dir.display().to_string())
                .collect(),
            ..Default::default()
        }));
        let service = Self { 
            containers, 
            pod_sandboxes, 
            container_names, 
            pod_names, 
            removed_container_ids: Arc::new(StdMutex::new(HashSet::new())), 
            removed_pod_sandbox_ids: Arc::new(StdMutex::new(HashSet::new())), 
            config, 
            runtime,
            image_service: image_service, 
            internal_services,
            shim_work_dir: PathBuf::new(), 
            attach_socket_dir: PathBuf::new(), 
            container_exits_dir: PathBuf::new(), 
            clean_shutdown_file: PathBuf::new(), 
            last_startup_clean_shutdown: Arc::new(StdMutex::new(None)), 
            runtime_network_config: Arc::new(Mutex::new(runtime_network_config)),
            reloadable_config,
            reload_state,
        };
        service
    }

    pub fn image_service(&self) -> ImageServiceImpl {
        self.image_service.clone()
    }

    pub(super) fn validate_container_image_spec(
        config: &crate::proto::runtime::v1::ContainerConfig,
    ) -> Result<&ImageSpec, Status> {
        let image = config.image.as_ref().ok_or_else(|| {
            Status::invalid_argument("CreateContainerRequest.ContainerConfig.Image is nil")
        })?;
        if image.image.trim().is_empty() {
            return Err(Status::invalid_argument(
                "CreateContainerRequest.ContainerConfig.Image.Image is empty",
            ));
        }
        Ok(image)
    }

    pub(super) fn container_name_key(
        metadata: &ContainerMetadata,
        pod_metadata: &PodSandboxMetadata,
    ) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            metadata.name,
            pod_metadata.name,
            pod_metadata.namespace,
            pod_metadata.uid,
            metadata.attempt
        )
    }

    pub(super) fn effective_userns_options(
        &self,
        requested: Option<&NamespaceOption>,
    ) -> Option<NamespaceOption> {
        let (Some(uid_mappings), Some(gid_mappings)) = (
            self.config.uid_mappings.as_ref(),
            self.config.gid_mappings.as_ref(),
        ) else {
            return requested.cloned();
        };

        if let Some(options) = requested {
            if let Some(userns) = options.userns_options.as_ref() {
                if userns.mode == NamespaceMode::Node as i32 {
                    return Some(options.clone());
                }
                if !userns.uids.is_empty() || !userns.gids.is_empty() {
                    return Some(options.clone());
                }
            }

            let mut effective = options.clone();
            effective.userns_options = Some(crate::proto::runtime::v1::UserNamespace {
                mode: NamespaceMode::Pod as i32,
                uids: uid_mappings.clone(),
                gids: gid_mappings.clone(),
            });
            return Some(effective);
        }

        Some(NamespaceOption {
            network: NamespaceMode::Pod as i32,
            pid: NamespaceMode::Pod as i32,
            ipc: NamespaceMode::Pod as i32,
            target_id: String::new(),
            userns_options: Some(crate::proto::runtime::v1::UserNamespace {
                mode: NamespaceMode::Pod as i32,
                uids: uid_mappings.clone(),
                gids: gid_mappings.clone(),
            }),
        })
    }

    pub(super) fn effective_container_namespace_options(
        &self,
        requested: Option<&NamespaceOption>,
        sandbox: Option<&StoredNamespaceOptions>,
    ) -> Option<NamespaceOption> {
        let mut effective = self.effective_userns_options(requested);
        let Some(sandbox) = sandbox else {
            return effective;
        };

        let sandbox = sandbox.to_proto();
        let requested_missing = requested.is_none();
        let effective = effective.get_or_insert_with(|| sandbox.clone());

        // Match CRI-O's behavior: the sandbox decides whether workload
        // containers must run in host namespaces, even if the container
        // request omitted namespace options or left them at proto defaults.
        if sandbox.network == NamespaceMode::Node as i32 || requested_missing {
            effective.network = sandbox.network;
        }

        if sandbox.pid == NamespaceMode::Node as i32 {
            effective.pid = sandbox.pid;
            effective.target_id.clear();
        } else if requested_missing {
            effective.pid = sandbox.pid;
            effective.target_id = sandbox.target_id.clone();
        }

        if sandbox.ipc == NamespaceMode::Node as i32 || requested_missing {
            effective.ipc = sandbox.ipc;
        }

        if effective.userns_options.is_none() {
            effective.userns_options = sandbox.userns_options;
        }

        Some(effective.clone())
    }

    fn run_as_user_is_non_root(run_as_user: Option<&str>) -> bool {
        let Some(run_as_user) = run_as_user.map(str::trim).filter(|value| !value.is_empty()) else {
            return false;
        };

        run_as_user
            .parse::<u64>()
            .map(|value| value != 0)
            .unwrap_or(true)
    }

    fn run_as_group_or_supplemental_is_non_root(
        run_as_group: Option<u32>,
        supplemental_groups: &[u32],
    ) -> bool {
        run_as_group.is_some_and(|group| group != 0)
            || supplemental_groups.iter().any(|group| *group != 0)
    }

    pub(super) fn validate_minimum_mappable_ids(
        &self,
        namespace_options: Option<&NamespaceOption>,
        run_as_user: Option<&str>,
        run_as_group: Option<u32>,
        supplemental_groups: &[u32],
    ) -> Result<(), Status> {
        let Some(userns) = namespace_options.and_then(|options| options.userns_options.as_ref())
        else {
            return Ok(());
        };
        if userns.mode == NamespaceMode::Node as i32 {
            return Ok(());
        }

        let non_root_user = Self::run_as_user_is_non_root(run_as_user);
        let non_root_group =
            Self::run_as_group_or_supplemental_is_non_root(run_as_group, supplemental_groups);

        if self.config.minimum_mappable_uid >= 0 && non_root_user {
            for mapping in &userns.uids {
                if i64::from(mapping.host_id) < self.config.minimum_mappable_uid {
                    return Err(Status::invalid_argument(format!(
                        "uid mapping {}:{}:{} is below minimum mappable uid {} for non-root user namespace",
                        mapping.container_id,
                        mapping.host_id,
                        mapping.length,
                        self.config.minimum_mappable_uid
                    )));
                }
            }
        }

        if self.config.minimum_mappable_gid >= 0 && (non_root_user || non_root_group) {
            for mapping in &userns.gids {
                if i64::from(mapping.host_id) < self.config.minimum_mappable_gid {
                    return Err(Status::invalid_argument(format!(
                        "gid mapping {}:{}:{} is below minimum mappable gid {} for non-root user namespace",
                        mapping.container_id,
                        mapping.host_id,
                        mapping.length,
                        self.config.minimum_mappable_gid
                    )));
                }
            }
        }

        Ok(())
    }

    pub(super) fn pod_network_domain_cni_config(&self, local: bool) -> crate::network::CniConfig {
        let mut config = if local {
            self.config.local_cni_config.clone()
        } else {
            self.current_cni_config()
        };
        config.set_event_sink(Some(crate::service::event::LedgerInternalEventSink::new(
            self.config.root_dir.join("crius.db"),
        )));
        config
    }

    pub fn current_reloadable_config(&self) -> RuntimeReloadableConfig {
        self.reloadable_config
            .lock()
            .expect("reloadable config lock poisoned")
            .clone()
    }

    pub(super) fn current_cni_config(&self) -> crate::network::CniConfig {
        let mut config = self
            .current_reloadable_config()
            .with_cni_config(&self.config.cni_config);
        config.set_event_sink(Some(crate::service::event::LedgerInternalEventSink::new(
            self.config.root_dir.join("crius.db"),
        )));
        config
    }

    fn runtime_network_config_path(root_dir: &Path) -> PathBuf {
        root_dir.join("runtime_network_config.json")
    }

    fn load_runtime_network_config(
        root_dir: &Path,
    ) -> anyhow::Result<Option<crate::proto::runtime::v1::NetworkConfig>> {
        let path = Self::runtime_network_config_path(root_dir);
        if !path.exists() {
            return Ok(None);
        }

        let raw = std::fs::read(&path)
            .with_context(|| format!("Failed to read runtime network config {}", path.display()))?;
        let stored: StoredRuntimeNetworkConfig =
            serde_json::from_slice(&raw).with_context(|| {
                format!("Failed to parse runtime network config {}", path.display())
            })?;
        if stored.pod_cidr.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(crate::proto::runtime::v1::NetworkConfig {
                pod_cidr: stored.pod_cidr,
            }))
        }
    }

    pub(super) fn effective_readonly_rootfs(&self, requested: bool) -> bool {
        self.config.read_only || requested
    }

    pub(super) fn effective_pids_limit(
        &self,
        requested: Option<i64>,
    ) -> Result<Option<i64>, Status> {
        match requested {
            Some(limit) if limit > 0 => Ok(Some(limit)),
            Some(0) | None => Ok((self.config.pids_limit > 0).then_some(self.config.pids_limit)),
            Some(-1) => Ok(None),
            Some(limit) => Err(Status::invalid_argument(format!(
                "pids_limit must be -1, 0, or greater than zero, got {}",
                limit
            ))),
        }
    }

    pub(super) fn clamp_stored_oom_score_adj(
        &self,
        resources: &mut StoredLinuxResources,
    ) -> Result<(), Status> {
        if !self.config.restrict_oom_score_adj || resources.oom_score_adj == 0 {
            return Ok(());
        }
        resources.oom_score_adj =
            crate::runtime::RuncRuntime::restrict_oom_score_adj_floor(resources.oom_score_adj)
                .map_err(|e| {
                    Status::internal(format!("Failed to enforce oom_score_adj policy: {}", e))
                })?;
        Ok(())
    }
}


/// 为 `RuntimeServiceImpl` 实现 CRI `RuntimeService` trait。
///
/// 当前所有方法均为桩实现，返回 `tonic::Status::unimplemented`，
/// 后续逐个替换为真实业务逻辑。
#[tonic::async_trait]
impl RuntimeService for RuntimeServiceImpl {
    // ---- PodSandbox 生命周期 ----

    // TODO: 返回运行时名称、版本及 API 版本
    async fn version(
        &self,
        _request: tonic::Request<VersionRequest>,
    ) -> std::result::Result<tonic::Response<VersionResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("version: not implemented"))
    }

    // TODO: 创建并启动 Pod 沙箱
    async fn run_pod_sandbox(
        &self,
        _request: tonic::Request<RunPodSandboxRequest>,
    ) -> std::result::Result<tonic::Response<RunPodSandboxResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("run_pod_sandbox: not implemented"))
    }

    // TODO: 停止 Pod 沙箱并回收网络资源
    async fn stop_pod_sandbox(
        &self,
        _request: tonic::Request<StopPodSandboxRequest>,
    ) -> std::result::Result<tonic::Response<StopPodSandboxResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("stop_pod_sandbox: not implemented"))
    }

    // TODO: 移除 Pod 沙箱
    async fn remove_pod_sandbox(
        &self,
        _request: tonic::Request<RemovePodSandboxRequest>,
    ) -> std::result::Result<tonic::Response<RemovePodSandboxResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("remove_pod_sandbox: not implemented"))
    }

    // TODO: 返回 Pod 沙箱状态
    async fn pod_sandbox_status(
        &self,
        _request: tonic::Request<PodSandboxStatusRequest>,
    ) -> std::result::Result<tonic::Response<PodSandboxStatusResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("pod_sandbox_status: not implemented"))
    }

    // TODO: 列出所有 Pod 沙箱
    async fn list_pod_sandbox(
        &self,
        _request: tonic::Request<ListPodSandboxRequest>,
    ) -> std::result::Result<tonic::Response<ListPodSandboxResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("list_pod_sandbox: not implemented"))
    }

    // ---- Container 生命周期 ----

    // TODO: 在指定 Pod 沙箱中创建容器
    async fn create_container(
        &self,
        request: tonic::Request<CreateContainerRequest>,
    ) -> std::result::Result<tonic::Response<CreateContainerResponse>, tonic::Status> {
        RuntimeServiceImpl::create_container_impl(self, request).await
    }

    // TODO: 启动容器
    async fn start_container(
        &self,
        request: tonic::Request<StartContainerRequest>,
    ) -> std::result::Result<tonic::Response<StartContainerResponse>, tonic::Status> {
        RuntimeServiceImpl::start_container_impl(self, request).await
    }

    // TODO: 停止容器（带 grace period）
    async fn stop_container(
        &self,
        request: tonic::Request<StopContainerRequest>,
    ) -> std::result::Result<tonic::Response<StopContainerResponse>, tonic::Status> {
        RuntimeServiceImpl::stop_container(self, request).await
    }

    // TODO: 移除容器
    async fn remove_container(
        &self,
        request: tonic::Request<RemoveContainerRequest>,
    ) -> std::result::Result<tonic::Response<RemoveContainerResponse>, tonic::Status> {
        RuntimeServiceImpl::remove_container(self, request).await
    }

    // TODO: 按过滤器列出容器
    async fn list_containers(
        &self,
        request: tonic::Request<ListContainersRequest>,
    ) -> std::result::Result<tonic::Response<ListContainersResponse>, tonic::Status> {
        RuntimeServiceImpl::list_containers(self, request).await
    }

    // TODO: 返回容器状态
    async fn container_status(
        &self,
        request: tonic::Request<ContainerStatusRequest>,
    ) -> std::result::Result<tonic::Response<ContainerStatusResponse>, tonic::Status> {
        RuntimeServiceImpl::container_status(self, request).await
    }

    // TODO: 更新容器资源配置
    async fn update_container_resources(
        &self,
        request: tonic::Request<UpdateContainerResourcesRequest>,
    ) -> std::result::Result<tonic::Response<UpdateContainerResourcesResponse>, tonic::Status> {
        RuntimeServiceImpl::update_container_resources(self, request).await
    }

    // TODO: 重新打开容器日志文件
    async fn reopen_container_log(
        &self,
        _request: tonic::Request<ReopenContainerLogRequest>,
    ) -> std::result::Result<tonic::Response<ReopenContainerLogResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "reopen_container_log: not implemented",
        ))
    }

    // ---- Exec / Attach / PortForward ----

    // TODO: 同步执行容器内命令
    async fn exec_sync(
        &self,
        request: tonic::Request<ExecSyncRequest>,
    ) -> std::result::Result<tonic::Response<ExecSyncResponse>, tonic::Status> {
        RuntimeServiceImpl::exec_sync(self, request).await
    }

    // TODO: 准备 exec 流式端点
    async fn exec(
        &self,
        request: tonic::Request<ExecRequest>,
    ) -> std::result::Result<tonic::Response<ExecResponse>, tonic::Status> {
        RuntimeServiceImpl::exec(self, request).await
    }

    // TODO: 准备 attach 流式端点
    async fn attach(
        &self,
        request: tonic::Request<AttachRequest>,
    ) -> std::result::Result<tonic::Response<AttachResponse>, tonic::Status> {
        RuntimeServiceImpl::attach(self, request).await
    }

    // TODO: 准备端口转发流式端点
    async fn port_forward(
        &self,
        _request: tonic::Request<PortForwardRequest>,
    ) -> std::result::Result<tonic::Response<PortForwardResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("port_forward: not implemented"))
    }

    // ---- Stats ----

    // TODO: 返回容器统计信息
    async fn container_stats(
        &self,
        request: tonic::Request<ContainerStatsRequest>,
    ) -> std::result::Result<tonic::Response<ContainerStatsResponse>, tonic::Status> {
        RuntimeServiceImpl::container_stats(self, request).await
    }

    // TODO: 列出所有运行中容器的统计信息
    async fn list_container_stats(
        &self,
        _request: tonic::Request<ListContainerStatsRequest>,
    ) -> std::result::Result<tonic::Response<ListContainerStatsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "list_container_stats: not implemented",
        ))
    }

    // TODO: 返回 Pod 沙箱统计信息
    async fn pod_sandbox_stats(
        &self,
        _request: tonic::Request<PodSandboxStatsRequest>,
    ) -> std::result::Result<tonic::Response<PodSandboxStatsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("pod_sandbox_stats: not implemented"))
    }

    // TODO: 列出匹配过滤器的 Pod 沙箱统计信息
    async fn list_pod_sandbox_stats(
        &self,
        _request: tonic::Request<ListPodSandboxStatsRequest>,
    ) -> std::result::Result<tonic::Response<ListPodSandboxStatsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "list_pod_sandbox_stats: not implemented",
        ))
    }

    // ---- Runtime 管理 ----

    // TODO: 更新运行时配置
    async fn update_runtime_config(
        &self,
        _request: tonic::Request<UpdateRuntimeConfigRequest>,
    ) -> std::result::Result<tonic::Response<UpdateRuntimeConfigResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "update_runtime_config: not implemented",
        ))
    }

    // TODO: 返回运行时状态
    async fn status(
        &self,
        request: tonic::Request<StatusRequest>,
    ) -> std::result::Result<tonic::Response<StatusResponse>, tonic::Status> {
        RuntimeServiceImpl::status(self, request).await
    }

    // TODO: 容器检查点
    async fn checkpoint_container(
        &self,
        _request: tonic::Request<CheckpointContainerRequest>,
    ) -> std::result::Result<tonic::Response<CheckpointContainerResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "checkpoint_container: not implemented",
        ))
    }

    // ---- Events（服务端流式） ----

    type GetContainerEventsStream =
        tokio_stream::wrappers::ReceiverStream<
            std::result::Result<ContainerEventResponse, tonic::Status>,>;

    // TODO: 获取容器事件流
    async fn get_container_events(
        &self,
        _request: tonic::Request<GetEventsRequest>,
    ) -> std::result::Result<tonic::Response<Self::GetContainerEventsStream>, tonic::Status> {
        Ok(Response::new(self.internal_services.events.stream()))
    }

    // ---- Metrics ----

    // TODO: 列出指标描述符
    async fn list_metric_descriptors(
        &self,
        _request: tonic::Request<ListMetricDescriptorsRequest>,
    ) -> std::result::Result<tonic::Response<ListMetricDescriptorsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "list_metric_descriptors: not implemented",
        ))
    }

    // TODO: 列出 Pod 沙箱指标
    async fn list_pod_sandbox_metrics(
        &self,
        _request: tonic::Request<ListPodSandboxMetricsRequest>,
    ) -> std::result::Result<tonic::Response<ListPodSandboxMetricsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "list_pod_sandbox_metrics: not implemented",
        ))
    }

    // ---- Config ----

    // TODO: 返回运行时配置信息
    async fn runtime_config(
        &self,
        _request: tonic::Request<RuntimeConfigRequest>,
    ) -> std::result::Result<tonic::Response<RuntimeConfigResponse>, tonic::Status> {
        let config = RuntimeConfigResponse {
            linux: Some(crate::proto::runtime::v1::LinuxRuntimeConfiguration {
                cgroup_driver: self.cgroup_driver() as i32,
            }),
        };

        Ok(Response::new(config))
    }

    // TODO: 更新 Pod 沙箱资源配置
    async fn update_pod_sandbox_resources(
        &self,
        _request: tonic::Request<UpdatePodSandboxResourcesRequest>,
    ) -> std::result::Result<tonic::Response<UpdatePodSandboxResourcesResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "update_pod_sandbox_resources: not implemented",
        ))
    }
}


#[derive(Debug)]
pub(super) struct NameReservationGuard {
    id: String,
    registry: StdArc<StdMutex<NameRegistry>>,
    active: bool,
}

impl NameReservationGuard {
    pub fn new(id: impl Into<String>, registry: StdArc<StdMutex<NameRegistry>>) -> Self {
        Self {
            id: id.into(),
            registry,
            active: true,
        }
    }
}

#[derive(Clone)]
pub struct RuntimeRegistry {
    default_handler: String,
    runtimes: Arc<HashMap<String, Arc<dyn RuntimeBackend>>>,
    container_create_timeouts: Arc<HashMap<String, u32>>,
    container_handlers: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl RuntimeRegistry {
    pub(super) fn new(
        default_handler: String,
        runtimes: HashMap<String, Arc<dyn RuntimeBackend>>,
        container_create_timeouts: HashMap<String, u32>,
    ) -> Self {
        Self {
            default_handler,
            runtimes: Arc::new(runtimes),
            container_create_timeouts: Arc::new(container_create_timeouts),
            container_handlers: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub(super) fn runtime_for_handler(
        &self,
        handler: &str,
    ) -> anyhow::Result<Arc<dyn RuntimeBackend>> {
        let resolved = if handler.trim().is_empty() {
            self.default_handler.as_str()
        } else {
            handler.trim()
        };
        self.runtimes
            .get(resolved)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unsupported runtime handler: {}", resolved))
    }

    pub(super) fn runtime_for_annotations_map(
        &self,
        annotations: &HashMap<String, String>,
    ) -> anyhow::Result<Arc<dyn RuntimeBackend>> {
        let handler = annotations
            .get(CRIO_RUNTIME_HANDLER_ANNOTATION)
            .or_else(|| annotations.get(CONTAINERD_RUNTIME_HANDLER_ANNOTATION))
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or(self.default_handler.as_str());
        self.runtime_for_handler(handler)
    }

    fn remember_container_handler(&self, container_id: &str, handler: &str) {
        if let Ok(mut handlers) = self.container_handlers.lock() {
            handlers.insert(container_id.to_string(), handler.to_string());
        }
    }


    pub(super) fn runtime_for_container(
        &self,
        container_id: &str,
    ) -> anyhow::Result<Arc<dyn RuntimeBackend>> {
        if let Ok(handlers) = self.container_handlers.lock() {
            if let Some(handler) = handlers.get(container_id) {
                return self.runtime_for_handler(handler);
            }
        }

        for (handler, runtime) in self.runtimes.iter() {
            if runtime
                .runtime_context()
                .bundle_path_for(container_id)
                .exists()
            {
                self.remember_container_handler(container_id, handler);
                return Ok(runtime.clone());
            }
        }

        self.runtime_for_handler(&self.default_handler)
    }
}