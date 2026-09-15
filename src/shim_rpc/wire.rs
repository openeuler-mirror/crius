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
use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::proto::ShimRpcResponse;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RpcEnvelope<T> {
    payload: T,
}

impl<T> RpcEnvelope<T> {
    pub(super) fn new(payload: T) -> Self {
        Self { payload }
    }

    pub(super) fn into_payload(self) -> T {
        self.payload
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RpcResultEnvelope {
    ok: bool,
    payload: Option<ShimRpcResponse>,
    error: Option<String>,
}

impl RpcResultEnvelope {
    pub(super) fn success(payload: ShimRpcResponse) -> Self {
        Self {
            ok: true,
            payload: Some(payload),
            error: None,
        }
    }

    pub(super) fn failure(error: String) -> Self {
        Self {
            ok: false,
            payload: None,
            error: Some(error),
        }
    }

    pub(super) fn into_result(self) -> Result<ShimRpcResponse> {
        if self.ok {
            self.payload
                .ok_or_else(|| anyhow::anyhow!("shim RPC response was missing a payload"))
        } else {
            Err(anyhow::anyhow!(
                "{}",
                self.error
                    .unwrap_or_else(|| "shim RPC request failed".to_string())
            ))
        }
    }
}
