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

use crate::crs::commands::image;
use crate::crs::{
    CliContext, CrsClient,
    args::{
        ImageListArgs, ImagePullArgs, 
        InspectArgs, ObjectType,
    },
    CommandResult,commands::CliError
};

pub(crate) async fn handle_images(
    ctx: &CliContext,
    client: &CrsClient,
    args: ImageListArgs,
) -> Result<CommandResult, CliError> {
    image::handle_list(ctx, client, args).await
}

pub(crate) async fn handle_pull(
    ctx: &CliContext,
    client: &CrsClient,
    args: ImagePullArgs,
) -> Result<CommandResult, CliError> {
    image::handle_pull(ctx, client, args, "crs pull").await
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum InspectCandidate {
    Container,
    Pod,
    Image,
}

async fn resolve_inspect_target(
    client: &CrsClient,
    target: &str,
) -> Result<InspectCandidate, CliError> {
    let mut candidates = Vec::new();
    if container_exists(client, target, "crs inspect").await? {
        candidates.push(InspectCandidate::Container);
    }
    // if pod_exists(client, target, "crs inspect").await? {
    //     candidates.push(InspectCandidate::Pod);
    // }
    if image_exists(client, target, "crs inspect").await? {
        candidates.push(InspectCandidate::Image);
    }

    match candidates.as_slice() {
        [candidate] => Ok(*candidate),
        [] => Err(CliError::invalid_input(format!(
            "target {target} did not match a container, pod, or image; use --type container|pod|image with a valid ID or image reference"
        ))
        .with_command("crs inspect")
        .with_object(target.to_string())),
        [_, ..] => Err(CliError::invalid_input(format!(
            "target {target} is ambiguous; specify --type container|pod|image"
        ))
        .with_command("crs inspect")
        .with_object(target.to_string())),
    }
}

async fn container_exists(
    client: &CrsClient,
    target: &str,
    command_name: &'static str,
) -> Result<bool, CliError> {
    let mut runtime = client.runtime()?;
    client
        .with_rpc_timeout(async {
            match runtime
                .container_status(crate::proto::runtime::v1::ContainerStatusRequest {
                    container_id: target.to_string(),
                    verbose: false,
                })
                .await
            {
                Ok(response) => Ok(response.into_inner().status.is_some()),
                Err(status) if status.code() == tonic::Code::NotFound => Ok(false),
                Err(status) => Err(candidate_error(
                    status,
                    client,
                    command_name,
                    "container",
                    target,
                )),
            }
        })
        .await
}

async fn image_exists(
    client: &CrsClient,
    target: &str,
    command_name: &'static str,
) -> Result<bool, CliError> {
    let mut image_client = client.image()?;
    client
        .with_rpc_timeout(async {
            match image_client
                .image_status(crate::proto::runtime::v1::ImageStatusRequest {
                    image: Some(crate::proto::runtime::v1::ImageSpec {
                        image: target.to_string(),
                        ..Default::default()
                    }),
                    verbose: false,
                })
                .await
            {
                Ok(response) => Ok(response.into_inner().image.is_some()),
                Err(status) if status.code() == tonic::Code::NotFound => Ok(false),
                Err(status) => Err(candidate_error(
                    status,
                    client,
                    command_name,
                    "image",
                    target,
                )),
            }
        })
        .await
}

pub(crate) async fn handle_inspect(
    ctx: &CliContext,
    client: &CrsClient,
    args: InspectArgs,
) -> Result<CommandResult, CliError> {
    match args.object_type {
        Some(ObjectType::Container) => unimplemented!(),
        Some(ObjectType::Pod) => unimplemented!(),
        Some(ObjectType::Image) => image::handle_inspect(ctx, client, args.target).await,
        None => match resolve_inspect_target(client, &args.target).await? {
            InspectCandidate::Container => {
                unimplemented!()
            }
            InspectCandidate::Pod => unimplemented!(),
            InspectCandidate::Image => image::handle_inspect(ctx, client, args.target).await,
        },
    }
}

fn candidate_error(
    status: tonic::Status,
    client: &CrsClient,
    command_name: &'static str,
    object_type: &'static str,
    target: &str,
) -> CliError {
    CliError::from_tonic_status(status)
        .with_command(command_name)
        .with_endpoint(client.endpoint())
        .with_object(format!("{object_type} {target}"))
}

fn not_found_error(
    client: &CrsClient,
    command_name: &'static str,
    object_type: &'static str,
    target: &str,
) -> CliError {
    CliError::from_tonic_status(tonic::Status::not_found(format!(
        "{object_type} {target} not found"
    )))
    .with_command(command_name)
    .with_endpoint(client.endpoint())
    .with_object(format!("{object_type} {target}"))
}