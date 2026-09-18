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


pub mod types;
pub mod error;
pub mod cni;

use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand};

use nix::mount::{
    mount, umount2,
    MntFlags, MsFlags
};
use nix::sched::{
    unshare, CloneFlags
};
use nix::unistd::gettid;

use crate::network::types::{MainIpPreference, NetworkStatus};
use crate::network::error::NetworkError;
use crate::network::cni::CniManager;
use crate::service::event::LedgerInternalEventSink;

use async_trait::async_trait;

/// 共享的 CNI 路径配置。
#[derive(Debug, Clone)]
pub struct CniConfig {
    config_dirs: Vec<PathBuf>,
    plugin_dirs: Vec<PathBuf>,
    cache_dir: PathBuf,
    conf_template: Option<PathBuf>,
    max_conf_num: usize,
    ip_pref: MainIpPreference,
    teardown_timeout: std::time::Duration,
    runtime_handler_config_dirs: std::collections::HashMap<String, Vec<PathBuf>>,
    runtime_handler_max_conf_nums: std::collections::HashMap<String, usize>,
    default_network_name: Option<String>,
    disable_hostport_mapping: bool,
    netns_mount_dir: PathBuf,
    netns_mounts_under_state_dir: bool,
    namespace_helper_path: Option<PathBuf>,
    // rootless: Option<RootlessNetworkConfig>,
    event_sink: Option<LedgerInternalEventSink>,
}

impl CniConfig {
    pub fn new(
        config_dirs: Vec<PathBuf>,
        plugin_dirs: Vec<PathBuf>,
        cache_dir: PathBuf,
        max_conf_num: usize,
        ip_pref: MainIpPreference,
        default_network_name: Option<String>,
        disable_hostport_mapping: bool,
    ) -> Self {
        Self {
            config_dirs,
            plugin_dirs,
            cache_dir,
            conf_template: None,
            max_conf_num,
            ip_pref,
            teardown_timeout: std::time::Duration::from_secs(60),
            runtime_handler_config_dirs: std::collections::HashMap::new(),
            runtime_handler_max_conf_nums: std::collections::HashMap::new(),
            default_network_name: default_network_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            disable_hostport_mapping,
            netns_mount_dir: PathBuf::from("/var/run/netns"),
            netns_mounts_under_state_dir: false,
            namespace_helper_path: None,
            event_sink: None,
        }
    }

    pub fn plugin_dirs(&self) -> &[PathBuf] {
        &self.plugin_dirs
    }

    pub fn config_dirs(&self) -> &[PathBuf] {
        &self.config_dirs
    }

    pub fn set_config_dirs(&mut self, config_dirs: Vec<PathBuf>) {
        self.config_dirs = config_dirs;
    }

    pub fn set_plugin_dirs(&mut self, plugin_dirs: Vec<PathBuf>) {
        self.plugin_dirs = plugin_dirs;
    }

    pub fn set_max_conf_num(&mut self, max_conf_num: usize) {
        self.max_conf_num = max_conf_num;
    }

    pub fn set_default_network_name(&mut self, default_network_name: Option<String>) {
        self.default_network_name = default_network_name;
    }

    pub fn set_conf_template(&mut self, conf_template: Option<PathBuf>) {
        self.conf_template = conf_template.filter(|path| !path.as_os_str().is_empty());
    }

    pub fn set_event_sink(&mut self, event_sink: Option<crate::service::event::LedgerInternalEventSink>) {
        self.event_sink = event_sink;
    }

    fn config_dir_strings(&self) -> Vec<String> {
        self.config_dirs
            .iter()
            .map(|dir| dir.to_string_lossy().to_string())
            .collect()
    }

    fn plugin_dir_strings(&self) -> Vec<String> {
        self.plugin_dirs
            .iter()
            .map(|dir| dir.to_string_lossy().to_string())
            .collect()
    }

    fn cache_dir_string(&self) -> String {
        self.cache_dir.to_string_lossy().to_string()
    }

    pub fn max_conf_num(&self) -> usize {
        self.max_conf_num
    }

    pub fn ip_pref(&self) -> MainIpPreference {
        self.ip_pref
    }

    pub fn teardown_timeout(&self) -> std::time::Duration {
        self.teardown_timeout
    }

    pub fn default_network_name(&self) -> Option<&str> {
        self.default_network_name.as_deref()
    }

    pub fn namespace_manager(&self) -> NamespaceManager {
        NamespaceManager::with_helper(
            self.netns_mount_dir.clone(),
            self.namespace_helper_path.clone(),
        )
    }

    pub fn set_teardown_timeout(&mut self, teardown_timeout: std::time::Duration) {
        self.teardown_timeout = teardown_timeout;
    }
    
    pub fn set_netns_mounts_under_state_dir(&mut self, enabled: bool) {
        self.netns_mounts_under_state_dir = enabled;
    }

    pub fn netns_path(&self, ns_name_or_path: &str) -> PathBuf {
        NamespaceManager::new(self.netns_mount_dir.clone()).resolve_path(ns_name_or_path)
    }

    pub fn conf_template(&self) -> Option<&Path> {
        self.conf_template.as_deref()
    }
}

/// 网络管理器接口
#[derive(Debug, Clone, Copy)]
pub struct NetworkSetupRequest<'a> {
    pub pod_id: &'a str,
    pub netns: &'a str,
    pub pod_name: &'a str,
    pub pod_namespace: &'a str,
    pub pod_uid: &'a str,
    pub runtime_handler: &'a str,
    pub pod_cidr: Option<&'a str>,
}

#[async_trait]
pub trait NetworkManager: Send + Sync + 'static {
    /// 初始化网络
    async fn init(&self) -> Result<(), NetworkError>;

    /// 创建网络命名空间
    async fn create_network_namespace(&self, ns_path: &str) -> Result<(), NetworkError>;

    /// 删除网络命名空间
    async fn remove_network_namespace(&self, ns_path: &str) -> Result<(), NetworkError>;

    /// 设置 Pod 网络
    async fn setup_pod_network(&self, request: NetworkSetupRequest<'_>,) -> Result<NetworkStatus, NetworkError>;

    /// 清理 Pod 网络
    async fn teardown_pod_network(
        &self,
        pod_id: &str,
        netns: &str,
        pod_namespace: &str,
        pod_name: &str,
        pod_uid: &str,
        runtime_handler: &str,
    ) -> Result<(), NetworkError>;
}

/// 默认网络管理器实现
#[derive(Debug)]
pub struct DefaultNetworkManager {
    cni_plugin_dirs: Vec<String>,
    cni_config_dirs: Vec<String>,
    cni_cache_dir: String,
    cni_max_conf_num: usize,
    cni_ip_pref: MainIpPreference,
    cni_teardown_timeout: std::time::Duration,
    cni_runtime_handler_config_dirs: std::collections::HashMap<String, Vec<String>>,
    cni_runtime_handler_max_conf_nums: std::collections::HashMap<String, usize>,
    cni_default_network_name: Option<String>,
    namespace_manager: NamespaceManager,
    // rootless: Option<RootlessNetworkConfig>,
    // rootless_processes: std::sync::Arc<std::sync::Mutex<HashMap<String, Child>>>,
    event_sink: Option<crate::service::event::LedgerInternalEventSink>,
}

impl DefaultNetworkManager {
    pub fn from_cni_config(cni: CniConfig) -> Self {
        Self {
            cni_plugin_dirs: cni.plugin_dir_strings(),
            cni_config_dirs: cni.config_dir_strings(),
            cni_cache_dir: cni.cache_dir_string(),
            cni_max_conf_num: cni.max_conf_num(),
            cni_ip_pref: cni.ip_pref(),
            cni_teardown_timeout: cni.teardown_timeout(),
            cni_runtime_handler_config_dirs: cni
                .runtime_handler_config_dirs
                .iter()
                .map(|(handler, dirs)| {
                    (
                        handler.clone(),
                        dirs.iter()
                            .map(|dir| dir.to_string_lossy().to_string())
                            .collect(),
                    )
                })
                .collect(),
            cni_runtime_handler_max_conf_nums: cni.runtime_handler_max_conf_nums.clone(),
            cni_default_network_name: cni.default_network_name().map(ToOwned::to_owned),
            namespace_manager: cni.namespace_manager(),
            // rootless: cni.rootless.clone(),
            // rootless_processes: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            event_sink: cni.event_sink.clone(),
        }
    }

    fn effective_cni_config_dirs(&self, runtime_handler: &str) -> Vec<String> {
        self.cni_runtime_handler_config_dirs
            .get(runtime_handler)
            .filter(|dirs| !dirs.is_empty())
            .cloned()
            .unwrap_or_else(|| self.cni_config_dirs.clone())
    }

    fn effective_cni_max_conf_num(&self, runtime_handler: &str) -> usize {
        self.cni_runtime_handler_max_conf_nums
            .get(runtime_handler)
            .copied()
            .unwrap_or(self.cni_max_conf_num)
    }
}

#[async_trait]
impl NetworkManager for DefaultNetworkManager {
    async fn init(&self) -> Result<(), NetworkError> {
        // 创建缓存目录
        if !Path::new(&self.cni_cache_dir).exists() {
            tokio::fs::create_dir_all(&self.cni_cache_dir).await?;
        }
        tokio::fs::create_dir_all(self.namespace_manager.mount_dir()).await?;
        Ok(())
    }

    async fn create_network_namespace(&self, ns_path: &str) -> Result<(), NetworkError> {
        self.namespace_manager.create(ns_path).await?;
        Ok(())
    }

    async fn remove_network_namespace(&self, ns_path: &str) -> Result<(), NetworkError> {
        self.namespace_manager.remove(ns_path).await?;
        Ok(())
    }

    async fn setup_pod_network(
        &self,
        request: NetworkSetupRequest<'_>,
    ) -> Result<NetworkStatus, NetworkError> {
        let mut cni = CniManager::new(
            self.cni_plugin_dirs.clone(),
            self.effective_cni_config_dirs(request.runtime_handler),
            self.cni_cache_dir.clone(),
        )
        .map_err(|e| NetworkError::Other(e.to_string()))?;
        cni.set_event_sink(self.event_sink.clone());
        cni.set_max_conf_num(self.effective_cni_max_conf_num(request.runtime_handler));
        cni.set_ip_pref(self.cni_ip_pref);
        cni.set_default_network_name(self.cni_default_network_name.clone());
        cni.set_teardown_timeout(self.cni_teardown_timeout);
        let load_status = cni
            .load_network_configs()
            .await
            .map_err(|e| NetworkError::Other(e.to_string()))?;
        cni.publish_config_load_event(request.pod_id, request.runtime_handler, &load_status);

        cni.setup_pod_network(
            request.pod_id,
            request.netns,
            request.pod_name,
            request.pod_namespace,
            request.pod_uid,
            request.pod_cidr,
        )
        .await
        .map_err(|e| NetworkError::Other(e.to_string()))
    }

    async fn teardown_pod_network(
        &self,
        pod_id: &str,
        netns: &str,
        pod_namespace: &str,
        pod_name: &str,
        pod_uid: &str,
        runtime_handler: &str,
    ) -> Result<(), NetworkError> {
        let mut cni = CniManager::new(
            self.cni_plugin_dirs.clone(),
            self.effective_cni_config_dirs(runtime_handler),
            self.cni_cache_dir.clone(),
        )
        .map_err(|e| NetworkError::Other(e.to_string()))?;
        cni.set_event_sink(self.event_sink.clone());
        cni.set_max_conf_num(self.effective_cni_max_conf_num(runtime_handler));
        cni.set_ip_pref(self.cni_ip_pref);
        cni.set_default_network_name(self.cni_default_network_name.clone());
        cni.set_teardown_timeout(self.cni_teardown_timeout);
        let load_status = cni
            .load_network_configs()
            .await
            .map_err(|e| NetworkError::Other(e.to_string()))?;
        cni.publish_config_load_event(pod_id, runtime_handler, &load_status);
        cni.teardown_pod_network(pod_id, netns, pod_namespace, pod_name, pod_uid)
            .await
            .map_err(|e| NetworkError::Other(e.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceManager {
    mount_dir: PathBuf,
    helper_path: Option<PathBuf>,
}

impl NamespaceManager {
    pub fn new(mount_dir: PathBuf) -> Self {
        Self::with_helper(mount_dir, None::<PathBuf>)
    }

    pub fn with_helper(mount_dir: PathBuf, helper_path: Option<impl Into<PathBuf>>) -> Self {
        Self {
            mount_dir,
            helper_path: helper_path.map(Into::into),
        }
    }

    pub fn resolve_path(&self, ns_name_or_path: &str) -> PathBuf {
        let candidate = Path::new(ns_name_or_path);
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.mount_dir.join(candidate)
        }
    }

    pub fn mount_dir(&self) -> &Path {
        &self.mount_dir
    }

    pub async fn create(&self, ns_name_or_path: &str) -> Result<PathBuf, NetworkError> {
        let manager = self.clone();
        let input = ns_name_or_path.to_string();
        tokio::task::spawn_blocking(move || manager.create_blocking(&input))
            .await
            .map_err(|err| {
                NetworkError::Other(format!("create network namespace task failed: {err}"))
            })?
    }

    fn create_blocking(&self, ns_name_or_path: &str) -> Result<PathBuf, NetworkError> {
        let path = self.resolve_path(ns_name_or_path);
        if path.exists() {
            return Ok(path);
        }

        if self.helper_path.is_some() && path.parent() == Some(self.mount_dir.as_path()) {
            self.create_with_helper(&path)?;
            return Ok(path);
        }

        self.create_internal(&path)?;
        Ok(path)
    }

    fn create_with_helper(&self, path: &Path) -> Result<(), NetworkError> {
        let helper = self.helper_path.as_ref().ok_or_else(|| {
            NetworkError::Other("namespace helper path is not configured".to_string())
        })?;
        let ns_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                NetworkError::Other(format!(
                    "failed to derive namespace name for helper-managed path {}",
                    path.display()
                ))
            })?;
        let helper_base_dir = self.mount_dir.parent().ok_or_else(|| {
            NetworkError::Other(format!(
                "failed to derive helper base directory from mount dir {}",
                self.mount_dir.display()
            ))
        })?;
        std::fs::create_dir_all(helper_base_dir)?;
        std::fs::create_dir_all(&self.mount_dir)?;

        let output = StdCommand::new(helper)
            .arg("-d")
            .arg(helper_base_dir)
            .arg("-f")
            .arg(ns_name)
            .arg("--net")
            .output()?;

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
            return Err(NetworkError::Other(format!(
                "namespace helper {} failed for {}: {}",
                helper.display(),
                path.display(),
                detail
            )));
        }

        if !path.exists() {
            return Err(NetworkError::Other(format!(
                "namespace helper {} completed without creating {}",
                helper.display(),
                path.display()
            )));
        }

        Ok(())
    }

    fn create_internal(&self, path: &Path) -> Result<(), NetworkError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _mountpoint = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;

        let mount_path = path.to_path_buf();
        std::thread::spawn(move || -> Result<(), NetworkError> {
            unshare(CloneFlags::CLONE_NEWNET).map_err(|err| {
                NetworkError::Other(format!("failed to unshare network namespace: {err}"))
            })?;
            mount(
                Some(current_thread_netns_path().as_str()),
                mount_path.as_path(),
                None::<&str>,
                MsFlags::MS_BIND,
                None::<&str>,
            )
            .map_err(|err| {
                NetworkError::Other(format!(
                    "failed to bind-mount network namespace at {}: {err}",
                    mount_path.display()
                ))
            })?;
            Ok(())
        })
        .join()
        .map_err(|_| {
            NetworkError::Other("network namespace creator thread panicked".to_string())
        })??;

        Ok(())
    }

    pub async fn remove(&self, ns_name_or_path: &str) -> Result<(), NetworkError> {
        let manager = self.clone();
        let input = ns_name_or_path.to_string();
        tokio::task::spawn_blocking(move || manager.remove_blocking(&input))
            .await
            .map_err(|err| {
                NetworkError::Other(format!("remove network namespace task failed: {err}"))
            })?
    }

    fn remove_blocking(&self, ns_name_or_path: &str) -> Result<(), NetworkError> {
        let path = self.resolve_path(ns_name_or_path);
        if !path.exists() {
            return Ok(());
        }

        match umount2(path.as_path(), MntFlags::MNT_DETACH) {
            Ok(()) => {}
            Err(nix::errno::Errno::EINVAL | nix::errno::Errno::ENOENT) => {}
            Err(err) => {
                return Err(NetworkError::Other(format!(
                    "failed to unmount network namespace {}: {err}",
                    path.display()
                )));
            }
        }

        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(NetworkError::Io(err)),
        }
    }
}

fn current_thread_netns_path() -> String {
    format!("/proc/{}/task/{}/ns/net", std::process::id(), gettid())
}