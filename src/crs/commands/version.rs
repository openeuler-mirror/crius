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
    args::VersionArgs,
    client::CrsClient,
    context::CliContext,
    error::CliError,
    error::CommandResult,
    format::{render_output, CommandOutput, FormatOptions, RuntimeVersionView},
};
use crate::proto::runtime::v1::VersionRequest;

pub(crate) async fn handle(
    ctx: &CliContext,
    client: &CrsClient,
    _args: VersionArgs,
) -> Result<CommandResult, CliError> {
    let mut runtime = client.runtime()?;
    let response = client
        .with_rpc_timeout(async {
            runtime
                .version(VersionRequest {
                    version: String::new(),
                })
                .await
                .map_err(CliError::from_tonic_status)
        })
        .await?
        .into_inner();

    let view = RuntimeVersionView {
        runtime_name: response.runtime_name,
        runtime_version: response.runtime_version,
        runtime_api_version: response.runtime_api_version,
    };
    let output = CommandOutput::new("RuntimeVersion", client.endpoint(), vec![view]);
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
