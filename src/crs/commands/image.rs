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

use crate::proto::runtime::v1::{
    ListImagesRequest, ImageFilter, 
    ImageSpec, Image, PullImageRequest,
    PodSandboxStatusRequest,
};
use crate::crs::{
    CliContext, CrsClient,
    args::ImageListArgs,
    CommandResult, commands::CliError,
    format::ImageView,
    format::{CommandOutput, ImageOperationView},
    commands::status::render_and_print,
    builders::build_auth_config,
};

pub(crate) async fn handle_list(
    ctx: &CliContext,
    client: &CrsClient,
    args: ImageListArgs,
) -> Result<CommandResult, CliError> {
    let mut image_client = client.image()?;
    let response = client
        .with_rpc_timeout(async {
            image_client
                .list_images(ListImagesRequest {
                    filter: args.image.map(|image| ImageFilter {
                        image: Some(ImageSpec {
                            image,
                            ..Default::default()
                        }),
                    }),
                })
                .await
                .map_err(|status| {
                    CliError::from_tonic_status(status)
                        .with_command("crs image list")
                        .with_endpoint(client.endpoint())
                })
        })
        .await?
        .into_inner();

    let views = response.images.into_iter().map(image_view).collect();
    render_and_print(
        ctx,
        CommandOutput::new("ImageList", client.endpoint(), views),
    )
}

pub(crate) async fn handle_pull(
    ctx: &CliContext,
    client: &CrsClient,
    args: crate::crs::args::ImagePullArgs,
    command_name: &'static str,
) -> Result<CommandResult, CliError> {
    if args.image.is_empty() {
        return Err(CliError::invalid_input("image must not be empty").with_command(command_name));
    }

    let auth = build_auth_config(&args.auth)
        .map_err(CliError::invalid_input)?
        .filter(|auth| !auth_is_empty(auth));
    let sandbox_config = if let Some(pod) = args.pod.as_deref() {
        Some(fetch_sandbox_config(client, pod, command_name).await?)
    } else {
        None
    };

    let mut image_client = client.image()?;
    let request = PullImageRequest {
        image: Some(ImageSpec {
            image: args.image.clone(),
            user_specified_image: args.image.clone(),
            ..Default::default()
        }),
        auth,
        sandbox_config,
    };
    let response = client
        .with_rpc_timeout(async {
            image_client.pull_image(request).await.map_err(|status| {
                CliError::from_tonic_status(status)
                    .with_command(command_name)
                    .with_endpoint(client.endpoint())
                    .with_object(format!("image {}", args.image))
            })
        })
        .await?
        .into_inner();

    let view = ImageOperationView {
        image: args.image.clone(),
        image_ref: response.image_ref,
        action: "pulled".to_string(),
        success: true,
    };
    render_and_print(
        ctx,
        CommandOutput::new("ImagePull", client.endpoint(), vec![view]).with_summary(
            serde_json::json!({
                "image": args.image,
                "pulled": true,
            }),
        ),
    )
}

pub(crate) fn image_view(image: Image) -> ImageView {
    let image_name = image
        .repo_tags
        .first()
        .or_else(|| image.repo_digests.first())
        .cloned()
        .or_else(|| image.spec.as_ref().map(|spec| spec.image.clone()))
        .unwrap_or_default();
    let user_spec = image
        .spec
        .as_ref()
        .map(|spec| spec.user_specified_image.clone())
        .unwrap_or_default();

    ImageView {
        image: image_name,
        image_id: image.id,
        size_bytes: image.size,
        user_spec,
        pinned: image.pinned,
    }
}

fn auth_is_empty(auth: &crate::proto::runtime::v1::AuthConfig) -> bool {
    auth.username.is_empty()
        && auth.password.is_empty()
        && auth.auth.is_empty()
        && auth.server_address.is_empty()
        && auth.identity_token.is_empty()
        && auth.registry_token.is_empty()
}

async fn fetch_sandbox_config(
    client: &CrsClient,
    pod: &str,
    command_name: &'static str,
) -> Result<crate::proto::runtime::v1::PodSandboxConfig, CliError> {
    let mut runtime = client.runtime()?;
    let response = client
        .with_rpc_timeout(async {
            runtime
                .pod_sandbox_status(PodSandboxStatusRequest {
                    pod_sandbox_id: pod.to_string(),
                    verbose: false,
                })
                .await
                .map_err(|status| {
                    CliError::from_tonic_status(status)
                        .with_command(command_name)
                        .with_endpoint(client.endpoint())
                        .with_object(format!("pod {pod}"))
                })
        })
        .await?
        .into_inner();

    response
        .status
        .and_then(|status| status.metadata)
        .map(|metadata| crate::proto::runtime::v1::PodSandboxConfig {
            metadata: Some(metadata),
            ..Default::default()
        })
        .ok_or_else(|| {
            CliError::invalid_input(format!(
                "daemon did not return sandbox metadata for pod {pod}"
            ))
            .with_command(command_name)
            .with_object(format!("pod {pod}"))
        })
}