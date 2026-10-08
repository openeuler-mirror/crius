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


use crate::proto::runtime::v1::{
    ListContainersRequest, ContainerFilter,
    ContainerStateValue, ContainerState,
    Container, ExecSyncRequest, ContainerStatusRequest,
    StopContainerRequest, RemoveContainerRequest,
};
use crate::crs::{
    CliContext, CrsClient,
    CommandResult,
    commands::{
        CliError, status::render_and_print,
    },
    args::{
        ContainerListArgs, ContainerStateArg,
    },
    format::{
        CommandOutput, ContainerView, InspectView,
        format_unix_nanos, ContainerOperationView,
    },
    parsers::parse_key_value,
};

pub(crate) async fn handle_list(
    ctx: &CliContext,
    client: &CrsClient,
    args: ContainerListArgs,
) -> Result<CommandResult, CliError> {
    let filter = container_filter_from_args(args)?;
    let mut runtime = client.runtime()?;
    let response = client
        .with_rpc_timeout(async {
            runtime
                .list_containers(ListContainersRequest { filter })
                .await
                .map_err(|status| {
                    CliError::from_tonic_status(status)
                        .with_command("crs container list")
                        .with_endpoint(client.endpoint())
                })
        })
        .await?
        .into_inner();

    let views = response
        .containers
        .into_iter()
        .map(container_view)
        .collect();
    render_and_print(
        ctx,
        CommandOutput::new("ContainerList", client.endpoint(), views),
    )
}

pub(crate) fn container_filter_from_args(
    args: ContainerListArgs,
) -> Result<Option<ContainerFilter>, CliError> {
    let labels = args
        .labels
        .iter()
        .map(|label| parse_key_value("--label", label).map(|pair| (pair.key, pair.value)))
        .collect::<Result<std::collections::HashMap<_, _>, _>>()
        .map_err(CliError::invalid_input)?;
    let state = if args.all {
        None
    } else {
        Some(ContainerStateValue {
            state: container_state(args.state.unwrap_or(ContainerStateArg::Running)) as i32,
        })
    };

    if args.id.is_none() && args.pod.is_none() && state.is_none() && labels.is_empty() {
        return Ok(None);
    }

    Ok(Some(ContainerFilter {
        id: args.id.unwrap_or_default(),
        state,
        pod_sandbox_id: args.pod.unwrap_or_default(),
        label_selector: labels,
    }))
}

pub(crate) fn container_state(state: ContainerStateArg) -> ContainerState {
    match state {
        ContainerStateArg::Created => ContainerState::ContainerCreated,
        ContainerStateArg::Running => ContainerState::ContainerRunning,
        ContainerStateArg::Exited => ContainerState::ContainerExited,
        ContainerStateArg::Unknown => ContainerState::ContainerUnknown,
    }
}

pub(crate) fn container_view(container: Container) -> ContainerView {
    let metadata = container.metadata.unwrap_or_default();
    let image = container
        .image
        .map(|image| {
            if image.user_specified_image.is_empty() {
                image.image
            } else {
                image.user_specified_image
            }
        })
        .unwrap_or_default();

    ContainerView {
        container_id: container.id,
        pod: container.pod_sandbox_id,
        image,
        state: container_state_name(container.state).to_string(),
        created: format_unix_nanos(container.created_at, std::time::SystemTime::now()),
        name: metadata.name,
        attempt: metadata.attempt,
    }
}

fn container_state_name(state: i32) -> &'static str {
    match ContainerState::try_from(state).ok() {
        Some(ContainerState::ContainerCreated) => "created",
        Some(ContainerState::ContainerRunning) => "running",
        Some(ContainerState::ContainerExited) => "exited",
        Some(ContainerState::ContainerUnknown) => "unknown",
        None => "unknown",
    }
}

pub(crate) async fn exec_sync_with_command(
    ctx: &CliContext,
    client: &CrsClient,
    args: crate::crs::args::ExecArgs,
    command_name: &'static str,
) -> Result<CommandResult, CliError> {
    let options = crate::crs::streaming::ExecStreamOptions::from_args(
        args.container,
        args.command,
        args.stream,
    )?;
    let mut runtime = client.runtime()?;
    let response = client
        .with_rpc_timeout(async {
            runtime
                .exec_sync(ExecSyncRequest {
                    container_id: options.container_id.clone(),
                    cmd: options.command.clone(),
                    timeout: 0,
                })
                .await
                .map_err(|status| {
                    CliError::from_tonic_status(status)
                        .with_command(command_name)
                        .with_endpoint(client.endpoint())
                        .with_object(format!("container {}", options.container_id))
                })
        })
        .await?
        .into_inner();

    if matches!(ctx.output(), crate::crs::args::OutputArg::Json) {
        let envelope = serde_json::json!({
            "kind": "ContainerExecSync",
            "apiVersion": crate::crs::format::API_VERSION,
            "endpoint": client.endpoint(),
            "summary": {
                "containerId": options.container_id,
                "exitCode": response.exit_code,
            },
            "stdout": String::from_utf8_lossy(&response.stdout),
            "stderr": String::from_utf8_lossy(&response.stderr),
            "warnings": [],
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&envelope).map_err(|source| CliError::internal(
                format!("failed to render exec-sync JSON: {source}")
            ))?
        );
    } else {
        std::io::Write::write_all(&mut std::io::stdout(), &response.stdout)
            .map_err(|source| CliError::internal(format!("failed to write stdout: {source}")))?;
        std::io::Write::write_all(&mut std::io::stderr(), &response.stderr)
            .map_err(|source| CliError::internal(format!("failed to write stderr: {source}")))?;
    }

    Ok(CommandResult::from_code(response.exit_code))
}

pub(crate) async fn handle_inspect(
    ctx: &CliContext,
    client: &CrsClient,
    id: String,
) -> Result<CommandResult, CliError> {
    let mut runtime = client.runtime()?;
    let response = client
        .with_rpc_timeout(async {
            runtime
                .container_status(ContainerStatusRequest {
                    container_id: id.clone(),
                    verbose: true,
                })
                .await
                .map_err(|status| {
                    CliError::from_tonic_status(status)
                        .with_command("crs inspect")
                        .with_endpoint(client.endpoint())
                        .with_object(format!("container {id}"))
                })
        })
        .await?
        .into_inner();

    let id = response
        .status
        .as_ref()
        .map(|status| status.id.clone())
        .unwrap_or_else(|| id.clone());

    let status = response.status.as_ref();
    let response_json = serde_json::json!({
        "status": {
            "id": status.map(|s| s.id.clone()).unwrap_or_default(),
            "state": status.map(|s| s.state).unwrap_or_default(),
            "createdAt": status.map(|s| s.created_at).unwrap_or_default(),
            "startedAt": status.map(|s| s.started_at).unwrap_or_default(),
            "finishedAt": status.map(|s| s.finished_at).unwrap_or_default(),
            "imageRef": status.map(|s| s.image_ref.clone()).unwrap_or_default(),
            "metadata": status.and_then(|s| s.metadata.as_ref()).map(|m| {
                serde_json::json!({
                    "name": m.name,
                    "attempt": m.attempt,
                })
            }),
            "image": status.and_then(|s| s.image.as_ref()).map(|i| {
                serde_json::json!({
                    "image": i.image,
                })
            }),
        },
    });
    let info_json = serde_json::to_value(&response.info)
        .unwrap_or(serde_json::Value::Null);

    render_and_print(
        ctx,
        CommandOutput::new(
            "ContainerInspect",
            client.endpoint(),
            vec![InspectView {
                object_type: "container".to_string(),
                id,
                response: response_json,
                info_json: info_json.clone(),
                info_raw: info_json,
            }],
        ),
    )
}

struct ContainerOperationRender {
    kind: &'static str,
    pod_id: String,
    container_id: String,
    image: String,
    action: &'static str,
    summary: serde_json::Value,
}

fn ensure_container_id(id: &str, command_name: &'static str) -> Result<(), CliError> {
    if id.is_empty() {
        return Err(
            CliError::invalid_input("container ID must not be empty").with_command(command_name)
        );
    }
    Ok(())
}

fn container_status_error(
    status: tonic::Status,
    client: &CrsClient,
    command_name: &'static str,
    id: &str,
) -> CliError {
    CliError::from_tonic_status(status)
        .with_command(command_name)
        .with_endpoint(client.endpoint())
        .with_object(format!("container {id}"))
}

fn render_container_operation(
    ctx: &CliContext,
    client: &CrsClient,
    operation: ContainerOperationRender,
) -> Result<CommandResult, CliError> {
    render_and_print(
        ctx,
        CommandOutput::new(
            operation.kind,
            client.endpoint(),
            vec![ContainerOperationView {
                container_id: operation.container_id,
                pod_id: operation.pod_id,
                image: operation.image,
                action: operation.action.to_string(),
                success: true,
            }],
        )
        .with_summary(operation.summary),
    )
}

pub(crate) async fn handle_stop(
    ctx: &CliContext,
    client: &CrsClient,
    id: String,
    timeout: Option<u32>,
) -> Result<CommandResult, CliError> {
    ensure_container_id(&id, "crs container stop")?;
    let mut runtime = client.runtime()?;
    client
        .with_rpc_timeout(async {
            runtime
                .stop_container(StopContainerRequest {
                    container_id: id.clone(),
                    timeout: timeout.map(i64::from).unwrap_or_default(),
                })
                .await
                .map_err(|status| container_status_error(status, client, "crs container stop", &id))
        })
        .await?;

    render_container_operation(
        ctx,
        client,
        ContainerOperationRender {
            kind: "ContainerStop",
            container_id: id.clone(),
            pod_id: String::new(),
            image: String::new(),
            action: "stopped",
            summary: serde_json::json!({
                "containerId": id,
                "stopped": true,
                "timeoutSeconds": timeout.unwrap_or_default(),
            }),
        },
    )
}

pub(crate) async fn handle_remove_with_command(
    ctx: &CliContext,
    client: &CrsClient,
    id: String,
    command_name: &'static str,
) -> Result<CommandResult, CliError> {
    ensure_container_id(&id, command_name)?;
    let mut runtime = client.runtime()?;
    client
        .with_rpc_timeout(async {
            runtime
                .remove_container(RemoveContainerRequest {
                    container_id: id.clone(),
                })
                .await
                .map_err(|status| container_status_error(status, client, command_name, &id))
        })
        .await?;

    render_container_operation(
        ctx,
        client,
        ContainerOperationRender {
            kind: "ContainerRemove",
            container_id: id.clone(),
            pod_id: String::new(),
            image: String::new(),
            action: "removed",
            summary: serde_json::json!({
                "containerId": id,
                "removed": true,
            }),
        },
    )
}