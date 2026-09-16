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

use tonic::{Request, Response, Status};

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
    RuntimeServiceImpl, NameReservationGuard
};
use crate::defaults::{
    CRS_RUN_ANNOTATION, CRS_RUN_ANNOTATION_VALUE,
    RANDOM_NAME_LEFT, RANDOM_NAME_RIGHT,
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


}