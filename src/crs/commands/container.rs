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
    Container,
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
        CommandOutput, ContainerView,
        format_unix_nanos,
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