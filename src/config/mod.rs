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

use std::{fs, path::Path};

use serde::Deserialize;

use crate::error::Result;
use crate::defaults::*;

/// 守护进程主配置。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// 持久化根目录。
    pub root: String,

    /// API 配置。
    pub api: ApiConfig,

    // 运行时配置。
    // pub runtime: RuntimeConfig,

    /// 镜像配置。
    pub image: ImageConfig,

    // 网络配置。
    // pub network: NetworkConfig,

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


impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
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