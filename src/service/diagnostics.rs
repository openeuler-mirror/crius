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


use serde_json::Value;
use std::unimplemented;
use std::{path::Path, time::Duration};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::image::{content_store::TransferState, ImageServiceImpl};
use crate::proto::diagnostics::v1::{
    diagnostics_service_server::DiagnosticsService, ContainerLogChunk, ContainerLogRequest,
    EffectiveConfigRequest, EffectiveConfigResponse, ImageTransferInfo, ImageTransfersRequest,
    ImageTransfersResponse, NriStatusRequest, NriStatusResponse, ContentGcRequest, ContentGcResponse,
    RecoveryCheckRequest, RecoveryCheckResponse, RecoveryStatusRequest, RecoveryStatusResponse,
    RuntimeHandlerInfo, RuntimeHandlersRequest, RuntimeHandlersResponse, SecurityStatusRequest,
    SecurityStatusResponse, ServerInfoRequest, ServerInfoResponse, ShimInfo, ShimStatusRequest,
    ShimStatusResponse,
};
use crate::server::service::RuntimeServiceImpl;

#[derive(Clone, Default)]
pub struct DiagnosticsState {
    version: String,
    git_commit: String,
    config_path: String,
    state_dir: String,
    socket_path: String,
    config_json: String,
    redacted_config_json: String,
    redacted_fields: Vec<String>,
    runtime_handlers: Vec<RuntimeHandlerInfo>,
    runtime_handler_warnings: Vec<String>,
    image_service: Option<ImageServiceImpl>,
    runtime_service: Option<RuntimeServiceImpl>,
}

impl DiagnosticsState {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn new(
        version: impl Into<String>,
        git_commit: impl Into<String>,
        config_path: impl Into<String>,
        state_dir: impl Into<String>,
        socket_path: impl Into<String>,
        config: &crate::config::Config,
    ) -> Self {
        let config_value = serde_json::to_value(config).unwrap_or_else(|_| Value::Null);
        let config_json = serde_json::to_string(&config_value).unwrap_or_else(|_| "{}".into());
        let mut redacted_config = config_value;
        let mut redacted_fields = Vec::new();
        redact_sensitive_config(&mut redacted_config, "", &mut redacted_fields);
        let redacted_config_json =
            serde_json::to_string(&redacted_config).unwrap_or_else(|_| "{}".into());
        let (runtime_handlers, runtime_handler_warnings) = runtime_handlers_from_config(config);

        Self {
            version: version.into(),
            git_commit: git_commit.into(),
            config_path: config_path.into(),
            state_dir: state_dir.into(),
            socket_path: socket_path.into(),
            config_json,
            redacted_config_json,
            redacted_fields,
            runtime_handlers,
            runtime_handler_warnings,
            image_service: None,
            runtime_service: None,
        }
    }

    pub fn from_runtime(
        version: impl Into<String>,
        git_commit: impl Into<String>,
        config: &crate::config::Config,
        runtime: &RuntimeServiceImpl,
        socket_path: impl Into<String>,
    ) -> Self {
        let snapshot = runtime.diagnostics_snapshot(socket_path);
        let mut state = Self::new(
            version,
            git_commit,
            snapshot
                .config_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            snapshot.state_dir.display().to_string(),
            snapshot.socket_path,
            config,
        );
        state.image_service = Some(snapshot.image_service);
        state.runtime_service = Some(runtime.clone());
        state
    }
}

#[derive(Clone)]
pub struct DiagnosticsServiceImpl {
    state: DiagnosticsState,
}

impl DiagnosticsServiceImpl {
    pub fn new(state: DiagnosticsState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &DiagnosticsState {
        &self.state
    }
}

#[tonic::async_trait]
impl DiagnosticsService for DiagnosticsServiceImpl {
    async fn server_info(
        &self,
        _request: Request<ServerInfoRequest>,
    ) -> Result<Response<ServerInfoResponse>, Status> {
        Ok(Response::new(ServerInfoResponse {
            version: self.state.version.clone(),
            git_commit: self.state.git_commit.clone(),
            config_path: self.state.config_path.clone(),
            state_dir: self.state.state_dir.clone(),
            socket_path: self.state.socket_path.clone(),
        }))
    }

    async fn effective_config(
        &self,
        request: Request<EffectiveConfigRequest>,
    ) -> Result<Response<EffectiveConfigResponse>, Status> {
        let include_sensitive = request.into_inner().include_sensitive;
        Ok(Response::new(EffectiveConfigResponse {
            config_json: if include_sensitive {
                self.state.config_json.clone()
            } else {
                self.state.redacted_config_json.clone()
            },
            redacted_fields: if include_sensitive {
                Vec::new()
            } else {
                self.state.redacted_fields.clone()
            },
            warnings: Vec::new(),
        }))
    }

    async fn runtime_handlers(
        &self,
        _request: Request<RuntimeHandlersRequest>,
    ) -> Result<Response<RuntimeHandlersResponse>, Status> {
        let mut handlers = self.state.runtime_handlers.clone();
        for handler in &mut handlers {
            handler
                .warnings
                .extend(self.state.runtime_handler_warnings.clone());
        }

        Ok(Response::new(RuntimeHandlersResponse { handlers }))
    }

    async fn image_transfers(
        &self,
        request: Request<ImageTransfersRequest>,
    ) -> Result<Response<ImageTransfersResponse>, Status> {
        let include_completed = request.into_inner().include_completed;
        let Some(image_service) = self.state.image_service.as_ref() else {
            return Ok(Response::new(ImageTransfersResponse {
                transfers: Vec::new(),
            }));
        };
        let status = image_service.content_transfer_status();
        let transfers = status
            .active
            .into_iter()
            .chain(status.recent)
            .filter(|record| include_completed || record.state != TransferState::Succeeded)
            .map(|record| ImageTransferInfo {
                image: record.source,
                status: record.state.as_str().to_string(),
                updated_at_unix_nanos: record
                    .finished_at_unix_nanos
                    .unwrap_or(record.started_at_unix_nanos),
                error: record.error.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(ImageTransfersResponse { transfers }))
    }

    async fn recovery_status(
        &self,
        _request: Request<RecoveryStatusRequest>,
    ) -> Result<Response<RecoveryStatusResponse>, Status> {
        let Some(runtime) = self.state.runtime_service.as_ref() else {
            return Ok(Response::new(RecoveryStatusResponse {
                status: "unknown".to_string(),
                last_startup: "unknown".to_string(),
                unhealthy_object_count: 0,
                ledger_summary_json: "{}".to_string(),
                warnings: vec!["runtime diagnostics state is not available".to_string()],
            }));
        };

        let mut warnings = Vec::new();
        let (status, unhealthy_object_count, ledger_summary_json) =
            match runtime.recovery_ledger_health_summary().await {
                Ok(summary) => {
                    let status = if summary.is_healthy() {
                        "healthy"
                    } else {
                        "degraded"
                    };
                    let unhealthy = summary.unhealthy_object_count() as u64;
                    let json = serde_json::to_string(&summary).map_err(|err| {
                        Status::internal(format!("failed to encode recovery ledger summary: {err}"))
                    })?;
                    (status.to_string(), unhealthy, json)
                }
                Err(err) => {
                    warnings.push(err);
                    ("unknown".to_string(), 0, "{}".to_string())
                }
            };

        Ok(Response::new(RecoveryStatusResponse {
            status,
            last_startup: last_startup_summary(runtime),
            unhealthy_object_count,
            ledger_summary_json,
            warnings,
        }))
    }

    async fn recovery_check(
        &self,
        request: Request<RecoveryCheckRequest>,
    ) -> Result<Response<RecoveryCheckResponse>, Status> {
        unimplemented!()
    }

    async fn shim_status(
        &self,
        request: Request<ShimStatusRequest>,
    ) -> Result<Response<ShimStatusResponse>, Status> {
        let container_id = request.into_inner().container_id;
        let Some(runtime) = self.state.runtime_service.as_ref() else {
            return Ok(Response::new(ShimStatusResponse { shims: Vec::new() }));
        };
        let shims = runtime
            .shim_diagnostics((!container_id.is_empty()).then_some(container_id.as_str()))
            .await
            .map_err(|err| Status::internal(redact_host_paths(&err)))?
            .into_iter()
            .map(|shim| ShimInfo {
                container_id: shim.container_id,
                pid: i64::from(shim.pid),
                task_socket: shim.task_socket,
                attach_socket: shim.attach_socket,
                state: shim.state,
                error: shim.error.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(ShimStatusResponse { shims }))
    }

    type ContainerLogStream = ReceiverStream<Result<ContainerLogChunk, Status>>;

    async fn container_log(
        &self,
        request: Request<ContainerLogRequest>,
    ) -> Result<Response<Self::ContainerLogStream>, Status> {
        let request = request.into_inner();
        if request.container_id.trim().is_empty() {
            return Err(Status::invalid_argument("container_id must not be empty"));
        }
        let Some(runtime) = self.state.runtime_service.as_ref() else {
            return Err(Status::failed_precondition(
                "runtime diagnostics state is not available",
            ));
        };
        let log_path = runtime.container_log_path(&request.container_id).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            if let Err(status) = stream_container_log(log_path, request, tx.clone()).await {
                let _ = tx.send(Err(status)).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn nri_status(
        &self,
        _request: Request<NriStatusRequest>,
    ) -> Result<Response<NriStatusResponse>, Status> {
        unimplemented!()
    }

    async fn security_status(
        &self,
        _request: Request<SecurityStatusRequest>,
    ) -> Result<Response<SecurityStatusResponse>, Status> {
        unimplemented!()
    }

    async fn content_gc(
        &self,
        request: Request<ContentGcRequest>,
    ) -> Result<Response<ContentGcResponse>, Status> {
        unimplemented!()
    }
}

async fn stream_container_log(
    log_path: std::path::PathBuf,
    request: ContainerLogRequest,
    tx: tokio::sync::mpsc::Sender<Result<ContainerLogChunk, Status>>,
) -> Result<(), Status> {
    let mut offset = stream_existing_container_log(&log_path, &request, &tx).await?;
    if !request.follow {
        return Ok(());
    }

    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let bytes = match tokio::fs::read(&log_path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(Status::internal(format!(
                    "failed to read container log: {err}"
                )));
            }
        };
        if bytes.len() <= offset {
            continue;
        }
        let appended = &bytes[offset..];
        offset = bytes.len();
        for line in String::from_utf8_lossy(appended).lines() {
            if let Some(chunk) = parse_container_log_line(line, request.timestamps) {
                if chunk.timestamp_unix_nanos >= request.since_unix_nanos
                    && tx.send(Ok(chunk)).await.is_err()
                {
                    return Ok(());
                }
            }
        }
    }
}

async fn stream_existing_container_log(
    log_path: &Path,
    request: &ContainerLogRequest,
    tx: &tokio::sync::mpsc::Sender<Result<ContainerLogChunk, Status>>,
) -> Result<usize, Status> {
    let bytes = tokio::fs::read(log_path)
        .await
        .map_err(|err| Status::not_found(format!("failed to read container log: {err}")))?;
    let mut chunks = String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| parse_container_log_line(line, request.timestamps))
        .filter(|chunk| chunk.timestamp_unix_nanos >= request.since_unix_nanos)
        .collect::<Vec<_>>();
    if request.tail_lines >= 0 {
        let tail = request.tail_lines as usize;
        if tail < chunks.len() {
            chunks = chunks.split_off(chunks.len() - tail);
        }
    }

    for chunk in chunks {
        if tx.send(Ok(chunk)).await.is_err() {
            break;
        }
    }

    Ok(bytes.len())
}

fn parse_container_log_line(line: &str, timestamps: bool) -> Option<ContainerLogChunk> {
    let mut parts = line.splitn(4, ' ');
    let timestamp = parts.next()?;
    let stream = parts.next()?.to_string();
    let _tag = parts.next()?;
    let payload = parts.next().unwrap_or_default();
    let data = if timestamps {
        format!("{timestamp} {payload}\n").into_bytes()
    } else {
        format!("{payload}\n").into_bytes()
    };
    Some(ContainerLogChunk {
        data,
        stream,
        timestamp_unix_nanos: parse_rfc3339_nanos(timestamp).unwrap_or_default(),
    })
}

fn parse_rfc3339_nanos(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|timestamp| timestamp.timestamp_nanos_opt())
}

fn redact_sensitive_config(value: &mut Value, path: &str, redacted_fields: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if is_sensitive_key(key) {
                    *child = Value::String("<redacted>".into());
                    redacted_fields.push(child_path);
                } else {
                    redact_sensitive_config(child, &child_path, redacted_fields);
                }
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter_mut().enumerate() {
                redact_sensitive_config(child, &format!("{path}[{index}]"), redacted_fields);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("auth")
}

fn runtime_handlers_from_config(
    config: &crate::config::Config,
) -> (Vec<RuntimeHandlerInfo>, Vec<String>) {
    match config.runtime.resolved_runtimes() {
        Ok(resolved) => {
            let mut handlers = resolved
                .into_iter()
                .map(|(name, handler)| RuntimeHandlerInfo {
                    name,
                    runtime_type: handler.backend,
                    runtime_path: handler.runtime_path,
                    runtime_config_path: handler.runtime_config_path,
                    features: vec![format!("snapshotter={}", handler.snapshotter)],
                    warnings: Vec::new(),
                })
                .collect::<Vec<_>>();
            handlers.sort_by(|left, right| left.name.cmp(&right.name));
            (handlers, Vec::new())
        }
        Err(err) => (
            Vec::new(),
            vec![format!("failed to resolve runtime handlers: {err}")],
        ),
    }
}

fn last_startup_summary(runtime: &RuntimeServiceImpl) -> String {
    let mut parts = Vec::new();
    if let Some(clean) = runtime.last_startup_clean_shutdown() {
        parts.push(if clean {
            "clean_shutdown"
        } else {
            "unclean_shutdown"
        });
    }

    if parts.is_empty() {
        "unknown".to_string()
    } else {
        parts.join(",")
    }
}

fn redact_host_paths(message: &str) -> String {
    message
        .split_whitespace()
        .map(|part| {
            if part.starts_with('/') {
                "<path>".to_string()
            } else if let Some(index) = part.find('/') {
                let (prefix, _) = part.split_at(index);
                if prefix
                    .chars()
                    .all(|ch| ch.is_ascii_alphabetic() || matches!(ch, ':' | '='))
                {
                    return format!("{prefix}<path>");
                }
                part.to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}