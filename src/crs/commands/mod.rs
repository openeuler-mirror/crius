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

pub(crate) mod shortcuts;
pub(crate) mod image;
pub(crate) mod status;
pub(crate) mod version;
pub(crate) mod run;
pub(crate) mod container;
pub(crate) mod logs;
pub(crate) mod exec;

use crate::crs::{
    args::{self, Command}, client::CrsClient, context::CliContext, error::{CliError, CommandResult},
};


pub(crate) async fn dispatch(
    ctx: &CliContext,
    client: &CrsClient,
    command: Command,
) -> Result<CommandResult, CliError> {
    match command {
        Command::Version(args) => version::handle(ctx, client, args).await,
        Command::Status(args) => status::handle(ctx, client, args).await,
        Command::Images(args) => shortcuts::handle_images(ctx, client, args).await,
        Command::Run(args) => run::handle(ctx, client, *args).await,
        Command::Ps(args) => shortcuts::handle_ps(ctx, client, args).await,
        Command::Exec(args) => exec::handle(ctx, client, args).await,
        Command::Stop(args) => shortcuts::handle_stop(ctx, client, args).await,
        Command::Rm(args) => shortcuts::handle_rm(ctx, client, args).await,
        Command::Logs(args) => logs::handle(ctx, client, args).await,
        Command::Pull(args) => shortcuts::handle_pull(ctx, client, args).await,
        Command::Rmi { image: image_name } => {
            image::handle_remove_with_command(ctx, client, image_name, "crs rmi").await
        }
        Command::Image(args) => image::handle(ctx, client, args).await,
        Command::Container(args) => match args.command {
            command => container::handle(ctx, client, command).await,
        }
        Command::Inspect(args) => shortcuts::handle_inspect(ctx, client, args).await,
    }
}