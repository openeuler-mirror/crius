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

use anyhow::Result;

use crate::proto::runtime::v1::{
    ContainerConfig, ContainerStatus,
    LinuxContainerResources,
};
use crate::config::CgroupDriverConfig;
use crate::runtime::{
    RuntimeFeatureProbe, PreparedRootfsMount, 
    MountSemanticsError,
};
use crate::shim_rpc::OpenAttachStreamResponse;
use crate::oci::spec::Spec;

#[derive(Debug, Clone, Copy)]
pub enum RuntimeContextKind {
    OciBundle,
    DirectTask,
}

pub trait TaskController: Send + Sync {
    fn create_container(&self, container_id: &str, config: &ContainerConfig) -> Result<String>;
    fn start_container(&self, container_id: &str) -> Result<()>;
    fn stop_container(&self, container_id: &str, timeout: Option<u32>) -> Result<()>;
    fn remove_container(&self, container_id: &str) -> Result<()>;
    fn container_status(&self, container_id: &str) -> Result<ContainerStatus>;
    fn reopen_container_log(&self, container_id: &str) -> Result<()>;
    fn exec_in_container(&self, container_id: &str, command: &[String], tty: bool) -> Result<i32>;
    fn update_container_resources(
        &self,
        container_id: &str,
        resources: &LinuxContainerResources,
    ) -> Result<()>;
    fn is_container_paused(&self, container_id: &str) -> Result<bool>;
    fn restore_attach_shim(&self, container_id: &str) -> Result<()>;
    fn open_attach_stream(
        &self,
        container_id: &str,
        stdin: bool,
        stdout: bool,
        stderr: bool,
        tty: bool,
    ) -> Result<OpenAttachStreamResponse>;
    fn close_attach_stream(&self, container_id: &str, stream_id: &str) -> Result<()>;
    fn resize_attach_pty(
        &self,
        container_id: &str,
        stream_id: Option<&str>,
        width: u16,
        height: u16,
    ) -> Result<()>;
    fn shim_status(&self, container_id: &str) -> Result<Option<crate::shim_rpc::StatusResponse>>;
    fn restore_container_from_checkpoint(
        &self,
        container_id: &str,
        checkpoint_path: &Path,
        work_path: &Path,
    ) -> Result<()>;
    fn pause_container(&self, container_id: &str) -> Result<()>;
    fn checkpoint_container(
        &self,
        container_id: &str,
        location: &Path,
        work_path: &Path,
    ) -> Result<()>;
    fn resume_container(&self, container_id: &str) -> Result<()>;
    fn container_pid(&self, container_id: &str) -> Result<Option<i32>>;
}

pub trait RuntimeContextManager: Send + Sync {
    fn bundle_path_for(&self, container_id: &str) -> PathBuf;
    fn enforce_oom_score_adj_policy(&self, spec: &mut Spec) -> Result<()>;
    fn prepare_rootfs(
        &self,
        container_id: &str,
        config: &ContainerConfig,
    ) -> Result<PreparedRootfsMount>;
    fn build_spec(&self, container_id: &str, config: &ContainerConfig) -> Result<Spec>;
    fn write_bundle(&self, container_id: &str, rootfs: &Path, spec: &Spec) -> Result<()>;
    fn create_task_from_prepared_bundle(
        &self,
        container_id: &str,
        rootfs: PreparedRootfsMount,
    ) -> Result<()>;
    fn load_spec(&self, container_id: &str) -> Result<Spec>;
    fn validate_mount_requests(
        &self,
        config: &ContainerConfig,
    ) -> std::result::Result<(), MountSemanticsError>;
}

pub trait RuntimeBackend: Send + Sync {
    fn backend_name(&self) -> &str;
    fn context_kind(&self) -> RuntimeContextKind {
        RuntimeContextKind::OciBundle
    }
    fn runtime_root(&self) -> &Path;
    fn runtime_path(&self) -> &Path;
    fn runtime_config_path(&self) -> &Path;
    fn task_controller(&self) -> &dyn TaskController;
    fn runtime_context(&self) -> &dyn RuntimeContextManager;
    fn probe_runtime_features(&self) -> RuntimeFeatureProbe;
    fn cgroup_driver(&self) -> CgroupDriverConfig;
}
