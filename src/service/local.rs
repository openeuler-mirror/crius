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


use tonic::{Request, Response, Status};

use crate::proto::local::v1::{
    local_service_server::LocalService, CreateLocalContainerRequest, CreateLocalContainerResponse,
};
use crate::server::service::RuntimeServiceImpl;

pub struct LocalServiceImpl {
    runtime: RuntimeServiceImpl,
}

impl LocalServiceImpl {
    pub fn new(runtime: RuntimeServiceImpl) -> Self {
        Self { runtime }
    }
}

#[tonic::async_trait]
impl LocalService for LocalServiceImpl {
    async fn create_local_container(
        &self,
        request: Request<CreateLocalContainerRequest>,
    ) -> Result<Response<CreateLocalContainerResponse>, Status> {
        self.runtime.create_local_container_impl(request).await
    }
}