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


use crate::crs::commands::image;

use crate::crs::{
    CliContext, CrsClient,
    args::{ImageListArgs, ImagePullArgs},
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