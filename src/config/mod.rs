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


pub mod validation;

use std::{fs, path::{Path, PathBuf},};
use std::str::FromStr;
use std::collections::HashMap;

use serde::{Deserialize, Serialize, Deserializer};

use crate::error::Result;
use crate::defaults::*;
use crate::error::Error;
use crate::config::validation::{
    resolve_platform_runtime_path,
    resolve_monitor_cgroup,
    detect_system_cgroup_driver,
};
use crate::network::CniConfig;
use crate::network::types::MainIpPreference;

/// 守护进程主配置。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// 持久化根目录。
    pub root: String,

    /// API 配置。
    pub api: ApiConfig,

    /// 运行时配置。
    pub runtime: RuntimeConfig,

    /// 镜像配置。
    pub image: ImageConfig,

    // 网络配置。
    pub network: NetworkConfig,

    // 日志配置。
    pub logging: LoggingConfig,

    // 指标配置。
    // pub metrics: MetricsConfig,

    // tracing 导出配置。
    // pub tracing: TracingConfig,

    // NRI 配置
    // pub nri: NriConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig{
    /// CRI gRPC 监听地址。
    pub listen: String,
    /// 额外暴露的 Unix socket 别名；用于兼容 kubeadm 识别的标准 CRI socket 路径。
    pub listen_aliases: Vec<String>,
    /// 是否允许通过 TCP 暴露 CRI gRPC 服务。
    pub allow_tcp_service: bool,
    /// gRPC 最大发送消息大小（字节）。
    pub grpc_max_send_msg_size: u32,
    /// gRPC 最大接收消息大小（字节）。
    pub grpc_max_recv_msg_size: u32,
    /// 是否在 GetEvents 流中额外发送 pod-level lifecycle events。
    pub enable_pod_events: bool,
    /// ListPodSandboxMetrics 中包含哪些 pod-level metrics；支持 `all`、`cpu`、`memory`、`network`、`process`、`disk`。
    pub included_pod_metrics: Vec<String>,
    /// Pod/container stats 的缓存周期（秒）；0 表示按请求即时采集。
    pub stats_collection_period: u64,
    /// Pod sandbox metrics 的缓存周期（秒）；0 表示按请求即时采集。
    pub pod_sandbox_metrics_collection_period: u64,
    /// ExecSync 在主进程退出后等待 stdout/stderr EOF 的超时。
    pub exec_sync_io_drain_timeout: std::time::Duration,
    // 流式服务配置。
    // pub streaming: StreamingConfig,
}

/// 运行时配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    /// 默认运行时类型/handler 名称。
    pub runtime_type: String,
    /// OCI runtime 二进制路径。
    pub runtime_path: String,
    /// 默认 runtime 特定配置文件路径。
    pub runtime_config_path: String,
    /// 默认 OCI spec 模板文件；为空表示使用内建生成逻辑。
    pub base_runtime_spec: String,
    /// 运行时状态根目录。
    pub root: String,
    /// 默认 runtime 按平台覆盖的二进制路径映射，格式为 `os/arch -> path`。
    pub platform_runtime_paths: HashMap<String, String>,
    /// 对外暴露的 runtime handlers。
    pub handlers: Vec<String>,
    /// 按 handler 细化的 runtime 配置，参考 CRI-O runtimes 表。
    pub runtimes: HashMap<String, RuntimeHandlerConfig>,
    /// 按 workload 名称定义的 annotation 驱动资源预设。
    pub workloads: HashMap<String, RuntimeWorkloadConfig>,
    /// PodSandbox pause 镜像。
    pub pause_image: String,
    /// pause 镜像内的 infra 命令路径。
    pub pause_command: String,
    /// 命名空间辅助二进制路径；为空时使用内建 netns 管理逻辑。
    pub pinns_path: String,
    /// 是否允许在特定场景下省略 infra/pause 容器。
    pub drop_infra_ctr: bool,
    /// 可选的 cgroup driver 显式配置。
    pub cgroup_driver: Option<CgroupDriverConfig>,
    /// shim 二进制路径。
    pub shim_path: String,
    /// 默认 monitor/shim 所在 cgroup；支持空字符串、`pod` 或 systemd slice。
    pub monitor_cgroup: String,
    /// shim 工作目录。
    pub shim_dir: String,
    /// attach/resize socket 根目录。
    pub attach_socket_dir: String,
    /// 容器退出记录根目录。
    pub container_exits_dir: String,
    /// 干净退出标记文件。
    pub clean_shutdown_file: String,
    /// 容器优雅停止的最小等待时间（秒）。
    pub container_stop_timeout: u32,
    /// 临时版本标记文件，用于识别 reboot 后启动。
    pub version_file: String,
    /// 持久版本标记文件，用于识别升级后的恢复分支。
    pub version_file_persist: String,
    /// 可选的 CRIU 二进制路径；为空时使用 runtime 默认行为。
    pub criu_path: String,
    /// 可选的 CRIU image staging 根目录；为空时沿用 artifact 邻接目录。
    pub criu_image_path: String,
    /// 可选的 CRIU work staging 根目录；为空时默认落到 image 目录下的 `work/`。
    pub criu_work_path: String,
    /// 是否启用 checkpoint/restore 支持。
    pub enable_criu_support: bool,
    /// 是否允许启动期自动清理孤儿 runtime/shim/pod 工件。
    pub internal_wipe: bool,
    /// 是否在 unclean 启动时检查并尝试修复持久化账本。
    pub internal_repair: bool,
    /// 对所有 bind mount source 添加的宿主路径前缀；为空表示不改写。
    pub bind_mount_prefix: String,
    /// 是否禁用 cgroup 支持。
    pub disable_cgroup: bool,
    /// hugetlb controller 缺失时是否容忍并忽略 hugepage limits。
    pub tolerate_missing_hugetlb_controller: bool,
    /// 镜像拉取使用的独立 cgroup；支持空值、`pod` 或指定 cgroup path。
    pub separate_pull_cgroup: String,
    /// 守护进程默认的 UID 映射，格式为 `container:host:size[,..]`。
    pub uid_mappings: String,
    /// 守护进程默认的 GID 映射，格式为 `container:host:size[,..]`。
    pub gid_mappings: String,
    /// 非 root userns 映射允许使用的最小宿主 UID；-1 表示不限制。
    pub minimum_mappable_uid: i64,
    /// 非 root userns 映射允许使用的最小宿主 GID；-1 表示不限制。
    pub minimum_mappable_gid: i64,
    /// shim 创建的宿主 IO 工件默认 UID。
    pub io_uid: u32,
    /// shim 创建的宿主 IO 工件默认 GID。
    pub io_gid: u32,
    /// 守护进程默认的 pids 限制；-1 表示不设置默认限制。
    pub pids_limit: i64,
    /// infra/pause 容器默认 cpuset。
    pub infra_ctr_cpuset: String,
    /// 允许 guaranteed workload 共享使用的 cpuset。
    pub shared_cpuset: String,
    /// exec / execSync 的 CPU 亲和策略；"" 表示 runtime 默认，"first" 表示使用 cpuset 的第一个 CPU。
    pub exec_cpu_affinity: String,
    /// irqbalance 守护进程配置文件路径。
    pub irqbalance_config_file: String,
    /// irqbalance banned CPU mask 启动期恢复文件；`disable` 表示禁用恢复逻辑。
    pub irqbalance_config_restore_file: String,
    /// 是否默认将所有 Pod/容器根文件系统设为只读。
    pub read_only: bool,
    /// 是否禁用 pivot_root，改用 MS_MOVE。
    pub no_pivot: bool,
    /// 是否禁止为容器创建新的 session keyring。
    pub no_new_keyring: bool,
    /// 是否启用 shim debug。
    pub shim_debug: bool,
    /// 传给 monitor/shim 进程的默认环境变量列表，格式为 `KEY=value`。
    pub monitor_env: Vec<String>,
    /// 注入到所有容器的默认环境变量，格式为 `KEY=value`。
    pub default_env: Vec<String>,
    /// daemon 级默认 capabilities 列表。
    pub default_capabilities: Vec<String>,
    /// daemon 级默认 sysctls，格式为 `key = value` 或 `key=value`。
    pub default_sysctls: Vec<String>,
    /// daemon 级默认 ulimits，格式为 `name=soft:hard`。
    pub default_ulimits: Vec<String>,
    /// 允许 CRI 请求映射进容器的宿主设备路径列表。
    pub allowed_devices: Vec<String>,
    /// 注入到所有容器的额外宿主设备映射，格式为 `/SRC[:/DST[:PERMS]]`。
    pub additional_devices: Vec<String>,
    /// 是否将设备节点的 uid/gid 跟随 security context 的 runAsUser/runAsGroup。
    pub device_ownership_from_security_context: bool,
    /// 是否把 capabilities 同步写入 inheritable 集合。
    pub add_inheritable_capabilities: bool,
    /// 默认附加挂载文件，格式为 `/SRC:/DST`，一行一个。
    pub default_mounts_file: String,
    /// OCI hooks 目录列表；后面的目录拥有更高同名文件优先级。
    pub hooks_dir: Vec<String>,
    /// 宿主关键挂载源缺失时需要直接拒绝创建的路径列表。
    pub absent_mount_sources_to_reject: Vec<String>,
    /// 是否禁用 Kubernetes ProcMount 支持。
    pub disable_proc_mount: bool,
    /// 容器时区策略；空字符串表示不注入，`Local` 表示跟随宿主机。
    pub timezone: String,
    /// 是否在 CRI 日志文件之外额外写 journald。
    pub log_to_journald: bool,
    /// 是否在日志轮转和容器退出时跳过 sync。
    pub no_sync_log: bool,
    /// 是否将容器/Pod 的 OOMScoreAdj 下界限制为 daemon 当前值。
    pub restrict_oom_score_adj: bool,
    /// 为非 hostNetwork Pod 默认开启低位端口绑定。
    pub enable_unprivileged_ports: bool,
    /// 为非 hostNetwork 且非 userns Pod 默认开启 ping group range。
    pub enable_unprivileged_icmp: bool,
}

impl RuntimeConfig {
    pub fn effective_cgroup_driver(&self) -> CgroupDriverConfig {
        self.cgroup_driver
            .unwrap_or_else(detect_system_cgroup_driver)
    }

    pub fn normalized_handlers(&self) -> Vec<String> {
        let mut handlers = Vec::new();
        for handler in &self.handlers {
            let trimmed = handler.trim();
            if !trimmed.is_empty() && !handlers.iter().any(|existing: &String| existing == trimmed)
            {
                handlers.push(trimmed.to_string());
            }
        }
        for handler in self.runtimes.keys() {
            let trimmed = handler.trim();
            if !trimmed.is_empty() && !handlers.iter().any(|existing: &String| existing == trimmed)
            {
                handlers.push(trimmed.to_string());
            }
        }
        if !handlers.iter().any(|handler| handler == &self.runtime_type) {
            handlers.push(self.runtime_type.clone());
        }
        handlers
    }

    pub fn resolved_runtimes(&self) -> Result<HashMap<String, ResolvedRuntimeHandlerConfig>> {
        let default_handler = self.runtime_type.trim();
        let default_runtime = ResolvedRuntimeHandlerConfig {
            backend: "runc".to_string(),
            backend_options: HashMap::new(),
            runtime_path: resolve_platform_runtime_path(
                self.runtime_path.trim(),
                &self.platform_runtime_paths,
            )?,
            runtime_config_path: self.runtime_config_path.trim().to_string(),
            runtime_root: self.root.trim().to_string(),
            platform_runtime_paths: self.platform_runtime_paths.clone(),
            monitor_path: self.shim_path.trim().to_string(),
            monitor_cgroup: resolve_monitor_cgroup(
                self.monitor_cgroup.as_str(),
                self.effective_cgroup_driver(),
            )?,
            monitor_env: self.monitor_env.clone(),
            stream_websockets: true,
            allowed_annotations: Vec::new(),
            default_annotations: HashMap::new(),
            privileged_without_host_devices: false,
            privileged_without_host_devices_all_devices_allowed: false,
            container_create_timeout: DEFAULT_CONTAINER_CREATE_TIMEOUT_SECS,
            snapshotter: "internal-overlay-untar".to_string(),
        };
        let mut resolved = HashMap::from([(default_handler.to_string(), default_runtime.clone())]);

        for handler in self.normalized_handlers() {
            if handler == default_handler {
                continue;
            }

            let Some(config) = self.runtimes.get(&handler) else {
                return Err(Error::Config(format!(
                    "runtime handler {handler} requires [runtime.runtimes.{handler}]"
                )));
            };

            if config.inherit_default_runtime {
                if !config.runtime_path.trim().is_empty() || !config.runtime_root.trim().is_empty()
                {
                    return Err(Error::Config(format!(
                        "runtime.runtimes.{handler} must not set runtime_path/runtime_root when inherit_default_runtime = true"
                    )));
                }
                let mut inherited = default_runtime.clone();
                if !config.runtime_config_path.trim().is_empty() {
                    inherited.runtime_config_path = config.runtime_config_path.trim().to_string();
                }
                if !config.backend.trim().is_empty() {
                    inherited.backend = config.backend.trim().to_string();
                }
                inherited.backend_options = config.backend_options.clone();
                if !config.platform_runtime_paths.is_empty() {
                    inherited.platform_runtime_paths = config.platform_runtime_paths.clone();
                    inherited.runtime_path = resolve_platform_runtime_path(
                        self.runtime_path.trim(),
                        &inherited.platform_runtime_paths,
                    )?;
                }
                if !config.monitor_path.trim().is_empty() {
                    inherited.monitor_path = config.monitor_path.trim().to_string();
                }
                if let Some(monitor_cgroup) = config.monitor_cgroup.as_deref() {
                    inherited.monitor_cgroup =
                        resolve_monitor_cgroup(monitor_cgroup, self.effective_cgroup_driver())?;
                }
                if let Some(monitor_env) = config.monitor_env.as_ref() {
                    inherited.monitor_env = monitor_env.clone();
                }
                if let Some(stream_websockets) = config.stream_websockets {
                    inherited.stream_websockets = stream_websockets;
                }
                inherited.allowed_annotations = config.allowed_annotations.clone();
                inherited.default_annotations = config.default_annotations.clone();
                inherited.privileged_without_host_devices = config.privileged_without_host_devices;
                inherited.privileged_without_host_devices_all_devices_allowed =
                    config.privileged_without_host_devices_all_devices_allowed;
                if let Some(timeout) = config.container_create_timeout {
                    inherited.container_create_timeout =
                        timeout.max(MIN_CONTAINER_CREATE_TIMEOUT_SECS);
                }
                if !config.snapshotter.trim().is_empty() {
                    inherited.snapshotter = config.snapshotter.trim().to_string();
                }
                resolved.insert(handler, inherited);
                continue;
            }

            if config.runtime_path.trim().is_empty() {
                return Err(Error::Config(format!(
                    "runtime.runtimes.{handler}.runtime_path must not be empty"
                )));
            }

            resolved.insert(
                handler,
                ResolvedRuntimeHandlerConfig {
                    backend: if config.backend.trim().is_empty() {
                        default_runtime.backend.clone()
                    } else {
                        config.backend.trim().to_string()
                    },
                    backend_options: config.backend_options.clone(),
                    runtime_path: resolve_platform_runtime_path(
                        config.runtime_path.trim(),
                        &config.platform_runtime_paths,
                    )?,
                    runtime_config_path: if config.runtime_config_path.trim().is_empty() {
                        default_runtime.runtime_config_path.clone()
                    } else {
                        config.runtime_config_path.trim().to_string()
                    },
                    runtime_root: if config.runtime_root.trim().is_empty() {
                        default_runtime.runtime_root.clone()
                    } else {
                        config.runtime_root.trim().to_string()
                    },
                    platform_runtime_paths: config.platform_runtime_paths.clone(),
                    monitor_path: if config.monitor_path.trim().is_empty() {
                        default_runtime.monitor_path.clone()
                    } else {
                        config.monitor_path.trim().to_string()
                    },
                    monitor_cgroup: resolve_monitor_cgroup(
                        config
                            .monitor_cgroup
                            .as_deref()
                            .unwrap_or(default_runtime.monitor_cgroup.as_str()),
                        self.effective_cgroup_driver(),
                    )?,
                    monitor_env: config
                        .monitor_env
                        .clone()
                        .unwrap_or_else(|| self.monitor_env.clone()),
                    stream_websockets: config
                        .stream_websockets
                        .unwrap_or(default_runtime.stream_websockets),
                    allowed_annotations: config.allowed_annotations.clone(),
                    default_annotations: config.default_annotations.clone(),
                    privileged_without_host_devices: config.privileged_without_host_devices,
                    privileged_without_host_devices_all_devices_allowed: config
                        .privileged_without_host_devices_all_devices_allowed,
                    container_create_timeout: config
                        .container_create_timeout
                        .unwrap_or(DEFAULT_CONTAINER_CREATE_TIMEOUT_SECS)
                        .max(MIN_CONTAINER_CREATE_TIMEOUT_SECS),
                    snapshotter: if config.snapshotter.trim().is_empty() {
                        "internal-overlay-untar".to_string()
                    } else {
                        config.snapshotter.trim().to_string()
                    },
                },
            );
        }

        Ok(resolved)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageConfig{
    /// 镜像存储后端。
    pub driver: String,
    /// 镜像存储路径。
    pub root: String,
    /// 守护进程级 registry 鉴权文件，兼容 docker config.json 的 auths 结构。
    pub global_auth_file: String,
    /// 按 Pod namespace 分隔的 registry 鉴权目录。
    pub namespaced_auth_dir: String,
    /// 默认镜像拉取 transport。
    pub default_transport: String,
    /// 短名解析策略。
    pub short_name_mode: String,
    /// 镜像拉取无进展超时。
    pub pull_progress_timeout: std::time::Duration,
    /// 单镜像最大并发下载层数。
    pub max_concurrent_downloads: usize,
    /// 拉取失败时的额外重试次数。
    pub pull_retry_count: u32,
    /// registry hosts.toml/certs.d 配置目录。
    pub registry_config_dir: String,
    /// 节点本地镜像解密私钥目录；为空表示关闭镜像解密。
    pub decryption_keys_path: String,
    /// OCI crypt decoder 二进制路径或命令名。
    pub decryption_decoder_path: String,
    /// 可选的 OCICRYPT keyprovider 配置文件路径。
    pub decryption_keyprovider_config: String,
    /// 额外的只读 OCI artifact store 根目录列表；每个目录下期望存在 `artifacts/` 子目录。
    pub additional_artifact_stores: Vec<String>,
    /// 全局镜像签名策略文件。
    pub signature_policy: String,
    /// 按 namespace 选择镜像签名策略文件的目录。
    pub signature_policy_dir: String,
    /// 镜像存储驱动额外参数。
    pub storage_options: Vec<String>,
    /// 镜像定义卷处理策略；支持 `mkdir`、`bind`、`ignore`。
    pub image_volumes: String,
    /// 不参与 kubelet 垃圾回收的保留镜像模式列表。
    pub pinned_images: Vec<String>,
    /// 大 layer staging 的临时目录；为空表示使用镜像目录同盘临时文件。
    pub big_files_temporary_dir: String,
    /// 是否允许把 OCI artifact 作为 CRI image volume mount 到容器中。
    pub oci_artifact_mount_support: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig{
    /// tracing filter/level。
    pub level: String,
    /// 可选的日志文件路径；为空时输出到 stderr。
    pub file: Option<String>,
    /// daemon/container 默认日志目录。
    pub dir: String,
    /// CRI 单条日志记录切分阈值（字节）。
    pub max_container_log_line_size: usize,
}

/// 单个 runtime handler 的细化配置。
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeHandlerConfig {
    /// 该 handler 绑定的 runtime backend 类型。
    pub backend: String,
    /// 该 handler 传递给 backend 的差异化配置项。
    pub backend_options: HashMap<String, String>,
    /// 该 handler 对应的 OCI runtime 二进制路径。
    pub runtime_path: String,
    /// 该 handler 对应的 runtime 特定配置文件路径；为空时继承默认值。
    pub runtime_config_path: String,
    /// 该 handler 对应的 runtime 状态根目录。
    pub runtime_root: String,
    /// 该 handler 按平台覆盖的 runtime 二进制路径映射。
    pub platform_runtime_paths: HashMap<String, String>,
    /// 是否继承默认 handler 的 runtime_path/runtime_root。
    pub inherit_default_runtime: bool,
    /// 该 handler 专属的 monitor/shim 二进制路径；为空时继承 runtime.shim_path。
    pub monitor_path: String,
    /// 该 handler 专属的 monitor/shim cgroup；未设置时继承 runtime.monitor_cgroup。
    pub monitor_cgroup: Option<String>,
    /// 该 handler 专属的 monitor/shim 环境变量；未设置时继承 runtime.monitor_env。
    pub monitor_env: Option<Vec<String>>,
    /// 是否允许该 handler 的 exec/attach/port-forward 使用 websocket 协议；未设置时继承默认值。
    pub stream_websockets: Option<bool>,
    /// 该 handler 额外允许处理的 annotation 前缀。
    pub allowed_annotations: Vec<String>,
    /// 该 handler 默认注入到 OCI annotations 的键值对；显式请求优先。
    pub default_annotations: HashMap<String, String>,
    /// privileged 容器是否跳过默认宿主设备注入。
    pub privileged_without_host_devices: bool,
    /// 在跳过宿主设备注入时，是否仍维持全设备 allowlist。
    pub privileged_without_host_devices_all_devices_allowed: bool,
    /// 该 handler 的容器创建超时（秒）；未设置时继承内置默认值。
    pub container_create_timeout: Option<u32>,
    /// 该 handler 的 rootfs snapshotter；为空时使用默认 `internal-overlay-untar`。
    pub snapshotter: String,
    /// 该 handler 专属的 CNI 配置目录；为空时继承全局 network.config_dirs。
    pub cni_conf_dir: String,
    /// 该 handler 专属的 CNI 配置文件最大加载数量；未设置时继承全局 network.max_conf_num。
    #[serde(alias = "cni_max_conf_num")]
    pub cni_max_conf_num: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct RuntimeWorkloadConfig {
    /// 激活该 workload 的 Pod annotation key。
    pub activation_annotation: String,
    /// 容器级资源覆盖 annotation 前缀。
    pub annotation_prefix: String,
    /// 该 workload 额外允许的 annotation 列表。
    pub allowed_annotations: Vec<String>,
    /// 该 workload 的默认资源预设。
    pub resources: RuntimeWorkloadResources,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RuntimeWorkloadResources {
    /// 默认 CPU shares。
    #[serde(rename = "cpushares")]
    pub cpu_shares: i64,
    /// 默认 CPU quota（微秒）。
    #[serde(rename = "cpuquota")]
    pub cpu_quota: i64,
    /// 默认 CPU period（微秒）。
    #[serde(rename = "cpuperiod")]
    pub cpu_period: i64,
    /// 默认 cpuset CPUs。
    #[serde(rename = "cpuset")]
    pub cpuset_cpus: String,
    /// 默认 CPU limit（millicores）。
    #[serde(rename = "cpulimit")]
    pub cpu_limit: i64,
}

/// 守护进程 cgroup driver 配置。
#[derive(Debug, Clone, Deserialize, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CgroupDriverConfig {
    Systemd,
    Cgroupfs,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn apply_env_overrides(&mut self) -> Result<()> {
        apply_string_override("CRIUS_ROOT", &mut self.root);
        apply_string_override("CRIUS_LISTEN", &mut self.api.listen);
        apply_csv_override("CRIUS_LISTEN_ALIASES", &mut self.api.listen_aliases);
        apply_bool_override("CRIUS_ALLOW_TCP_SERVICE", &mut self.api.allow_tcp_service)?;
        apply_numeric_override(
            "CRIUS_GRPC_MAX_SEND_MSG_SIZE",
            &mut self.api.grpc_max_send_msg_size,
        )?;
        apply_numeric_override(
            "CRIUS_GRPC_MAX_RECV_MSG_SIZE",
            &mut self.api.grpc_max_recv_msg_size,
        )?;
        apply_bool_override("CRIUS_ENABLE_POD_EVENTS", &mut self.api.enable_pod_events)?;
        apply_csv_override(
            "CRIUS_INCLUDED_POD_METRICS",
            &mut self.api.included_pod_metrics,
        );
        apply_numeric_override(
            "CRIUS_STATS_COLLECTION_PERIOD",
            &mut self.api.stats_collection_period,
        )?;
        apply_numeric_override(
            "CRIUS_POD_SANDBOX_METRICS_COLLECTION_PERIOD",
            &mut self.api.pod_sandbox_metrics_collection_period,
        )?;
        apply_string_override("CRIUS_IMAGE_DRIVER", &mut self.image.driver);
        apply_string_override("CRIUS_IMAGE_ROOT", &mut self.image.root);
        apply_string_override(
            "CRIUS_IMAGE_GLOBAL_AUTH_FILE",
            &mut self.image.global_auth_file,
        );
        apply_string_override(
            "CRIUS_IMAGE_NAMESPACED_AUTH_DIR",
            &mut self.image.namespaced_auth_dir,
        );
        apply_string_override(
            "CRIUS_IMAGE_DEFAULT_TRANSPORT",
            &mut self.image.default_transport,
        );
        apply_string_override(
            "CRIUS_IMAGE_SHORT_NAME_MODE",
            &mut self.image.short_name_mode,
        );
        apply_numeric_override(
            "CRIUS_MAX_CONCURRENT_DOWNLOADS",
            &mut self.image.max_concurrent_downloads,
        )?;
        apply_numeric_override(
            "CRIUS_IMAGE_PULL_RETRY_COUNT",
            &mut self.image.pull_retry_count,
        )?;
        apply_string_override(
            "CRIUS_IMAGE_REGISTRY_CONFIG_DIR",
            &mut self.image.registry_config_dir,
        );
        apply_string_override(
            "CRIUS_IMAGE_DECRYPTION_KEYS_PATH",
            &mut self.image.decryption_keys_path,
        );
        apply_string_override(
            "CRIUS_IMAGE_DECRYPTION_DECODER_PATH",
            &mut self.image.decryption_decoder_path,
        );
        apply_string_override(
            "CRIUS_IMAGE_DECRYPTION_KEYPROVIDER_CONFIG",
            &mut self.image.decryption_keyprovider_config,
        );
        apply_csv_override(
            "CRIUS_ADDITIONAL_ARTIFACT_STORES",
            &mut self.image.additional_artifact_stores,
        );
        apply_string_override(
            "CRIUS_IMAGE_SIGNATURE_POLICY",
            &mut self.image.signature_policy,
        );
        apply_string_override(
            "CRIUS_IMAGE_SIGNATURE_POLICY_DIR",
            &mut self.image.signature_policy_dir,
        );
        apply_csv_override(
            "CRIUS_IMAGE_STORAGE_OPTIONS",
            &mut self.image.storage_options,
        );
        apply_string_override("CRIUS_IMAGE_VOLUMES", &mut self.image.image_volumes);
        apply_csv_override("CRIUS_PINNED_IMAGES", &mut self.image.pinned_images);
        apply_string_override(
            "CRIUS_IMAGE_BIG_FILES_TEMPORARY_DIR",
            &mut self.image.big_files_temporary_dir,
        );
        apply_bool_override(
            "CRIUS_OCI_ARTIFACT_MOUNT_SUPPORT",
            &mut self.image.oci_artifact_mount_support,
        )?;
        apply_string_override("CRIUS_LOG_LEVEL", &mut self.logging.level);
        apply_optional_string_override("CRIUS_LOG_FILE", &mut self.logging.file);
        apply_string_override("CRIUS_LOG_DIR", &mut self.logging.dir);
        apply_numeric_override(
            "CRIUS_MAX_CONTAINER_LOG_LINE_SIZE",
            &mut self.logging.max_container_log_line_size,
        )?;

        Ok(())
    }

}

impl Default for ApiConfig {
    fn default() -> Self {
        Self { 
            listen: DEFAULT_CRI_SOCKET_URI.to_string(),
            listen_aliases: Vec::new(),
            allow_tcp_service: false,
            grpc_max_send_msg_size: DEFAULT_GRPC_MAX_MESSAGE_SIZE_BYTES,
            grpc_max_recv_msg_size: DEFAULT_GRPC_MAX_MESSAGE_SIZE_BYTES,
            enable_pod_events: true,
            included_pod_metrics: vec!["all".to_string()],
            stats_collection_period: 0,
            pod_sandbox_metrics_collection_period: 0,
            exec_sync_io_drain_timeout: std::time::Duration::ZERO, 
        }        
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            runtime_type: "runc".to_string(),
            runtime_path: "/usr/bin/runc".to_string(),
            runtime_config_path: String::new(),
            base_runtime_spec: String::new(),
            root: DEFAULT_RUNTIME_STATE_DIR.to_string(),
            platform_runtime_paths: HashMap::new(),
            handlers: Vec::new(),
            runtimes: HashMap::new(),
            workloads: HashMap::new(),
            pause_image: "registry.k8s.io/pause:3.9".to_string(),
            pause_command: "/pause".to_string(),
            pinns_path: String::new(),
            drop_infra_ctr: false,
            cgroup_driver: None,
            shim_path: "/usr/bin/crius-shim".to_string(),
            monitor_cgroup: String::new(),
            shim_dir: DEFAULT_RUNTIME_SHIM_DIR.to_string(),
            attach_socket_dir: DEFAULT_RUNTIME_ATTACH_SOCKET_DIR.to_string(),
            container_exits_dir: DEFAULT_RUNTIME_CONTAINER_EXITS_DIR.to_string(),
            clean_shutdown_file: DEFAULT_RUNTIME_CLEAN_SHUTDOWN_FILE.to_string(),
            container_stop_timeout: MIN_CONTAINER_STOP_TIMEOUT_SECS,
            version_file: DEFAULT_RUNTIME_VERSION_FILE.to_string(),
            version_file_persist: DEFAULT_RUNTIME_VERSION_FILE_PERSIST.to_string(),
            criu_path: String::new(),
            criu_image_path: String::new(),
            criu_work_path: String::new(),
            enable_criu_support: true,
            internal_wipe: true,
            internal_repair: true,
            bind_mount_prefix: String::new(),
            disable_cgroup: false,
            tolerate_missing_hugetlb_controller: true,
            separate_pull_cgroup: String::new(),
            uid_mappings: String::new(),
            gid_mappings: String::new(),
            minimum_mappable_uid: -1,
            minimum_mappable_gid: -1,
            io_uid: 0,
            io_gid: 0,
            pids_limit: -1,
            infra_ctr_cpuset: String::new(),
            shared_cpuset: String::new(),
            exec_cpu_affinity: String::new(),
            irqbalance_config_file: String::new(),
            irqbalance_config_restore_file: "disable".to_string(),
            read_only: false,
            no_pivot: false,
            no_new_keyring: false,
            shim_debug: false,
            monitor_env: Vec::new(),
            default_env: Vec::new(),
            default_capabilities: vec![
                "CHOWN".to_string(),
                "DAC_OVERRIDE".to_string(),
                "FSETID".to_string(),
                "FOWNER".to_string(),
                "MKNOD".to_string(),
                "NET_RAW".to_string(),
                "SETGID".to_string(),
                "SETUID".to_string(),
                "SETFCAP".to_string(),
                "SETPCAP".to_string(),
                "NET_BIND_SERVICE".to_string(),
                "SYS_CHROOT".to_string(),
                "KILL".to_string(),
                "AUDIT_WRITE".to_string(),
            ],
            default_sysctls: Vec::new(),
            default_ulimits: Vec::new(),
            allowed_devices: Vec::new(),
            additional_devices: Vec::new(),
            device_ownership_from_security_context: false,
            add_inheritable_capabilities: false,
            default_mounts_file: String::new(),
            hooks_dir: Vec::new(),
            absent_mount_sources_to_reject: Vec::new(),
            disable_proc_mount: false,
            timezone: String::new(),
            log_to_journald: false,
            no_sync_log: false,
            restrict_oom_score_adj: false,
            enable_unprivileged_ports: false,
            enable_unprivileged_icmp: false,
        }
    }
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            driver: DEFAULT_STORAGE_DRIVER.to_string(),
            root: DEFAULT_CONTAINER_STORAGE_DIR.to_string(),
            global_auth_file: String::new(),
            namespaced_auth_dir: String::new(),
            default_transport: "docker://".to_string(),
            short_name_mode: "disabled".to_string(),
            pull_progress_timeout: std::time::Duration::ZERO,
            max_concurrent_downloads: 3,
            pull_retry_count: 0,
            registry_config_dir: String::new(),
            decryption_keys_path: String::new(),
            decryption_decoder_path: "ctd-decoder".to_string(),
            decryption_keyprovider_config: String::new(),
            additional_artifact_stores: Vec::new(),
            signature_policy: String::new(),
            signature_policy_dir: String::new(),
            storage_options: Vec::new(),
            image_volumes: "mkdir".to_string(),
            pinned_images: Vec::new(),
            big_files_temporary_dir: String::new(),
            oci_artifact_mount_support: true, 
             }        
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self { 
            level: "info".to_string(), 
            file: None, 
            dir: "/var/log/crius".to_string(),
            max_container_log_line_size: 4096 
        }
    }    
}

impl CgroupDriverConfig {
    pub fn as_proto(self) -> crate::proto::runtime::v1::CgroupDriver {
        match self {
            Self::Systemd => crate::proto::runtime::v1::CgroupDriver::Systemd,
            Self::Cgroupfs => crate::proto::runtime::v1::CgroupDriver::Cgroupfs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRuntimeHandlerConfig {
    pub backend: String,
    pub backend_options: HashMap<String, String>,
    pub runtime_path: String,
    pub runtime_config_path: String,
    pub runtime_root: String,
    pub platform_runtime_paths: HashMap<String, String>,
    pub monitor_path: String,
    pub monitor_cgroup: String,
    pub monitor_env: Vec<String>,
    pub stream_websockets: bool,
    pub allowed_annotations: Vec<String>,
    pub default_annotations: HashMap<String, String>,
    pub privileged_without_host_devices: bool,
    pub privileged_without_host_devices_all_devices_allowed: bool,
    pub container_create_timeout: u32,
    pub snapshotter: String,
}

impl Default for ResolvedRuntimeHandlerConfig {
    fn default() -> Self {
        Self {
            backend: "runc".to_string(),
            backend_options: HashMap::new(),
            runtime_path: String::new(),
            runtime_config_path: String::new(),
            runtime_root: String::new(),
            platform_runtime_paths: HashMap::new(),
            monitor_path: String::new(),
            monitor_cgroup: String::new(),
            monitor_env: Vec::new(),
            stream_websockets: true,
            allowed_annotations: Vec::new(),
            default_annotations: HashMap::new(),
            privileged_without_host_devices: false,
            privileged_without_host_devices_all_devices_allowed: false,
            container_create_timeout: DEFAULT_CONTAINER_CREATE_TIMEOUT_SECS,
            snapshotter: "internal-overlay-untar".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkDomainConfig {
    /// CNI 配置目录。
    #[serde(
        default,
        alias = "config_dir",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub config_dirs: Vec<String>,
    /// CNI 插件目录。
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub plugin_dirs: Vec<String>,
    /// CNI 缓存目录。
    pub cache_dir: String,
    /// CNI 配置模板文件路径。
    pub conf_template: String,
    /// 最大允许加载的 CNI 配置文件数量；0 表示不限制。
    #[serde(alias = "cni_max_conf_num")]
    pub max_conf_num: usize,
    /// Pod 主 IP 选择策略。
    pub ip_pref: MainIpPreference,
    /// CNI teardown/DEL 的固定超时。
    pub teardown_timeout: std::time::Duration,
    /// 显式指定默认使用的 CNI 网络名。
    #[serde(alias = "cni_default_network")]
    pub default_network_name: Option<String>,
    /// 是否全局禁用 hostPort 映射。
    pub disable_hostport_mapping: bool,
    /// 是否将 netns 挂载统一放到 runtime state dir 下。
    #[serde(alias = "netns_mounts_under_state_dir")]
    pub netns_mounts_under_state_dir: bool,
}

impl Default for NetworkDomainConfig {
    fn default() -> Self {
        Self::cri_default()
    }
}

impl NetworkDomainConfig {
    fn local_default() -> Self {
        Self {
            config_dirs: vec!["/etc/crius/cni/net.d".to_string()],
            ..Self::common_default()
        }
    }

    fn cri_default() -> Self {
        Self {
            config_dirs: vec![
                "/etc/cni/net.d".to_string(),
                "/etc/kubernetes/cni/net.d".to_string(),
            ],
            ..Self::common_default()
        }
    }

    fn common_default() -> Self {
        Self {
            config_dirs: Vec::new(),
            plugin_dirs: vec![
                "/opt/cni/bin".to_string(),
                "/usr/lib/cni".to_string(),
                "/usr/libexec/cni".to_string(),
            ],
            cache_dir: "/var/lib/cni/cache".to_string(),
            conf_template: String::new(),
            max_conf_num: 0,
            ip_pref: MainIpPreference::Cni,
            teardown_timeout: std::time::Duration::from_secs(60),
            default_network_name: None,
            disable_hostport_mapping: false,
            netns_mounts_under_state_dir:false,
        }
    }

    pub fn cni_config(&self) -> CniConfig {
        let mut cni = CniConfig::new(
            self.config_dirs.iter().map(PathBuf::from).collect(),
            self.plugin_dirs.iter().map(PathBuf::from).collect(),
            PathBuf::from(&self.cache_dir),
            self.max_conf_num,
            self.ip_pref,
            self.default_network_name.clone(),
            self.disable_hostport_mapping,
        );
        cni.set_teardown_timeout(self.teardown_timeout);
        if !self.conf_template.trim().is_empty() {
            cni.set_conf_template(Some(PathBuf::from(self.conf_template.trim())));
        }
        cni.set_netns_mounts_under_state_dir(self.netns_mounts_under_state_dir);
        cni
    }
}

/// 网络配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// 网络插件类型。
    pub plugin: String,
    /// CNI 配置目录，兼容旧单目录配置。
    #[serde(
        default,
        alias = "config_dir",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub config_dirs: Vec<String>,
    /// CNI 插件目录。
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub plugin_dirs: Vec<String>,
    /// CNI 缓存目录。
    pub cache_dir: String,
    /// CNI 配置模板文件路径；开启后 UpdateRuntimeConfig 可基于 PodCIDR 渲染配置文件。
    pub conf_template: String,
    /// 最大允许加载的 CNI 配置文件数量；0 表示不限制。
    #[serde(alias = "cni_max_conf_num")]
    pub max_conf_num: usize,
    /// Pod 主 IP 选择策略。
    pub ip_pref: MainIpPreference,
    /// CNI teardown/DEL 的固定超时；默认 1 分钟，对齐 CRI-O 的 networkStop 语义。
    pub teardown_timeout: std::time::Duration,
    /// 显式指定默认使用的 CNI 网络名；为空时按文件名字典序选择第一个。
    #[serde(alias = "cni_default_network")]
    pub default_network_name: Option<String>,
    /// 是否全局禁用 hostPort 映射。
    pub disable_hostport_mapping: bool,
    /// 是否将 netns 挂载统一放到 runtime state dir 下。
    #[serde(alias = "netns_mounts_under_state_dir")]
    pub netns_mounts_under_state_dir: bool,
    /// `crs` 本地 Pod 使用的网络域。
    pub local: NetworkDomainConfig,
    /// kubelet / CRI 调用使用的网络域。
    pub cri: NetworkDomainConfig,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        let local = NetworkDomainConfig::local_default();
        let cri = NetworkDomainConfig::cri_default();
        Self {
            plugin: "cni".to_string(),
            config_dirs: cri.config_dirs.clone(),
            plugin_dirs: cri.plugin_dirs.clone(),
            cache_dir: cri.cache_dir.clone(),
            conf_template: cri.conf_template.clone(),
            max_conf_num: cri.max_conf_num,
            ip_pref: cri.ip_pref,
            teardown_timeout: cri.teardown_timeout,
            default_network_name: cri.default_network_name.clone(),
            disable_hostport_mapping: cri.disable_hostport_mapping,
            netns_mounts_under_state_dir: cri.netns_mounts_under_state_dir,
            local,
            cri,
        }
    }
}

impl NetworkConfig {
    pub fn cni_config(&self) -> CniConfig {
        let mut cni = CniConfig::new(
            self.config_dirs.iter().map(PathBuf::from).collect(),
            self.plugin_dirs.iter().map(PathBuf::from).collect(),
            PathBuf::from(&self.cache_dir),
            self.max_conf_num,
            self.ip_pref,
            self.default_network_name.clone(),
            self.disable_hostport_mapping,
        );
        cni.set_teardown_timeout(self.teardown_timeout);
        if !self.conf_template.trim().is_empty() {
            cni.set_conf_template(Some(PathBuf::from(self.conf_template.trim())));
        }
        cni.set_netns_mounts_under_state_dir(self.netns_mounts_under_state_dir);
        cni
    }

    pub fn local_cni_config(&self) -> CniConfig {
        self.local.cni_config()
    }
}

fn apply_string_override(env_name: &str, target: &mut String) {
    if let Some(value) = std::env::var_os(env_name) {
        *target = value.to_string_lossy().trim().to_string();
    }
}

fn apply_csv_override(env_name: &str, target: &mut Vec<String>) {
    if let Some(value) = std::env::var_os(env_name) {
        *target = split_csv_list(&value.to_string_lossy());
    }
}

fn apply_bool_override(env_name: &str, target: &mut bool) -> Result<()> {
    if let Some(value) = std::env::var_os(env_name) {
        *target = parse_bool(&value.to_string_lossy())
            .map_err(|err| Error::Config(format!("{env_name}: {err}")))?;
    }
    Ok(())
}

fn apply_optional_string_override(env_name: &str, target: &mut Option<String>) {
    if let Some(value) = std::env::var_os(env_name) {
        let trimmed = value.to_string_lossy().trim().to_string();
        *target = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
}

fn apply_numeric_override<T>(env_name: &str, target: &mut T) -> Result<()> 
where 
    T: FromStr,
    T::Err: std::fmt::Display,
{
    if let Some(value) = std::env::var_os(env_name) {
        *target = value.to_string_lossy().trim().parse::<T>().map_err(|err|Error::Config(format!(
            "{env_name}: invalid {} value: {err}",
            std::any::type_name::<T>()
        )))?;
    }

    Ok(())
}

fn split_csv_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_bool(raw: &str) -> std::result::Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(format!("invalid boolean value {other}")),
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<StringOrVec>::deserialize(deserializer)?;
    Ok(match value {
        Some(StringOrVec::String(value)) => vec![value],
        Some(StringOrVec::Vec(values)) => values,
        None => Vec::new(),
    })
}