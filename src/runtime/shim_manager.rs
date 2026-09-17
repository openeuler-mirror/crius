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


use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::fs;

use tokio::sync::Mutex;
use log::debug;
use serde::{Serialize, Deserialize};
use anyhow::Result;

use crate::storage::StorageManager;
use crate::service::event::{
    InternalEvent, InternalEventSeverity,
    LedgerInternalEventSink,
};
use crate::defaults::{
    DEFAULT_SHIM_WORK_DIR, SHIM_METADATA_FILE,
    SHIM_PIDFILE_NAME,
};

/// Shim配置
#[derive(Debug, Clone)]
pub struct ShimConfig {
    /// Shim二进制路径
    pub shim_path: PathBuf,
    /// OCI runtime 特定配置文件路径。
    pub runtime_config_path: PathBuf,
    /// monitor/shim 所在 cgroup；支持空字符串、`pod` 或 systemd slice。
    pub monitor_cgroup: String,
    /// Shim工作目录
    pub work_dir: PathBuf,
    /// attach/resize socket 根目录
    pub attach_socket_dir: PathBuf,
    /// 容器退出记录根目录
    pub container_exits_dir: PathBuf,
    /// shim 创建的宿主 IO 工件默认 UID。
    pub io_uid: u32,
    /// shim 创建的宿主 IO 工件默认 GID。
    pub io_gid: u32,
    /// 传给 shim 进程的环境变量列表，格式为 `KEY=value`。
    pub monitor_env: Vec<String>,
    /// 是否启用debug模式
    pub debug: bool,
    /// 是否将容器输出双写到 journald。
    pub log_to_journald: bool,
    /// 是否在日志轮转和容器退出时跳过 sync。
    pub no_sync_log: bool,
    /// 是否禁用 pivot_root，改用 MS_MOVE。
    pub no_pivot: bool,
    /// 是否禁止创建新的 session keyring。
    pub no_new_keyring: bool,
    /// 是否让运行时使用 systemd cgroup 模式。
    pub systemd_cgroup: bool,
    /// 运行时路径(runc)
    pub runtime_path: PathBuf,
    /// CRI 单条日志记录切分阈值（字节）。
    pub max_container_log_line_size: usize,
    /// 状态账本数据库路径。
    pub state_db_path: PathBuf,
}

impl Default for ShimConfig {
    fn default() -> Self {
        Self {
            shim_path: PathBuf::from("crius-shim"),
            runtime_config_path: PathBuf::new(),
            monitor_cgroup: String::new(),
            work_dir: default_shim_work_dir(),
            attach_socket_dir: default_attach_socket_dir(),
            container_exits_dir: PathBuf::from("/var/run/crius/exits"),
            io_uid: 0,
            io_gid: 0,
            monitor_env: Vec::new(),
            debug: false,
            log_to_journald: false,
            no_sync_log: false,
            no_pivot: false,
            no_new_keyring: false,
            systemd_cgroup: false,
            runtime_path: PathBuf::from("runc"),
            max_container_log_line_size: 4096,
            state_db_path: PathBuf::new(),
        }
    }
}

/// Shim进程信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShimProcess {
    /// 容器ID
    pub container_id: String,
    /// Shim进程ID
    pub shim_pid: u32,
    /// 退出码文件路径
    pub exit_code_file: PathBuf,
    /// 日志文件路径
    pub log_file: PathBuf,
    /// Unix socket路径（用于attach）
    pub socket_path: PathBuf,
    /// Bundle目录
    pub bundle_path: PathBuf,
}

/// Shim管理器
#[derive(Debug)]
pub struct ShimManager {
    config: ShimConfig,
    /// 正在运行的shim进程
    processes: Arc<Mutex<Vec<ShimProcess>>>,
}

impl ShimManager {
    /// 创建新的ShimManager
    pub fn new(config: ShimConfig) -> Self {
        // 确保工作目录存在
        let _ = fs::create_dir_all(&config.work_dir);
        let restored = Self::restore_processes_from_disk(&config);
        for process in &restored {
            let _ = Self::persist_pidfile_for(&config, &process.container_id, process.shim_pid);
        }

        Self {
            config,
            processes: Arc::new(Mutex::new(restored)),
        }
    }

    fn ledger_enabled(config: &ShimConfig) -> bool {
        !config.state_db_path.as_os_str().is_empty()
    }

    fn process_exists(pid: u32) -> bool {
        PathBuf::from("/proc").join(pid.to_string()).exists()
    }

    fn metadata_path_for(config: &ShimConfig, container_id: &str) -> PathBuf {
        config.work_dir.join(container_id).join(SHIM_METADATA_FILE)
    }

    fn pidfile_path_for(config: &ShimConfig, container_id: &str) -> PathBuf {
        config.work_dir.join(container_id).join(SHIM_PIDFILE_NAME)
    }

    fn restore_processes_from_disk(config: &ShimConfig) -> Vec<ShimProcess> {
        if Self::ledger_enabled(config) {
            if let Ok(mut storage) = StorageManager::new(&config.state_db_path) {
                if let Ok(records) = storage.list_shim_processes() {
                    let restored = records
                        .into_iter()
                        .filter_map(|mut record| {
                            let live_or_exited = Self::process_exists(record.shim_pid)
                                || Path::new(&record.exit_code_file).exists();
                            if !live_or_exited {
                                return None;
                            }

                            let metadata_path =
                                Self::metadata_path_for(config, &record.container_id);
                            let pidfile_path = Self::pidfile_path_for(config, &record.container_id);
                            if !metadata_path.exists() || !pidfile_path.exists() {
                                let previous_state = record.state.clone();
                                record.state = "degraded".to_string();
                                record.last_seen_at = chrono::Utc::now().timestamp();
                                let _ = storage.save_shim_process(&record);
                                let details = format!(
                                    "diagnostic shim files missing: metadata={}, pidfile={}",
                                    metadata_path.exists(),
                                    pidfile_path.exists()
                                );
                                let event = InternalEvent::new(
                                    "shim.degraded",
                                    "shim",
                                    &record.container_id,
                                    InternalEventSeverity::Warning,
                                    serde_json::json!({
                                        "previousState": previous_state,
                                        "state": "degraded",
                                        "details": details,
                                        "metadataExists": metadata_path.exists(),
                                        "pidfileExists": pidfile_path.exists(),
                                    }),
                                );
                                let _ = LedgerInternalEventSink::new(&config.state_db_path)
                                    .publish(&event);
                            }

                            Some(ShimProcess {
                                container_id: record.container_id,
                                shim_pid: record.shim_pid,
                                exit_code_file: PathBuf::from(record.exit_code_file),
                                log_file: PathBuf::from(record.log_file),
                                socket_path: PathBuf::from(record.socket_path),
                                bundle_path: PathBuf::from(record.bundle_path),
                            })
                        })
                        .collect::<Vec<_>>();
                    if !restored.is_empty() {
                        return restored;
                    }
                }
            }
            return Vec::new();
        }
        let mut restored = Vec::new();
        let Ok(entries) = fs::read_dir(&config.work_dir) else {
            return restored;
        };

        for entry in entries.flatten() {
            let metadata_path = entry.path().join(SHIM_METADATA_FILE);
            if !metadata_path.exists() {
                continue;
            }

            let raw = match fs::read(&metadata_path) {
                Ok(raw) => raw,
                Err(err) => {
                    debug!(
                        "Ignoring unreadable shim metadata {}: {}",
                        metadata_path.display(),
                        err
                    );
                    continue;
                }
            };
            let process: ShimProcess = match serde_json::from_slice(&raw) {
                Ok(process) => process,
                Err(err) => {
                    debug!(
                        "Ignoring invalid shim metadata {}: {}",
                        metadata_path.display(),
                        err
                    );
                    continue;
                }
            };

            if Self::process_exists(process.shim_pid) || process.exit_code_file.exists() {
                restored.push(process);
            } else {
                debug!(
                    "Ignoring stale shim metadata for container {} from {}",
                    process.container_id,
                    metadata_path.display()
                );
            }
        }

        restored
    }

    fn persist_pidfile_for(config: &ShimConfig, container_id: &str, shim_pid: u32) -> Result<()> {
        let pidfile_path = Self::pidfile_path_for(config, container_id);
        if let Some(parent) = pidfile_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(pidfile_path, format!("{shim_pid}\n"))?;
        Ok(())
    }

}

pub fn default_shim_work_dir() -> PathBuf {
    std::env::var("CRIUS_SHIM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SHIM_WORK_DIR))
}

pub fn default_attach_socket_dir() -> PathBuf {
    std::env::var("CRIUS_ATTACH_SOCKET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/run/crius/attach"))
}