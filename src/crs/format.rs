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


use std::time::{
    Duration, UNIX_EPOCH, SystemTime
};

use serde_json::{json, Value};
use serde::Serialize;

use crate::crs::{
    CliContext,
    args::OutputArg,
    ids::{truncate_field, short_image_id},
};

pub(crate) const API_VERSION: &str = "crius.io/crs/v1";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageView {
    pub image: String,
    pub image_id: String,
    pub size_bytes: u64,
    pub user_spec: String,
    pub pinned: bool,
}

impl TableRow for ImageView {
    fn headers() -> &'static [&'static str] {
        &["REPOSITORY", "TAG", "IMAGE ID", "SIZE"]
    }

    fn cells(&self) -> Vec<String> {
        let (repository, tag) = split_image_reference(&self.image);
        vec![
            repository,
            tag,
            short_image_id(&self.image_id),
            format_bytes(self.size_bytes),
        ]
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandOutput<T>
where
    T: Serialize,
{
    pub kind: &'static str,
    pub api_version: &'static str,
    pub endpoint: String,
    pub items: Vec<T>,
    pub summary: Value,
    pub warnings: Vec<String>,
}

impl<T> CommandOutput<T>
where
    T: Serialize,
{
    pub(crate) fn new(kind: &'static str, endpoint: impl Into<String>, items: Vec<T>) -> Self {
        let count = items.len();
        Self {
            kind,
            api_version: API_VERSION,
            endpoint: endpoint.into(),
            items,
            summary: json!({ "count": count }),
            warnings: Vec::new(),
        }
    }

    pub(crate) fn with_summary(mut self, summary: Value) -> Self {
        self.summary = summary;
        self
    }

    pub(crate) fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FormatOptions {
    output: OutputArg,
    quiet: bool,
    no_trunc: bool,
}

impl FormatOptions {
    pub(crate) fn from_context(ctx: &CliContext) -> Self {
        Self {
            output: ctx.output(),
            quiet: ctx.quiet(),
            no_trunc: ctx.no_trunc(),
        }
    }

    pub(crate) fn output(self) -> OutputArg {
        self.output
    }

    pub(crate) fn quiet(self) -> bool {
        self.quiet
    }

    pub(crate) fn no_trunc(self) -> bool {
        self.no_trunc
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenderedOutput {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageOperationView {
    pub image: String,
    pub image_ref: String,
    pub action: String,
    pub success: bool,
}

impl TableRow for ImageOperationView {
    fn headers() -> &'static [&'static str] {
        &["IMAGE", "IMAGE REF", "ACTION", "SUCCESS"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.image.clone(),
            self.image_ref.clone(),
            self.action.clone(),
            format_bool(self.success).to_string(),
        ]
    }

    fn quiet_cell(&self) -> String {
        if self.image_ref.is_empty() {
            self.image.clone()
        } else {
            self.image_ref.clone()
        }
    }
}

pub(crate) trait TableRow {
    fn headers() -> &'static [&'static str]
    where
        Self: Sized;
    fn cells(&self) -> Vec<String>;
    fn table_cells(&self, _no_trunc: bool) -> Vec<String> {
        self.cells()
    }
    fn quiet_cell(&self) -> String {
        self.cells().into_iter().next().unwrap_or_default()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilesystemUsageView {
    pub kind: String,
    pub mountpoint: String,
    pub used_bytes: u64,
    pub inodes_used: u64,
    pub timestamp: i64,
}

impl TableRow for FilesystemUsageView {
    fn headers() -> &'static [&'static str] {
        &["KIND", "MOUNTPOINT", "USED", "INODES", "TIMESTAMP"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.kind.clone(),
            self.mountpoint.clone(),
            format_bytes(self.used_bytes),
            self.inodes_used.to_string(),
            self.timestamp.to_string(),
        ]
    }
}

pub(crate) fn render_output<T>(
    output: &CommandOutput<T>,
    options: FormatOptions,
) -> Result<RenderedOutput, serde_json::Error>
where
    T: Serialize + TableRow,
{
    let stdout = match options.output() {
        OutputArg::Json => print_envelope(output)?,
        OutputArg::Table | OutputArg::Text if options.quiet() => {
            print_quiet(&output.items, options.no_trunc())
        }
        OutputArg::Table | OutputArg::Text => print_table(&output.items, options.no_trunc()),
    };

    let stderr = render_warnings(&output.warnings, options.output()).unwrap_or_default();

    Ok(RenderedOutput { stdout, stderr })
}

pub(crate) fn render_warnings(warnings: &[String], output: OutputArg) -> Option<String> {
    match output {
        OutputArg::Json => None,
        OutputArg::Table | OutputArg::Text if warnings.is_empty() => None,
        OutputArg::Table | OutputArg::Text => Some(
            warnings
                .iter()
                .map(|warning| format!("warning: {warning}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    }
}

pub(crate) fn print_envelope<T>(output: &CommandOutput<T>) -> Result<String, serde_json::Error>
where
    T: Serialize,
{
    serde_json::to_string_pretty(output)
}

pub(crate) fn print_quiet<T>(items: &[T], no_trunc: bool) -> String
where
    T: TableRow,
{
    items
        .iter()
        .map(|item| truncate_field(&normalize_cell(&item.quiet_cell()), no_trunc))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn print_table<T>(items: &[T], no_trunc: bool) -> String
where
    T: TableRow,
{
    let headers = T::headers();
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            item.table_cells(no_trunc)
                .into_iter()
                .map(|cell| truncate_field(&normalize_cell(&cell), no_trunc))
                .collect()
        })
        .collect();

    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter()
                .filter_map(|row| row.get(index))
                .map(|cell| cell.len())
                .max()
                .unwrap_or(0)
                .max(header.len())
        })
        .collect();

    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(format_row(
        &headers
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
        &widths,
    ));
    lines.extend(rows.iter().map(|row| format_row(row, &widths)));
    lines.join("\n")
}

fn format_row(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(index, cell)| format!("{cell:<width$}", width = widths[index]))
        .collect::<Vec<_>>()
        .join("  ")
        .trim_end()
        .to_string()
}

pub(crate) fn normalize_cell(value: &str) -> String {
    let value = value.replace(['\n', '\r'], " ");
    if value.is_empty() {
        "-".to_string()
    } else {
        value
    }
}

fn split_image_reference(image: &str) -> (String, String) {
    let image = image.trim();
    if image.is_empty() {
        return ("<none>".to_string(), "<none>".to_string());
    }

    let repository = image
        .split_once('@')
        .map(|(repository, _digest)| repository)
        .unwrap_or(image);
    let last_slash = repository.rfind('/');
    let tag_separator = repository
        .rfind(':')
        .filter(|index| last_slash.is_none_or(|slash| *index > slash));

    match tag_separator {
        Some(index) if index + 1 < repository.len() => (
            repository[..index].to_string(),
            repository[index + 1..].to_string(),
        ),
        _ => (repository.to_string(), "<none>".to_string()),
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes}B")
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

pub(crate) fn format_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InspectView {
    pub object_type: String,
    pub id: String,
    pub response: Value,
    pub info_json: Value,
    pub info_raw: Value,
}

impl TableRow for InspectView {
    fn headers() -> &'static [&'static str] {
        &["TYPE", "ID", "NAME", "STATE", "IMAGE"]
    }

    fn cells(&self) -> Vec<String> {
        let status = self.response.get("status");
        vec![
            self.object_type.clone(),
            self.id.clone(),
            string_pointer(status, &["/metadata/name"]).unwrap_or_default(),
            string_pointer(status, &["/state"]).unwrap_or_default(),
            string_pointer(status, &["/image/image", "/imageRef"]).unwrap_or_default(),
        ]
    }

    fn quiet_cell(&self) -> String {
        self.id.clone()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageTransferView {
    pub image: String,
    pub status: String,
    pub updated: String,
    pub error: String,
}

impl TableRow for ImageTransferView {
    fn headers() -> &'static [&'static str] {
        &["IMAGE", "STATUS", "UPDATED", "ERROR"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.image.clone(),
            self.status.clone(),
            self.updated.clone(),
            self.error.clone(),
        ]
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigReloadStatusView {
    pub watcher: String,
    pub last_reload: String,
    pub last_error: String,
    pub cni_watcher: String,
}

impl TableRow for ConfigReloadStatusView {
    fn headers() -> &'static [&'static str] {
        &["WATCHER", "LAST RELOAD", "LAST ERROR", "CNI WATCHER"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.watcher.clone(),
            self.last_reload.clone(),
            self.last_error.clone(),
            self.cni_watcher.clone(),
        ]
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EffectiveConfigView {
    pub config: Value,
    pub redacted_fields: Vec<String>,
}

impl TableRow for EffectiveConfigView {
    fn headers() -> &'static [&'static str] {
        &["REDACTED FIELDS", "CONFIG KEYS"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.redacted_fields.join(","),
            value_object_keys(&self.config).join(","),
        ]
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageConfigView {
    pub snapshotter: String,
    pub policy: String,
    pub auth_configured: String,
    pub pinned_images: String,
    pub config: Value,
}

impl TableRow for ImageConfigView {
    fn headers() -> &'static [&'static str] {
        &["SNAPSHOTTER", "POLICY", "AUTH", "PINNED IMAGES"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.snapshotter.clone(),
            self.policy.clone(),
            self.auth_configured.clone(),
            self.pinned_images.clone(),
        ]
    }
}

fn string_pointer(value: Option<&Value>, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        value
            .and_then(|value| value.pointer(path))
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

pub(crate) fn format_unix_nanos(unix_nanos: i64, now: SystemTime) -> String {
    let timestamp = if unix_nanos >= 0 {
        UNIX_EPOCH + Duration::from_nanos(unix_nanos as u64)
    } else {
        UNIX_EPOCH
    };

    let age = now.duration_since(timestamp).unwrap_or_default();
    format!("{} ago", format_human_duration(age))
}

fn format_human_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();

    if seconds < 1 {
        "Less than a second".to_string()
    } else if seconds == 1 {
        "1 second".to_string()
    } else if seconds < 60 {
        format!("{seconds} seconds")
    } else {
        let minutes = seconds / 60;
        if minutes == 1 {
            "About a minute".to_string()
        } else if minutes < 60 {
            format!("{minutes} minutes")
        } else {
            let hours = ((duration.as_secs_f64() / 3_600.0) + 0.5) as u64;
            if hours == 1 {
                "About an hour".to_string()
            } else if hours < 48 {
                format!("{hours} hours")
            } else if hours < 24 * 7 * 2 {
                format!("{} days", hours / 24)
            } else if hours < 24 * 30 * 2 {
                format!("{} weeks", hours / 24 / 7)
            } else if hours < 24 * 365 * 2 {
                format!("{} months", hours / 24 / 30)
            } else {
                format!("{} years", hours / 24 / 365)
            }
        }
    }
}

fn value_object_keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}