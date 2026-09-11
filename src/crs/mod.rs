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

pub mod args;
pub mod error;

pub(crate) mod client;
pub(crate) mod context;
pub(crate) mod parsers;
pub(crate) mod commands;
pub(crate) mod format;
pub(crate) mod ids;

use std::ffi::OsString;

use clap::error::ErrorKind;
use clap::Parser;

use crate::crs::{
    args::{Args, Command},
    client::CrsClient,
    context::CliContext,
    error::{CommandResult, ExitStatus},
};

pub async fn run_cli<I, T>(args: I) -> CommandResult
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args = match Args::try_parse_from(args) {
        Ok(args) => args,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return CommandResult::success();
        }
        Err(error) => {
            let _ = error.print();
            return CommandResult::failure(ExitStatus::Usage);
        }
    };

    let ctx = match CliContext::from_args(&args) {
        Ok(ctx) => ctx,
        Err(error) => {
            eprintln!("error: {error}");
            return CommandResult::failure(ExitStatus::Usage);
        }
    };

    let client = if matches!(args.command, Command::Completion(_)) {
        CrsClient::new(&ctx)
    } else {
        match CrsClient::connect(&ctx).await {
            Ok(client) => client,
            Err(error) => {
                error.render(ctx.output());
                return CommandResult::failure(error.exit_status());
            }
        }
    };

    match commands::dispatch(&ctx, &client, args.command).await {
        Ok(result) => result,
        Err(error) => {
            error.render(ctx.output());
            CommandResult::failure(error.exit_status())
        }
    }
}