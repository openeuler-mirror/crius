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

use crate::proto::runtime::v1::{ListImagesRequest, ImageFilter, ImageSpec, Image};
use crate::crs::{
    CliContext, CrsClient,
    args::ImageListArgs,
    CommandResult, commands::CliError,
    format::ImageView,
    format::CommandOutput,
    commands::status::render_and_print,
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