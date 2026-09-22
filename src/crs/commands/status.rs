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


use std::collections::HashMap;

use serde_json::{Value, Map,};

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

pub(crate) fn parse_info_map(
    info: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> (Value, Value) {
    let mut parsed = Map::new();
    let mut raw = Map::new();

    for (key, value) in info {
        raw.insert(key.clone(), Value::String(value.clone()));
        match serde_json::from_str::<Value>(value) {
            Ok(json) => {
                parsed.insert(key.clone(), json);
            }
            Err(source) => warnings.push(format!(
                "failed to parse verbose info field {key:?} as JSON: {source}"
            )),
        }
    }

    (Value::Object(parsed), Value::Object(raw))
}