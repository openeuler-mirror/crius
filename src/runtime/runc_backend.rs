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
    ContainerConfig, ContainerStatus
};
use crate::runtime::{
    RuncRuntime, RuntimeFeatureProbe,
    CgroupDriverConfig, LinuxContainerResources,
    ContainerRuntime, PreparedRootfsMount,
    MountSemanticsError, 
};
use crate::runtime::backend::{
    RuntimeBackend, TaskController,
    RuntimeContextManager, 
};
use crate::oci::spec::Spec;


#[derive(Debug, Clone)]
pub struct RuncBackend {
    inner: RuncRuntime,
}

impl RuncBackend {
    pub fn new(inner: RuncRuntime) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &RuncRuntime {
        &self.inner
    }
}

impl TaskController for RuncBackend {
    fn create_container(&self, container_id: &str, config: &ContainerConfig) -> Result<String> {
        self.inner.create_container(container_id, config)
    }

    fn start_container(&self, container_id: &str) -> Result<()> {
        self.inner.start_container(container_id)
    }

    fn stop_container(&self, container_id: &str, timeout: Option<u32>) -> Result<()> {
        self.inner.stop_container(container_id, timeout)
    }

    fn remove_container(&self, container_id: &str) -> Result<()> {
        self.inner.remove_container(container_id)
    }

    fn container_status(&self, container_id: &str) -> Result<ContainerStatus> {
        self.inner.container_status(container_id)
    }

    fn reopen_container_log(&self, container_id: &str) -> Result<()> {
        self.inner.reopen_container_log(container_id)
    }

    fn exec_in_container(&self, container_id: &str, command: &[String], tty: bool) -> Result<i32> {
        self.inner.exec_in_container(container_id, command, tty)
    }

    fn update_container_resources(
        &self,
        container_id: &str,
        resources: &LinuxContainerResources,
    ) -> Result<()> {
        self.inner
            .update_container_resources(container_id, resources)
    }

    fn is_container_paused(&self, container_id: &str) -> Result<bool> {
        self.inner.is_container_paused(container_id)
    }

    fn restore_attach_shim(&self, container_id: &str) -> Result<()> {
        self.inner.restore_attach_shim(container_id)
    }

    fn open_attach_stream(
        &self,
        container_id: &str,
        stdin: bool,
        stdout: bool,
        stderr: bool,
        tty: bool,
    ) -> Result<crate::shim_rpc::OpenAttachStreamResponse> {
        self.inner
            .open_attach_stream(container_id, stdin, stdout, stderr, tty)
    }

    fn close_attach_stream(&self, container_id: &str, stream_id: &str) -> Result<()> {
        self.inner.close_attach_stream(container_id, stream_id)
    }

    fn resize_attach_pty(
        &self,
        container_id: &str,
        stream_id: Option<&str>,
        width: u16,
        height: u16,
    ) -> Result<()> {
        self.inner
            .resize_attach_pty(container_id, stream_id, width, height)
    }

    fn shim_status(&self, container_id: &str) -> Result<Option<crate::shim_rpc::StatusResponse>> {
        self.inner.shim_status(container_id)
    }

    fn restore_container_from_checkpoint(
        &self,
        container_id: &str,
        checkpoint_path: &Path,
        work_path: &Path,
    ) -> Result<()> {
        self.inner
            .restore_container_from_checkpoint(container_id, checkpoint_path, work_path)
    }

    fn pause_container(&self, container_id: &str) -> Result<()> {
        self.inner.pause_container(container_id)
    }

    fn checkpoint_container(
        &self,
        container_id: &str,
        location: &Path,
        work_path: &Path,
    ) -> Result<()> {
        self.inner
            .checkpoint_container(container_id, location, work_path)
    }

    fn resume_container(&self, container_id: &str) -> Result<()> {
        self.inner.resume_container(container_id)
    }

    fn container_pid(&self, container_id: &str) -> Result<Option<i32>> {
        self.inner.container_pid(container_id)
    }
}

impl RuntimeContextManager for RuncBackend {
    fn bundle_path_for(&self, container_id: &str) -> PathBuf {
        self.inner.bundle_path_for(container_id)
    }

    fn enforce_oom_score_adj_policy(&self, spec: &mut Spec) -> Result<()> {
        self.inner.enforce_oom_score_adj_policy(spec)
    }

    fn prepare_rootfs(
        &self,
        container_id: &str,
        config: &ContainerConfig,
    ) -> Result<PreparedRootfsMount> {
        self.inner.prepare_rootfs(container_id, config)
    }

    fn build_spec(&self, container_id: &str, config: &ContainerConfig) -> Result<Spec> {
        self.inner.build_spec(container_id, config)
    }

    fn write_bundle(&self, container_id: &str, rootfs: &Path, spec: &Spec) -> Result<()> {
        self.inner.write_bundle(container_id, rootfs, spec)
    }

    fn create_task_from_prepared_bundle(
        &self,
        container_id: &str,
        rootfs: PreparedRootfsMount,
    ) -> Result<()> {
        self.inner
            .create_task_from_prepared_bundle(container_id, rootfs)
    }

    fn load_spec(&self, container_id: &str) -> Result<Spec> {
        self.inner.load_spec(container_id)
    }

    fn validate_mount_requests(
        &self,
        config: &ContainerConfig,
    ) -> std::result::Result<(), MountSemanticsError> {
        self.inner.validate_mount_requests(config)
    }
}

impl RuntimeBackend for RuncBackend {
    fn backend_name(&self) -> &str {
        "runc"
    }

    fn runtime_root(&self) -> &Path {
        self.inner.runtime_root()
    }

    fn runtime_path(&self) -> &Path {
        self.inner.runtime_path()
    }

    fn runtime_config_path(&self) -> &Path {
        self.inner.runtime_config_path()
    }

    fn task_controller(&self) -> &dyn TaskController {
        self
    }

    fn runtime_context(&self) -> &dyn RuntimeContextManager {
        self
    }

    fn probe_runtime_features(&self) -> RuntimeFeatureProbe {
        self.inner.probe_runtime_features()
    }

    fn cgroup_driver(&self) -> CgroupDriverConfig {
        self.inner.cgroup_driver()
    }
}