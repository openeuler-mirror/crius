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
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use super::proto::{ShimRpcRequest, ShimRpcResponse};
use super::wire::{RpcEnvelope, RpcResultEnvelope};

#[derive(Debug, Clone)]
pub struct ShimRpcClient {
    socket_path: PathBuf,
    timeout: Duration,
}

impl ShimRpcClient {
    pub fn new(socket_path: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout,
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn request(&self, payload: ShimRpcRequest) -> Result<ShimRpcResponse> {
        let mut stream = UnixStream::connect(&self.socket_path).with_context(|| {
            format!(
                "failed to connect to shim RPC socket {}",
                self.socket_path.display()
            )
        })?;
        stream
            .set_read_timeout(Some(self.timeout))
            .context("failed to configure shim RPC read timeout")?;
        stream
            .set_write_timeout(Some(self.timeout))
            .context("failed to configure shim RPC write timeout")?;

        let request = serde_json::to_vec(&RpcEnvelope::new(payload))
            .context("failed to encode shim RPC request")?;
        stream
            .write_all(&request)
            .context("failed to write shim RPC request")?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .context("failed to half-close shim RPC socket")?;

        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .context("failed to read shim RPC response")?;
        let envelope: RpcResultEnvelope =
            serde_json::from_slice(&response).context("failed to decode shim RPC response")?;
        envelope.into_result()
    }
}
