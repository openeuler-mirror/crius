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


use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc as StdArc, Mutex as StdMutex};

use crate::proto::runtime::v1::runtime_service_server::RuntimeService;
use crate::proto::runtime::v1::*;

/// 运行时配置
#[derive(Debug, Clone)]
pub struct RuntimeServiceConfig {
    pub root_dir: PathBuf,
    pub runtime: String,
    pub runtime_handlers: Vec<String>,
    // pub runtime_configs: HashMap<String, crate::config::ResolvedRuntimeHandlerConfig>,
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
    // pub seccomp_profile: PathBuf,
    // pub privileged_seccomp_profile: String,
    // pub unset_seccomp_profile: String,
    // pub apparmor_default_profile: String,
    // pub disable_apparmor: bool,
    // pub enable_selinux: bool,
    // pub selinux_category_range: u32,
    // pub hostnetwork_disable_selinux: bool,
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
    // pub cni_config: CniConfig,
    // pub local_cni_config: CniConfig,
    pub cgroup_driver: Option<CgroupDriver>,
    pub exec_sync_io_drain_timeout: std::time::Duration,
    pub max_container_log_line_size: usize,
    pub log_to_journald: bool,
    pub no_sync_log: bool,
    pub restrict_oom_score_adj: bool,
    pub enable_unprivileged_ports: bool,
    pub enable_unprivileged_icmp: bool,
    // pub rootless: crate::rootless::EffectiveRootlessConfig,
    // pub shim: ShimConfig,
    // pub streaming: crate::streaming::StreamingConfig,
    // pub config_path: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub(super) struct NameRegistry {
    ids_by_name: HashMap<String, String>,
    names_by_id: HashMap<String, String>,
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
    // pub(super) image_service: ImageServiceImpl,
    pub(super) shim_work_dir: PathBuf,
    pub(super) attach_socket_dir: PathBuf,
    pub(super) container_exits_dir: PathBuf,
    pub(super) clean_shutdown_file: PathBuf,
    pub(super) last_startup_clean_shutdown: StdArc<StdMutex<Option<bool>>>,
}

impl RuntimeServiceImpl {
    pub fn new(config: RuntimeServiceConfig) -> Self {
        let containers = Arc::new(Mutex::new(HashMap::new()));
        let pod_sandboxes = Arc::new(Mutex::new(HashMap::new()));
        let container_names = StdArc::new(StdMutex::new(NameRegistry::default()));
        let pod_names = StdArc::new(StdMutex::new(NameRegistry::default()));
        let mut config = config;
        let service = Self { 
            containers, 
            pod_sandboxes, 
            container_names, 
            pod_names, 
            removed_container_ids: Arc::new(Mutex::new(HashSet::new())), 
            removed_pod_sandbox_ids: Arc::new(Mutex::new(HashSet::new())), 
            config, 
            // image_service: (), 
            shim_work_dir: PathBuf::new(), 
            attach_socket_dir: PathBuf::new(), 
            container_exits_dir: PathBuf::new(), 
            clean_shutdown_file: PathBuf::new(), 
            last_startup_clean_shutdown: Arc::new(Mutex::new(None)), 
        };
        service
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
        _request: tonic::Request<CreateContainerRequest>,
    ) -> std::result::Result<tonic::Response<CreateContainerResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("create_container: not implemented"))
    }

    // TODO: 启动容器
    async fn start_container(
        &self,
        _request: tonic::Request<StartContainerRequest>,
    ) -> std::result::Result<tonic::Response<StartContainerResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("start_container: not implemented"))
    }

    // TODO: 停止容器（带 grace period）
    async fn stop_container(
        &self,
        _request: tonic::Request<StopContainerRequest>,
    ) -> std::result::Result<tonic::Response<StopContainerResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("stop_container: not implemented"))
    }

    // TODO: 移除容器
    async fn remove_container(
        &self,
        _request: tonic::Request<RemoveContainerRequest>,
    ) -> std::result::Result<tonic::Response<RemoveContainerResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("remove_container: not implemented"))
    }

    // TODO: 按过滤器列出容器
    async fn list_containers(
        &self,
        _request: tonic::Request<ListContainersRequest>,
    ) -> std::result::Result<tonic::Response<ListContainersResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("list_containers: not implemented"))
    }

    // TODO: 返回容器状态
    async fn container_status(
        &self,
        _request: tonic::Request<ContainerStatusRequest>,
    ) -> std::result::Result<tonic::Response<ContainerStatusResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("container_status: not implemented"))
    }

    // TODO: 更新容器资源配置
    async fn update_container_resources(
        &self,
        _request: tonic::Request<UpdateContainerResourcesRequest>,
    ) -> std::result::Result<tonic::Response<UpdateContainerResourcesResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "update_container_resources: not implemented",
        ))
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
        _request: tonic::Request<ExecSyncRequest>,
    ) -> std::result::Result<tonic::Response<ExecSyncResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("exec_sync: not implemented"))
    }

    // TODO: 准备 exec 流式端点
    async fn exec(
        &self,
        _request: tonic::Request<ExecRequest>,
    ) -> std::result::Result<tonic::Response<ExecResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("exec: not implemented"))
    }

    // TODO: 准备 attach 流式端点
    async fn attach(
        &self,
        _request: tonic::Request<AttachRequest>,
    ) -> std::result::Result<tonic::Response<AttachResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("attach: not implemented"))
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
        _request: tonic::Request<ContainerStatsRequest>,
    ) -> std::result::Result<tonic::Response<ContainerStatsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("container_stats: not implemented"))
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
        _request: tonic::Request<StatusRequest>,
    ) -> std::result::Result<tonic::Response<StatusResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("status: not implemented"))
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
            std::result::Result<ContainerEventResponse, tonic::Status>,
        >;

    // TODO: 获取容器事件流
    async fn get_container_events(
        &self,
        _request: tonic::Request<GetEventsRequest>,
    ) -> std::result::Result<tonic::Response<Self::GetContainerEventsStream>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "get_container_events: not implemented",
        ))
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
        Err(tonic::Status::unimplemented("runtime_config: not implemented"))
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
