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

use std::unimplemented;

use crate::crs::{
    args::Command, client::CrsClient, context::CliContext, error::{CliError, CommandResult},
};


pub(crate) async fn dispatch(
    ctx: &CliContext,
    client: &CrsClient,
    command: Command,
) -> Result<CommandResult, CliError> {
    match command {
        Command::Version(args) => unimplemented!(),
        Command::Images(args) => shortcuts::handle_images(ctx, client, args).await,
        Command::Pull(args) => unimplemented!(),
        Command::Rmi { image: image_name } => unimplemented!(),
        Command::Image(args) => unimplemented!(),
        Command::Inspect(args) => unimplemented!(),
        Command::Debug(args) => unimplemented!(),
        Command::Completion(args) => unimplemented!(),
    }
}