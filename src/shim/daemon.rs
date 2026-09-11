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


//! Shim守护进程实现
//!
//! 守护进程负责：
//! 1. 设置子进程收割（PR_SET_CHILD_SUBREAPER）
//! 2. 创建容器进程（通过runc create）
//! 3. 监控容器进程生命周期
//! 4. 记录容器退出码
//! 5. 管理IO流

use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use nix::cmsg_space;
use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::fs::File;
use std::io::IoSliceMut;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::io::{IoConfig, IoManager, JournalConfig, DEFAULT_JOURNALD_SOCKET_PATH};
use crate::image::snapshotter::{RootfsHandle, RootfsHandleKind, RootfsMountSpec};
use crate::runtime::RuncRuntime;
use crate::services::{InternalEvent, InternalEventSeverity, LedgerInternalEventSink};
use crate::shim_rpc::server::{default_task_socket_path, serve, ShimRpcHandler};
use crate::shim_rpc::{
    CheckpointTaskRequest, CreateTaskRequest, DeleteTaskRequest, ExecProcessRequest,
    ExecProcessResponse, KillTaskRequest, OpenAttachStreamRequest, OpenAttachStreamResponse,
    OpenExecSessionRequest, OpenExecSessionResponse, PauseTaskRequest, ReopenLogRequest,
    ResizePtyRequest, RestoreTaskRequest, ResumeTaskRequest, ShimRpcRequest, ShimRpcResponse,
    StartTaskRequest, StatusRequest, StatusResponse, TaskState, UpdateResourcesRequest,
    WaitProcessRequest, WaitProcessResponse,
};
use crate::storage::StorageManager;

const INTERNAL_CONTAINER_STATE_KEY: &str = "io.crius.internal/container-state";

#[derive(Debug, Deserialize, Default)]
struct ShimBundleProcess {
    terminal: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ShimBundleConfig {
    process: Option<ShimBundleProcess>,
    linux: Option<ShimBundleLinux>,
    annotations: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Default)]
struct ShimBundleLinux {
    #[serde(rename = "cgroupsPath")]
    cgroups_path: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ShimStoredContainerState {
    log_path: Option<String>,
    metadata_name: Option<String>,
    tty: bool,
    stdin: bool,
    stdin_once: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonTaskState {
    Init,
    Created,
    Running,
    Paused,
    Stopped,
    Deleted,
}

impl DaemonTaskState {
    fn as_rpc_state(self) -> TaskState {
        match self {
            Self::Init => TaskState::Init,
            Self::Created => TaskState::Created,
            Self::Running => TaskState::Running,
            Self::Paused => TaskState::Paused,
            Self::Stopped => TaskState::Stopped,
            Self::Deleted => TaskState::Deleted,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Created => "created",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
            Self::Deleted => "deleted",
        }
    }
}

/// Shim守护进程
#[derive(Clone)]
pub struct Daemon {
    /// 容器ID
    container_id: String,
    /// Bundle目录
    bundle: PathBuf,
    /// Runtime路径
    runtime: PathBuf,
    /// OCI runtime 特定配置文件路径。
    runtime_config_path: PathBuf,
    /// monitor/shim 所在 cgroup。
    monitor_cgroup: String,
    /// 退出码文件路径
    exit_code_file: Option<PathBuf>,
    /// attach/resize socket 根目录
    attach_socket_dir: Option<PathBuf>,
    /// 是否禁用 pivot_root，改用 MS_MOVE。
    no_pivot: bool,
    /// 是否禁止创建新的 session keyring。
    no_new_keyring: bool,
    /// runtime 是否启用 systemd cgroup。
    systemd_cgroup: bool,
    /// shim 工作目录根路径。
    work_dir: PathBuf,
    /// 统一账本路径。
    state_db_path: Option<PathBuf>,
    /// 是否正在运行
    running: Arc<AtomicBool>,
    /// task 生命周期状态。
    task_state: Arc<Mutex<DaemonTaskState>>,
    /// 最近已知的容器 PID。
    container_pid: Arc<Mutex<Option<i32>>>,
    /// 最近已知的退出码。
    exit_code: Arc<Mutex<Option<i32>>>,
    /// task 后台线程。
    task_thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

pub struct DaemonOptions {
    pub runtime_config_path: PathBuf,
    pub monitor_cgroup: String,
    pub work_dir: PathBuf,
    pub state_db_path: Option<PathBuf>,
    pub exit_code_file: Option<PathBuf>,
    pub attach_socket_dir: Option<PathBuf>,
    pub io_uid: u32,
    pub io_gid: u32,
    pub max_container_log_line_size: usize,
    pub log_to_journald: bool,
    pub no_sync_log: bool,
    pub no_pivot: bool,
    pub no_new_keyring: bool,
    pub systemd_cgroup: bool,
}

impl Daemon {
    /// 创建新的守护进程
    pub fn new(
        container_id: String,
        bundle: PathBuf,
        runtime: PathBuf,
        options: DaemonOptions,
    ) -> Self {
        let DaemonOptions {
            runtime_config_path,
            monitor_cgroup,
            work_dir,
            state_db_path,
            exit_code_file,
            attach_socket_dir,
            no_pivot,
            no_new_keyring,
            systemd_cgroup,
            ..
        } = options;
        Self {
            container_id,
            bundle,
            runtime,
            runtime_config_path,
            monitor_cgroup,
            work_dir,
            state_db_path,
            exit_code_file,
            attach_socket_dir,
            no_pivot,
            no_new_keyring,
            systemd_cgroup,
            running: Arc::new(AtomicBool::new(true)),
            task_state: Arc::new(Mutex::new(DaemonTaskState::Init)),
            container_pid: Arc::new(Mutex::new(None)),
            exit_code: Arc::new(Mutex::new(None)),
            task_thread: Arc::new(Mutex::new(None)),
        }
    }


    /// 运行守护进程
    pub fn run(self) -> Result<()> {
        // 1. 设置子进程收割者
        self.setup_subreaper()?;

        // 2. 设置信号处理器
        self.setup_signal_handlers()?;

        // 3. 把 shim/monitor 进程放到目标 cgroup。
        self.configure_monitor_cgroup()?;

        // 4. 启动 RPC task service。
        let socket_path = self.task_socket_path();
        info!(
            "Shim daemon ready for container {} on {}",
            self.container_id,
            socket_path.display()
        );
        serve(&socket_path, self.running.clone(), Arc::new(self.clone()))
    }

    fn task_socket_path(&self) -> PathBuf {
        default_task_socket_path(&self.work_dir, &self.container_id)
    }

    fn set_task_state(&self, next: DaemonTaskState) {
        let previous = {
            let mut guard = self.task_state.lock().unwrap();
            let previous = *guard;
            *guard = next;
            previous
        };
        if previous != next {
            let _ = self.record_task_event(previous, next, None);
        }
    }

    fn record_task_event(
        &self,
        previous: DaemonTaskState,
        next: DaemonTaskState,
        details: Option<String>,
    ) -> Result<()> {
        let Some(path) = self.state_db_path.as_ref() else {
            return Ok(());
        };
        let mut event_details = serde_json::json!({
            "previousState": previous.as_str(),
            "state": next.as_str(),
        });
        if let Some(details) = details {
            event_details["details"] = serde_json::Value::String(details);
        }
        let event = InternalEvent::new(
            "task.state",
            "task",
            &self.container_id,
            InternalEventSeverity::Info,
            event_details,
        );
        LedgerInternalEventSink::new(path).publish(&event)
    }

    fn task_status(&self) -> StatusResponse {
        let state = *self.task_state.lock().unwrap();
        let pid = *self.container_pid.lock().unwrap();
        let exit_code = *self.exit_code.lock().unwrap();
        StatusResponse {
            state: state.as_rpc_state(),
            pid,
            exit_code,
        }
    }
    fn spawn_task_runner(&self) -> Result<()> {
        let state = *self.task_state.lock().unwrap();
        match state {
            DaemonTaskState::Running | DaemonTaskState::Paused => return Ok(()),
            DaemonTaskState::Deleted => {
                return Err(anyhow::anyhow!(
                    "cannot start deleted task {}",
                    self.container_id
                ))
            }
            DaemonTaskState::Stopped => {
                return Err(anyhow::anyhow!(
                    "cannot restart stopped task {}",
                    self.container_id
                ))
            }
            DaemonTaskState::Init => {
                return Err(anyhow::anyhow!(
                    "task {} has not been created",
                    self.container_id
                ))
            }
            DaemonTaskState::Created => {}
        }

        let daemon = self.clone();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let handle = std::thread::spawn(move || {
            daemon.set_task_state(DaemonTaskState::Running);
            if daemon.is_terminal().unwrap_or(false) {
                let _ = started_tx.send(Err(
                    "TTY containers are not supported by this shim milestone".to_string()
                ));
                daemon.set_task_state(DaemonTaskState::Stopped);
                let _ = daemon.record_exit_code(1);
                return;
            }
            let _ = started_tx.send(Ok(()));
            match daemon.run_non_terminal_container() {
                Ok(exit_code) => {
                    *daemon.exit_code.lock().unwrap() = Some(exit_code);
                    *daemon.container_pid.lock().unwrap() = None;
                    daemon.set_task_state(DaemonTaskState::Stopped);
                    if let Err(err) = daemon.record_exit_code(exit_code) {
                        warn!(
                            "Failed to persist shim exit code for {}: {}",
                            daemon.container_id, err
                        );
                    }
                }
                Err(err) => {
                    error!("Task runner for {} failed: {}", daemon.container_id, err);
                    *daemon.exit_code.lock().unwrap() = Some(1);
                    *daemon.container_pid.lock().unwrap() = None;
                    daemon.set_task_state(DaemonTaskState::Stopped);
                    let _ = daemon.record_exit_code(1);
                }
            }
        });

        let mut guard = self.task_thread.lock().unwrap();
        *guard = Some(handle);
        started_rx
            .recv()
            .map_err(|err| anyhow::anyhow!("task runner exited before start confirmation: {err}"))?
            .map_err(|err| anyhow::anyhow!("task start failed: {err}"))?;
        Ok(())
    }


    /// 设置子进程收割者
    fn setup_subreaper(&self) -> Result<()> {
        // 使用libc直接调用prctl设置子进程收割者
        // PR_SET_CHILD_SUBREAPER = 36
        const PR_SET_CHILD_SUBREAPER: i32 = 36;
        let result = unsafe { libc::prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };

        if result != 0 {
            return Err(anyhow::anyhow!(
                "Failed to set subreaper: {}",
                std::io::Error::last_os_error()
            ));
        }

        info!("Set as child subreaper");
        Ok(())
    }

    /// 设置信号处理器
    fn setup_signal_handlers(&self) -> Result<()> {
        // 处理SIGCHLD信号
        let running = self.running.clone();

        ctrlc::set_handler(move || {
            info!("Received SIGINT/SIGTERM, shutting down...");
            running.store(false, Ordering::SeqCst);
        })
        .context("Failed to set signal handler")?;

        Ok(())
    }

    fn shim_dir(&self) -> PathBuf {
        self.work_dir.join(&self.container_id)
    }

    fn attach_socket_container_dir(&self) -> PathBuf {
        match self.attach_socket_dir.as_ref() {
            Some(root) => root.join(&self.container_id),
            None => self.shim_dir(),
        }
    }

    fn cleanup_attach_socket_directory(&self) {
        let Some(root) = self.attach_socket_dir.as_ref() else {
            return;
        };
        if root == &self.work_dir {
            return;
        }
        let path = self.attach_socket_container_dir();
        if let Err(err) = fs::remove_dir_all(&path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    "Failed to remove attach socket directory {}: {}",
                    path.display(),
                    err
                );
            }
        }
    }

    fn load_bundle_config(&self) -> Result<ShimBundleConfig> {
        let config_path = self.bundle.join("config.json");
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read bundle config {:?}", config_path))?;
        let config = serde_json::from_str(&content).context("Failed to parse bundle config")?;
        Ok(config)
    }

    fn runtime_command(&self) -> Command {
        let mut cmd = Command::new(&self.runtime);
        if self.systemd_cgroup {
            cmd.arg("--systemd-cgroup");
        }
        if !self.runtime_config_path.as_os_str().is_empty() {
            cmd.arg("--config").arg(&self.runtime_config_path);
        }
        let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/run/user/0"));
        if !xdg_runtime_dir.as_os_str().is_empty() {
            cmd.env("XDG_RUNTIME_DIR", xdg_runtime_dir);
        }
        cmd
    }

    fn runtime_command_output(&self, args: &[&str]) -> Result<Output> {
        self.runtime_command()
            .args(args)
            .output()
            .with_context(|| format!("Failed to execute runtime {}", self.runtime.display()))
    }

    fn configure_monitor_cgroup(&self) -> Result<()> {
        let target = self.monitor_cgroup.trim();
        if target.is_empty() {
            return Ok(());
        }

        let bundle_config = self.load_bundle_config()?;
        let cgroup_target = if target == "pod" {
            bundle_config
                .linux
                .as_ref()
                .and_then(|linux| linux.cgroups_path.as_ref())
                .map(|path| path.trim())
                .filter(|path| !path.is_empty())
                .ok_or_else(|| anyhow::anyhow!("bundle config is missing linux.cgroupsPath"))?
                .to_string()
        } else {
            target.to_string()
        };

        move_pid_to_cgroup(std::process::id(), &cgroup_target).with_context(|| {
            format!(
                "failed to move shim {} into monitor cgroup {}",
                self.container_id, cgroup_target
            )
        })
    }

    fn load_container_state(&self, config: &ShimBundleConfig) -> Option<ShimStoredContainerState> {
        config
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(INTERNAL_CONTAINER_STATE_KEY))
            .and_then(|raw| serde_json::from_str(raw).ok())
    }

    fn is_terminal(&self) -> Result<bool> {
        let bundle_config = self.load_bundle_config()?;
        let container_state = self
            .load_container_state(&bundle_config)
            .unwrap_or_default();
        Ok(bundle_config
            .process
            .as_ref()
            .and_then(|process| process.terminal)
            .unwrap_or(container_state.tty))
    }

    /// 设置IO

    /// 创建TTY容器
    fn run_non_terminal_container(&self) -> Result<i32> {
        let bundle_config = self.load_bundle_config()?;
        let container_state = self
            .load_container_state(&bundle_config)
            .unwrap_or_default();

        let mut cmd = self.runtime_command();
        cmd.arg("run").arg("--bundle").arg(&self.bundle);
        if self.no_pivot {
            cmd.arg("--no-pivot");
        }
        if self.no_new_keyring {
            cmd.arg("--no-new-keyring");
        }
        cmd.arg(&self.container_id);

        if container_state.stdin {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        match container_state.log_path.as_ref() {
            Some(log_path) => {
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_path)
                    .with_context(|| format!("failed to open log file {}", log_path))?;
                let stderr = file.try_clone().context("failed to clone log file")?;
                cmd.stdout(Stdio::from(file));
                cmd.stderr(Stdio::from(stderr));
            }
            None => {
                cmd.stdout(Stdio::null());
                cmd.stderr(Stdio::null());
            }
        }

        let mut child = cmd.spawn().context("Failed to execute runc run")?;
        if let Some(stdin) = child.stdin.take() {
            drop(stdin);
        }
        info!(
            "Container {} started via foreground runc run (stdin={}, stdin_once={})",
            self.container_id, container_state.stdin, container_state.stdin_once
        );

        let shutdown_grace = std::time::Duration::from_secs(5);
        let mut shutdown_deadline: Option<std::time::Instant> = None;
        let status = loop {
            match child.try_wait().context("Failed to poll runc run status")? {
                Some(status) => break status,
                None => {
                    if !self.running.load(Ordering::SeqCst) {
                        let deadline = shutdown_deadline
                            .get_or_insert_with(|| std::time::Instant::now() + shutdown_grace);
                        if std::time::Instant::now() >= *deadline {
                            warn!(
                                "Shim shutdown for {} timed out waiting for runc run; force killing child {}",
                                self.container_id,
                                child.id()
                            );
                            let _ = child.kill();
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        };
        let exit_code = match (status.code(), status.signal()) {
            (Some(code), _) => code,
            (None, Some(signal)) => 128 + signal,
            _ => 1,
        };

        self.cleanup_container()?;
        Ok(exit_code)
    }


    /// 监控容器进程

    /// 清理容器
    fn cleanup_container(&self) -> Result<()> {
        info!("Cleaning up container: {}", self.container_id);

        // 尝试删除容器
        let output = self.runtime_command_output(&["delete", &self.container_id])?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Container cleanup warning: {}", stderr);
        }

        Ok(())
    }

    /// 记录退出码
    fn record_exit_code(&self, exit_code: i32) -> Result<()> {
        if let Some(path) = &self.exit_code_file {
            let parent = path.parent().context("Invalid exit code file path")?;
            fs::create_dir_all(parent)?;

            fs::write(path, exit_code.to_string()).context("Failed to write exit code file")?;

            info!("Recorded exit code {} to {:?}", exit_code, path);
        }

        Ok(())
    }
}

impl Daemon {
    fn wait_for_exit_code(&self, request: &WaitProcessRequest) -> Result<Option<i32>> {
        if let Some(exit_code) = *self.exit_code.lock().unwrap() {
            return Ok(Some(exit_code));
        }

        let deadline = request
            .timeout_ms
            .map(|timeout| Instant::now() + Duration::from_millis(timeout));
        loop {
            if let Some(exit_code) = *self.exit_code.lock().unwrap() {
                return Ok(Some(exit_code));
            }
            if let Some(deadline) = deadline {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn kill_task_internal(&self, request: &KillTaskRequest) -> Result<()> {
        let mut args = vec![
            "kill",
            request.container_id.as_str(),
            request.signal.as_str(),
        ];
        if request.all {
            args.insert(1, "--all");
        }
        let output = self
            .runtime_command()
            .args(&args)
            .output()
            .with_context(|| {
                format!(
                    "Failed to execute runtime kill for container {}",
                    request.container_id
                )
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(anyhow::anyhow!(
                "failed to kill container {}: {}",
                request.container_id,
                stderr
            ));
        }
        Ok(())
    }
    fn delete_task_internal(&self, request: &DeleteTaskRequest) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        if matches!(
            *self.task_state.lock().unwrap(),
            DaemonTaskState::Running | DaemonTaskState::Paused
        ) {
            let _ = self.kill_task_internal(&KillTaskRequest {
                container_id: request.container_id.clone(),
                signal: "KILL".to_string(),
                all: true,
            });
            let _ = self.wait_for_exit_code(&WaitProcessRequest {
                container_id: request.container_id.clone(),
                timeout_ms: Some(2_000),
            });
        }

        if let Some(handle) = self.task_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        self.set_task_state(DaemonTaskState::Deleted);
        Ok(())
    }

}
impl ShimRpcHandler for Daemon {
    fn handle_request(&self, request: ShimRpcRequest) -> Result<ShimRpcResponse> {
        match request {
            ShimRpcRequest::Ping => Ok(ShimRpcResponse::Empty),
            ShimRpcRequest::CreateTask(_) => {
                self.set_task_state(DaemonTaskState::Created);
                Ok(ShimRpcResponse::Empty)
            }
            ShimRpcRequest::StartTask(StartTaskRequest { .. }) => {
                self.spawn_task_runner()?;
                Ok(ShimRpcResponse::Empty)
            }
            ShimRpcRequest::WaitProcess(request) => {
                Ok(ShimRpcResponse::WaitProcess(WaitProcessResponse {
                    exit_code: self.wait_for_exit_code(&request)?,
                }))
            }
            ShimRpcRequest::KillTask(request) => {
                self.kill_task_internal(&request)?;
                Ok(ShimRpcResponse::Empty)
            }
            ShimRpcRequest::DeleteTask(request) => {
                self.delete_task_internal(&request)?;
                Ok(ShimRpcResponse::Empty)
            }
            ShimRpcRequest::Status(StatusRequest { .. }) => {
                Ok(ShimRpcResponse::Status(self.task_status()))
            }
            ShimRpcRequest::ContainerPid(StatusRequest { .. }) => Ok(
                ShimRpcResponse::ContainerPid(*self.container_pid.lock().unwrap()),
            ),
            _ => Err(anyhow::anyhow!(
                "RPC is not implemented by this shim milestone (task lifecycle)"
            )),
        }
    }
}


fn move_pid_to_cgroup(pid: u32, target: &str) -> Result<()> {
    let mount_point = Path::new("/sys/fs/cgroup");
    let relative = target
        .trim()
        .trim_start_matches('/')
        .trim_start_matches("./");
    if relative.is_empty() {
        return Ok(());
    }

    if mount_point.join("cgroup.controllers").exists() {
        let procs_file = mount_point.join(relative).join("cgroup.procs");
        if let Some(parent) = procs_file.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create cgroup directory {}", parent.display())
            })?;
        }
        fs::write(&procs_file, pid.to_string())
            .with_context(|| format!("Failed to write {}", procs_file.display()))?;
        return Ok(());
    }

    for subsystem in ["cpu", "memory", "pids"] {
        let procs_file = mount_point
            .join(subsystem)
            .join(relative)
            .join("cgroup.procs");
        if let Some(parent) = procs_file.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create cgroup directory {}", parent.display())
            })?;
        }
        fs::write(&procs_file, pid.to_string())
            .with_context(|| format!("Failed to write {}", procs_file.display()))?;
    }

    Ok(())
}

