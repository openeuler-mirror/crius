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


use std::unimplemented;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tonic::{Request, Response, Status};
use serde_json::json;

use crate::proto::runtime::v1::{
    CreateContainerRequest, CreateContainerResponse,
    StartContainerRequest, StartContainerResponse,
    UpdateContainerResourcesRequest, UpdateContainerResourcesResponse,
    StopContainerRequest, StopContainerResponse,
    RemoveContainerRequest, RemoveContainerResponse,
    NamespaceMode, PodSandboxConfig,
    PodSandboxMetadata, PodSandboxState,
    ContainerMetadata
};
use crate::server::service::{
    RuntimeServiceImpl, NameReservationGuard,
};
use crate::service::event::InternalEventSeverity;
use crate::server::state_model::{
    StoredPodState, StoredNamespaceOptions
};
use crate::defaults::{
    CRS_RUN_ANNOTATION, CRS_RUN_ANNOTATION_VALUE,
    RANDOM_NAME_LEFT, RANDOM_NAME_RIGHT,
    INTERNAL_POD_STATE_KEY,
};

enum ContainerOwner {
    Local { runtime_handler: Option<String> },
    Pod { pod_sandbox_id: String },
}

impl ContainerOwner {
    fn pod_sandbox_id(&self) -> &str {
        match self {
            Self::Local { .. } => "",
            Self::Pod { pod_sandbox_id } => pod_sandbox_id,
        }
    }
}

struct ContainerCreateInput {
    config: crate::proto::runtime::v1::ContainerConfig,
    sandbox_config: Option<PodSandboxConfig>,
    owner: ContainerOwner,
}

impl RuntimeServiceImpl {
    pub async fn create_local_container_impl(
        &self,
        request: Request<crate::proto::local::v1::CreateLocalContainerRequest>,
    ) -> Result<Response<crate::proto::local::v1::CreateLocalContainerResponse>, Status> {
        log::info!("CreateLocalContainer called");
        let req = request.into_inner();
        let config = req
            .config
            .ok_or_else(|| Status::invalid_argument("Container config not specified"))?;
        let runtime_handler = (!req.runtime_handler.trim().is_empty())
            .then(|| req.runtime_handler.trim().to_string());
        let mut sandbox_config = crate::proto::runtime::v1::PodSandboxConfig::default();
        sandbox_config.linux = Some(crate::proto::runtime::v1::LinuxPodSandboxConfig {
            cgroup_parent: req.cgroup_parent,
            sysctls: req
                .sysctls
                .iter()
                .filter_map(|raw| raw.split_once('='))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            security_context: Some(crate::proto::runtime::v1::LinuxSandboxSecurityContext {
                namespace_options: Some(crate::proto::runtime::v1::NamespaceOption {
                    network: NamespaceMode::Pod as i32,
                    pid: NamespaceMode::Pod as i32,
                    ipc: NamespaceMode::Pod as i32,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
        let response = self
            .create_container_from_input(ContainerCreateInput {
                config,
                sandbox_config: Some(sandbox_config),
                owner: ContainerOwner::Local { runtime_handler },
            })
            .await?;
        Ok(Response::new(
            crate::proto::local::v1::CreateLocalContainerResponse {
                container_id: response.into_inner().container_id,
            },
        ))
    }

    pub(super) async fn create_container_impl(
        &self,
        request: Request<CreateContainerRequest>,
    ) -> Result<Response<CreateContainerResponse>, Status> {
        unimplemented!()
    }

    async fn create_container_from_input(
        &self,
        input: ContainerCreateInput,
    ) -> Result<Response<CreateContainerResponse>, Status> {
        // 请求解析与校验
        let ContainerCreateInput {
            mut config,
            sandbox_config,
            owner,
        } = input;
        let pod_sandbox_id = owner.pod_sandbox_id().to_string();
        let mut container_metadata = config
            .metadata
            .clone()
            .ok_or_else(|| Status::invalid_argument("Container config metadata not specified"))?;
        Self::validate_container_image_spec(&config)?;
        let pod_metadata = match &owner {
            ContainerOwner::Local { .. } => PodSandboxMetadata {
                name: "local".to_string(),
                namespace: "local".to_string(),
                uid: "local".to_string(),
                attempt: 0,
            },
            ContainerOwner::Pod { pod_sandbox_id } => {
                let pod_sandboxes = self.pod_sandboxes.lock().await;
                let pod = pod_sandboxes
                    .get(pod_sandbox_id)
                    .ok_or_else(|| Status::not_found("Pod sandbox not found"))?;
                if pod.state != PodSandboxState::SandboxReady as i32 {
                    return Err(Self::create_container_sandbox_not_ready_error(
                        pod_sandbox_id,
                        pod.state,
                    ));
                }
                pod.metadata
                    .clone()
                    .ok_or_else(|| Status::failed_precondition("Pod sandbox metadata is missing"))?
            }
        };

        // 生成容器ID和日志路径
        let container_id = uuid::Uuid::new_v4().to_simple().to_string();
        if config.log_path.trim().is_empty()
            && Self::should_assign_default_log_path(&owner, &config)
        {
            config.log_path = self.default_container_log_path(&container_id);
        }

        // 仿照docker容器命名机制
        let mut container_name_guard = self
            .reserve_container_name_like_docker(
                &container_id,
                &mut container_metadata,
                &pod_metadata,
            )
            .await?;

        // 注册容器生命周期事件
        config.metadata = Some(container_metadata.clone());
        self.publish_container_lifecycle_event(
            &container_id,
            "create_start",
            InternalEventSeverity::Info,
            json!({
                "podSandboxId": pod_sandbox_id.clone(),
                "name": container_metadata.name.clone(),
                "attempt": container_metadata.attempt,
            }),
        )
        .await;
      
        log::info!("Creating container with ID: {}", container_id);
        log::debug!("Container config: {:?}", config);

        // 提取pod state和 annotations
        let pod_state = match &owner {
            ContainerOwner::Local { .. } => None,
            ContainerOwner::Pod { pod_sandbox_id } => {
                let pod_sandboxes = self.pod_sandboxes.lock().await;
                pod_sandboxes.get(pod_sandbox_id).and_then(|pod| {
                    Self::read_internal_state::<StoredPodState>(
                        &pod.annotations,
                        INTERNAL_POD_STATE_KEY,
                    )
                })
            }
        };
        let pod_external_annotations = match &owner {
            ContainerOwner::Local { .. } => HashMap::new(),
            ContainerOwner::Pod { pod_sandbox_id } => {
                let pod_sandboxes = self.pod_sandboxes.lock().await;
                pod_sandboxes
                    .get(pod_sandbox_id)
                    .map(|pod| Self::external_pod_annotations(&pod.annotations))
                    .unwrap_or_default()
            }
        };

        // 提取容器和pod相关配置
        let sandbox_linux = sandbox_config
            .as_ref()
            .and_then(|config| config.linux.as_ref());
        let security = config
            .linux
            .as_ref()
            .and_then(|linux| linux.security_context.as_ref());
        let sandbox_namespace_options = sandbox_linux
            .and_then(|linux| linux.security_context.as_ref())
            .and_then(|security| security.namespace_options.as_ref())
            .map(StoredNamespaceOptions::from)
            .or_else(|| {
                pod_state
                    .as_ref()
                    .and_then(|state| state.namespace_options.clone())
            });
        let namespace_options = self.effective_container_namespace_options(
            security.and_then(|security| security.namespace_options.as_ref()),
            sandbox_namespace_options.as_ref(),
        );
        let run_as_user = security
            .and_then(|security| security.run_as_user.as_ref())
            .map(|user| user.value.to_string())
            .or_else(|| {
                security.and_then(|security| {
                    if security.run_as_username.is_empty() {
                        None
                    } else {
                        Some(security.run_as_username.clone())
                    }
                })
            });
        let run_as_group = security
            .and_then(|security| security.run_as_group.as_ref())
            .and_then(|group| u32::try_from(group.value).ok());
        let supplemental_groups: Vec<u32> = security
            .map(|security| {
                security
                    .supplemental_groups
                    .iter()
                    .filter_map(|group| u32::try_from(*group).ok())
                    .collect()
            })
            .unwrap_or_default();
        self.validate_minimum_mappable_ids(
            namespace_options.as_ref(),
            run_as_user.as_deref(),
            run_as_group,
            &supplemental_groups,
        )?;

        // 日志路径解析与目录创建
        let pod_log_directory = sandbox_config
            .as_ref()
            .and_then(|config| {
                (!config.log_directory.is_empty()).then(|| config.log_directory.clone())
            })
            .or_else(|| {
                pod_state
                    .as_ref()
                    .and_then(|state| state.log_directory.clone())
            });
        let log_path =
            Self::resolve_container_log_path(pod_log_directory.as_deref(), &config.log_path)?;

        if let Some(path) = &log_path {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    Status::internal(format!("Failed to prepare log directory: {}", e))
                })?;
            }
        }

        // 网络命名空间与本地网络
        let network_namespace_path = match &owner {
            ContainerOwner::Local { .. } => None,
            ContainerOwner::Pod { pod_sandbox_id } => unimplemented!(),
        }
        .or_else(|| {
            pod_state
                .as_ref()
                .and_then(|state| state.netns_path.as_ref().map(PathBuf::from))
        });
        let pause_container_id = match &owner {
            ContainerOwner::Local { .. } => None,
            ContainerOwner::Pod { pod_sandbox_id } => unimplemented!(),
        }
        .or_else(|| {
            pod_state
                .as_ref()
                .and_then(|state| state.pause_container_id.clone())
        });
        let pid_namespace_path = if let Some(options) = namespace_options.as_ref() {
            if options.pid == NamespaceMode::Pod as i32 {
                if let Some(pause_id) = pause_container_id.as_deref() {
                    self.runtime_namespace_path_for_container(pause_id, "pid")
                        .await?
                } else {
                    None
                }
            } else if options.pid == NamespaceMode::Target as i32 {
                self.runtime_namespace_path_for_target(&options.target_id, "pid")
                    .await?
            } else {
                None
            }
        } else {
            None
        };
        let ipc_namespace_path = if let Some(options) = namespace_options.as_ref() {
            if options.ipc == NamespaceMode::Pod as i32 {
                if let Some(pause_id) = pause_container_id.as_deref() {
                    self.runtime_namespace_path_for_container(pause_id, "ipc")
                        .await?
                } else {
                    None
                }
            } else if options.ipc == NamespaceMode::Target as i32 {
                self.runtime_namespace_path_for_target(&options.target_id, "ipc")
                    .await?
            } else {
                None
            }
        } else {
            None
        };

        unimplemented!()
    }

    pub(super) async fn start_container_impl(
        &self,
        request: Request<StartContainerRequest>,
    ) -> Result<Response<StartContainerResponse>, Status> {
        unimplemented!()
    }

    pub(super) async fn update_container_resources(
        &self,
        request: Request<UpdateContainerResourcesRequest>,
    ) -> Result<Response<UpdateContainerResourcesResponse>, Status> {
        unimplemented!()
    }

    pub(super) async fn stop_container(
        &self,
        request: Request<StopContainerRequest>,
    ) -> Result<Response<StopContainerResponse>, Status> {
        unimplemented!()
    }

    pub(super) async fn remove_container(
        &self,
        request: Request<RemoveContainerRequest>,
    ) -> Result<Response<RemoveContainerResponse>, Status> {
        unimplemented!()
    }

    fn create_container_sandbox_not_ready_error(
        pod_sandbox_id: &str,
        sandbox_state: i32,
    ) -> Status {
        let state_name = match sandbox_state {
            x if x == PodSandboxState::SandboxReady as i32 => "ready",
            x if x == PodSandboxState::SandboxNotready as i32 => "notready",
            _ => "unknown",
        };
        Status::failed_precondition(format!(
            "CreateContainer failed as the sandbox is not ready: {} (state: {})",
            pod_sandbox_id, state_name
        ))
    }

    fn should_assign_default_log_path(
        owner: &ContainerOwner,
        config: &crate::proto::runtime::v1::ContainerConfig,
    ) -> bool {
        matches!(owner, ContainerOwner::Local { .. })
            || config
                .annotations
                .get(CRS_RUN_ANNOTATION)
                .is_some_and(|value| value == CRS_RUN_ANNOTATION_VALUE)
    }
    
    fn default_container_log_path(&self, container_id: &str) -> String {
        self.config
            .log_dir
            .join("containers")
            .join(format!("{container_id}.log"))
            .display()
            .to_string()
    }

    async fn reserve_container_name_like_docker(
        &self,
        container_id: &str,
        metadata: &mut ContainerMetadata,
        pod_metadata: &PodSandboxMetadata,
    ) -> Result<NameReservationGuard, Status> {
        // 使用用户指定名称
        if !metadata.name.trim().is_empty() {
            let name_key = Self::container_name_key(metadata, pod_metadata);
            return self
                .reserve_container_name_for_create(container_id, &name_key)
                .await;
        }

        // 使用随机名生成 + 冲突重试
        for retry in 0..6 {
            metadata.name = Self::random_container_name(retry);
            let name_key = Self::container_name_key(metadata, pod_metadata);
            match self
                .reserve_container_name_for_create(container_id, &name_key)
                .await
            {
                Ok(guard) => return Ok(guard),
                Err(status) if status.code() == tonic::Code::AlreadyExists => continue,
                Err(status) => return Err(status),
            }
        }

        // 通过container ID前short_id 进行兜底
        metadata.name = crate::crs::ids::short_id(container_id).to_string();
        let name_key = Self::container_name_key(metadata, pod_metadata);
        self.reserve_container_name_for_create(container_id, &name_key)
            .await
    }

    fn random_container_name(retry: usize) -> String {
        use rand::Rng;

        let mut rng = rand::thread_rng();
        loop {
            let left = RANDOM_NAME_LEFT[rng.gen_range(0..RANDOM_NAME_LEFT.len())];
            let right = RANDOM_NAME_RIGHT[rng.gen_range(0..RANDOM_NAME_RIGHT.len())];
            let mut name = format!("{left}_{right}");
            if name == "boring_wozniak" {
                continue;
            }
            if retry > 0 {
                name.push_str(&rng.gen_range(0..10).to_string());
            }
            return name;
        }
    }

    pub(super) fn reserve_container_name(
        &self,
        container_id: &str,
        name: &str,
    ) -> Result<NameReservationGuard, Status> {
        let mut registry = self
            .container_names
            .lock()
            .map_err(|_| Status::internal("container name registry lock poisoned"))?;
        if let Err(existing_id) = registry.reserve(name, container_id) {
            return Err(Status::already_exists(format!(
                "container with name {name:?} already exists as {existing_id}"
            )));
        }
        drop(registry);
        Ok(NameReservationGuard::new(
            container_id,
            self.container_names.clone(),
        ))
    }

    pub(super) async fn reserve_container_name_for_create(
        &self,
        container_id: &str,
        name: &str,
    ) -> Result<NameReservationGuard, Status> {
        match self.reserve_container_name(container_id, name) {
            Ok(guard) => Ok(guard),
            Err(status)if status.code() == tonic::Code::AlreadyExists => unimplemented!(),
            Err(status) => Err(status),
        }
    }

    pub(super) fn resolve_container_log_path(
        sandbox_log_directory: Option<&str>,
        container_log_path: &str,
    ) -> Result<Option<PathBuf>, Status> {
        let log_path = container_log_path.trim();
        if log_path.is_empty() {
            return Ok(None);
        }

        let path = Path::new(log_path);
        let sandbox_log_directory = sandbox_log_directory
            .map(str::trim)
            .filter(|dir| !dir.is_empty());

        if let Some(log_directory) = sandbox_log_directory {
            if path.is_absolute() {
                return Err(Status::invalid_argument(
                    "container log_path must be relative when sandbox log_directory is set",
                ));
            }
            if path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            }) {
                return Err(Status::invalid_argument(
                    "container log_path must not escape the sandbox log_directory",
                ));
            }
            return Ok(Some(PathBuf::from(log_directory).join(path)));
        }

        if !path.is_absolute() {
            return Err(Status::invalid_argument(
                "container log_path must be absolute when sandbox log_directory is not set",
            ));
        }

        Ok(Some(path.to_path_buf()))
    }


}