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


use crate::crs::{
    CliContext,
    format::{CommandOutput, TableRow, FormatOptions},
    commands::CliError,
    CommandResult,
    format::render_output,
};

pub(crate) fn render_and_print<T>(
    ctx: &CliContext,
    output: CommandOutput<T>,
) -> Result<CommandResult, CliError>
where
    T: serde::Serialize + TableRow,
{
    let rendered = render_output(&output, FormatOptions::from_context(ctx)).map_err(|source| {
        CliError::internal(format!("failed to render command output: {source}"))
    })?;

    if !rendered.stdout.is_empty() {
        println!("{}", rendered.stdout);
    }
    if !rendered.stderr.is_empty() {
        eprintln!("{}", rendered.stderr);
    }

    Ok(CommandResult::success())
}