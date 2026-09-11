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


use std::time::Duration;
use std::path::Path;

use tonic::transport::{Channel, Endpoint as TonicEndpoint};
use hyper::Uri;
use tokio::net::UnixStream;
use tower::service_fn;

use crate::proto::runtime::v1::{runtime_service_client::RuntimeServiceClient, image_service_client::ImageServiceClient};
use crate::crs::{CliContext, error::CliError, parsers::Endpoint};

#[derive(Clone, Debug)]
pub(crate) struct CrsClient {
    endpoint: Endpoint,
    endpoint_display: String,
    connect_timeout: Duration,
    rpc_timeout: Duration,
    runtime: Option<RuntimeServiceClient<Channel>>,
    image: Option<ImageServiceClient<Channel>>,
    // diagnostics: Option<DiagnosticsServiceClient<Channel>>,
    // local: Option<LocalServiceClient<Channel>>,
}

impl CrsClient {
    pub(crate) fn new(ctx: &CliContext) -> Self {
        let endpoint = ctx.endpoint().clone();
        let endpoint_display = ctx.endpoint_display();

        Self {
            endpoint,
            endpoint_display,
            connect_timeout: ctx.connect_timeout(),
            rpc_timeout: ctx.rpc_timeout(),
            runtime: None,
            image: None,
            // diagnostics: None,
            // local: None,
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn connect(ctx: &CliContext) -> Result<Self, CliError> {
        let mut client = Self::new(ctx);
        let channel = client.connect_channel().await?;
        client.runtime = Some(RuntimeServiceClient::new(channel.clone()));
        client.image = Some(ImageServiceClient::new(channel.clone()));
        // client.diagnostics = Some(DiagnosticsServiceClient::new(channel.clone()));
        // client.local = Some(LocalServiceClient::new(channel));
        Ok(client)
    }

    #[allow(dead_code)]
    async fn connect_channel(&self) -> Result<Channel, CliError> {
        let connect = async {
            match self.endpoint_kind() {
                Endpoint::Unix(path) => connect_unix_channel(path).await,
                Endpoint::Tcp(uri) => connect_tcp_channel(uri).await,
            }
        };

        tokio::time::timeout(self.connect_timeout, connect)
            .await
            .map_err(|_| CliError::timeout("connection timed out", self.endpoint()))?
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint_display
    }

    #[allow(dead_code)]
    pub(crate) fn endpoint_kind(&self) -> &Endpoint {
        &self.endpoint
    }


}

async fn connect_unix_channel(path: &str) -> Result<Channel, CliError> {
    if !Path::new(path).exists() {
        return Err(CliError::daemon_unavailable(
            format!("unix://{path}"),
            format!("socket {path} does not exist"),
        ));
    }

    let path = path.to_string();
    TonicEndpoint::try_from("http://[::]:50051")
        .map_err(|source| CliError::internal(format!("failed to build unix channel: {source}")))?
        .connect_with_connector(service_fn(move |_uri: Uri| {
            let path = path.clone();
            async move { UnixStream::connect(path).await }
        }))
        .await
        .map_err(|source| {
            CliError::daemon_unavailable("unix socket", format!("failed to connect: {source}"))
        })
}

async fn connect_tcp_channel(uri: &str) -> Result<Channel, CliError> {
    TonicEndpoint::from_shared(uri.to_string())
        .map_err(|source| CliError::internal(format!("invalid TCP endpoint {uri}: {source}")))?
        .connect()
        .await
        .map_err(|source| CliError::daemon_unavailable(uri, format!("failed to connect: {source}")))
}
