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

use serde::Serialize;
use tonic::Code;
use serde_json::json;

use crate::crs::args::OutputArg;


const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "token",
    "identity-token",
    "identity_token",
    "identitytoken",
    "registry-token",
    "registry_token",
    "registrytoken",
    "auth-json",
    "auth_json",
    "auth",
    "authorization",
    "secret",
    "client-secret",
    "client_secret",
];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ExitStatus {
    Success,
    General,
    Usage,
    NotFound,
    AlreadyExists,
    FailedPrecondition,
    PermissionDenied,
    Timeout,
    Unavailable,
    Interrupted,
}

impl ExitStatus {
    pub fn code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::General => 1,
            Self::Usage => 2,
            Self::NotFound => 4,
            Self::AlreadyExists => 5,
            Self::FailedPrecondition => 6,
            Self::PermissionDenied => 13,
            Self::Timeout => 124,
            Self::Unavailable => 125,
            Self::Interrupted => 130,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandResult {
    Exit { code: i32 },
    ContainerExit { code: i32 },
}

impl CommandResult {
    pub fn success() -> Self {
        Self::Exit {
            code: ExitStatus::Success.code(),
        }
    }

    pub fn failure(status: ExitStatus) -> Self {
        Self::Exit {
            code: status.code(),
        }
    }

    pub fn code(self) -> i32 {
        match self {
            Self::Exit { code } | Self::ContainerExit { code } => code,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ErrorContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

impl ErrorContext {
    fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CliError {
    #[error("{command} is not implemented yet")]
    NotImplemented {
        command: String,
        context: Box<ErrorContext>,
    },
    #[error("{message} for endpoint {endpoint}")]
    Timeout {
        message: String,
        endpoint: String,
        context: Box<ErrorContext>,
    },
    #[error("diagnostics service is not available from this crius daemon at {endpoint}")]
    DiagnosticsUnavailable {
        endpoint: String,
        context: Box<ErrorContext>,
    },
    #[error("daemon is unavailable at {endpoint}: {message}")]
    DaemonUnavailable {
        endpoint: String,
        message: String,
        context: Box<ErrorContext>,
    },
    #[error("daemon returned {code:?}: {message}")]
    Grpc {
        code: Code,
        message: String,
        context: Box<ErrorContext>,
    },
    #[error("operation interrupted")]
    Interrupted { context: Box<ErrorContext> },
    #[error("{message}")]
    InvalidInput {
        message: String,
        context: Box<ErrorContext>,
    },
    #[error("{message}")]
    Internal {
        message: String,
        context: Box<ErrorContext>,
    },
}


impl CliError {
    fn context(&self) -> &ErrorContext {
        match self {
            Self::NotImplemented { context, .. }
            | Self::Timeout { context, .. }
            | Self::DiagnosticsUnavailable { context, .. }
            | Self::DaemonUnavailable { context, .. }
            | Self::Grpc { context, .. }
            | Self::Interrupted { context }
            | Self::InvalidInput { context, .. }
            | Self::Internal { context, .. } => context,
        }
    }

    pub(crate) fn timeout(message: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let endpoint = endpoint.into();
        Self::Timeout {
            message: message.into(),
            context: Box::new(ErrorContext::default().with_endpoint(endpoint.clone())),
            endpoint,
        }
    }

    pub(crate) fn daemon_unavailable(
        endpoint: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let endpoint = endpoint.into();
        Self::DaemonUnavailable {
            context: Box::new(ErrorContext::default().with_endpoint(endpoint.clone())),
            endpoint,
            message: message.into(),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
            context: Box::default(),
        }
    }

    pub(crate) fn render(&self, output: OutputArg) {
        eprintln!("{}", self.render_to_string(output));
    }

    pub(crate) fn render_to_string(&self, output: OutputArg) -> String {
        let message = redact_sensitive(&self.to_string());
        match output {
            OutputArg::Json => json!({
                "error": {
                    "message": message,
                    "exitCode": self.exit_status().code(),
                    "grpcCode": self.grpc_code_name(),
                    "daemonMessage": self.daemon_message(),
                    "context": self.redacted_context_json(),
                    "suggestion": self.suggestion(),
                }
            })
            .to_string(),
            OutputArg::Table | OutputArg::Text => self.render_text_error(&message),
        }
    }

     fn render_text_error(&self, message: &str) -> String {
        let mut lines = vec![format!("error: {message}")];
        let context = self.redacted_context();
        if let Some(command) = context.command {
            lines.push(format!("  command: {command}"));
        }
        if let Some(endpoint) = context.endpoint {
            lines.push(format!("  endpoint: {endpoint}"));
        }
        if let Some(object) = context.object {
            lines.push(format!("  object: {object}"));
        }
        if let Some(grpc_code) = self.grpc_code_name() {
            lines.push(format!("  gRPC code: {grpc_code}"));
        }
        if let Some(daemon_message) = self.daemon_message() {
            lines.push(format!(
                "  daemon message: {}",
                redact_sensitive(&daemon_message)
            ));
        }
        if let Some(source) = context.source {
            lines.push(format!("  source: {source}"));
        }
        if let Some(suggestion) = self.suggestion() {
            lines.push(format!("  next step: {suggestion}"));
        }
        lines.join("\n")
    }

    fn redacted_context(&self) -> ErrorContext {
        let mut context = self.context().clone();
        context.command = context.command.map(|value| redact_sensitive(&value));
        context.endpoint = context.endpoint.map(|value| redact_sensitive(&value));
        context.object = context.object.map(|value| redact_sensitive(&value));
        context.source = context.source.map(|value| redact_sensitive(&value));
        context
    }

    fn grpc_code_name(&self) -> Option<String> {
        match self {
            Self::Grpc { code, .. } => Some(format!("{code:?}")),
            _ => None,
        }
    }

    fn daemon_message(&self) -> Option<String> {
        match self {
            Self::Grpc { message, .. } => Some(message.clone()),
            Self::DaemonUnavailable { message, .. } => Some(message.clone()),
            _ => None,
        }
    }

    fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::DaemonUnavailable { .. } => Some(
                "verify that crius is running, check socket permissions, or override the endpoint with --address",
            ),
            Self::Timeout { .. } => Some(
                "increase --connect-timeout or --timeout, then retry the command",
            ),
            Self::Grpc {
                code: Code::NotFound,
                context,
                ..
            } if context
                .object
                .as_deref()
                .is_some_and(|object| object.starts_with("container ")) =>
            {
                Some("run `crs container list --all` and retry with the full container ID")
            }
            Self::Grpc {
                code: Code::NotFound,
                context,
                ..
            } if context
                .object
                .as_deref()
                .is_some_and(|object| object.starts_with("pod ")) =>
            {
                Some("run `crs pod list --all` and retry with the full pod ID")
            }
            Self::Grpc {
                code: Code::NotFound,
                context,
                ..
            } if context
                .object
                .as_deref()
                .is_some_and(|object| object.starts_with("image ")) =>
            {
                Some("run `crs image list` or pull the image before retrying")
            }
            Self::Grpc {
                code: Code::NotFound,
                ..
            } => Some("list the relevant objects and retry with a longer ID or exact name"),
            Self::Grpc {
                code: Code::FailedPrecondition,
                ..
            } => Some("inspect the object state before retrying the requested operation"),
            Self::Grpc {
                code: Code::PermissionDenied,
                ..
            } => Some("check daemon socket permissions and the current user's privileges"),
            Self::Grpc {
                code: Code::Unavailable,
                ..
            } => Some("verify that crius is running and reachable at the selected endpoint"),
            Self::Grpc {
                code: Code::DeadlineExceeded,
                ..
            } => Some("increase --timeout or inspect daemon health before retrying"),
            _ => None,
        }
    }

    pub(crate) fn exit_status(&self) -> ExitStatus {
        match self {
            Self::NotImplemented { .. } => ExitStatus::General,
            Self::Timeout { .. } => ExitStatus::Timeout,
            Self::DiagnosticsUnavailable { .. } => ExitStatus::General,
            Self::DaemonUnavailable { .. } => ExitStatus::Unavailable,
            Self::Grpc { code, .. } => match code {
                Code::NotFound => ExitStatus::NotFound,
                Code::AlreadyExists => ExitStatus::AlreadyExists,
                Code::FailedPrecondition => ExitStatus::FailedPrecondition,
                Code::PermissionDenied => ExitStatus::PermissionDenied,
                Code::DeadlineExceeded => ExitStatus::Timeout,
                Code::Unavailable => ExitStatus::Unavailable,
                _ => ExitStatus::General,
            },
            Self::Interrupted { .. } => ExitStatus::Interrupted,
            Self::InvalidInput { .. } => ExitStatus::Usage,
            Self::Internal { .. } => ExitStatus::General,
        }
    }

    fn redacted_context_json(&self) -> serde_json::Value {
        serde_json::to_value(self.redacted_context()).unwrap_or_else(|_| json!({}))
    }
}

pub(crate) fn redact_sensitive(input: &str) -> String {
    let mut value = redact_json_secrets(input);
    for key in SENSITIVE_KEYS {
        value = redact_key_value_pattern(&value, key);
        value = redact_key_value_pattern(&value, &key.replace('-', "_"));
    }
    value
}

fn redact_key_value_pattern(input: &str, sensitive_key: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = find_sensitive_assignment(rest, sensitive_key) {
        output.push_str(&rest[..index]);
        let assignment = &rest[index..];
        let separator_offset = sensitive_key.len();
        let value_start = separator_offset + 1;
        let value_end = sensitive_value_end(assignment, value_start);
        output.push_str(&assignment[..value_start]);
        output.push_str("<redacted>");
        rest = &assignment[value_end..];
    }
    output.push_str(rest);
    output
}

fn find_sensitive_assignment(input: &str, sensitive_key: &str) -> Option<usize> {
    let lower = input.to_ascii_lowercase();
    let key = sensitive_key.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(relative) = lower[search_from..].find(&key) {
        let index = search_from + relative;
        let before_ok = input[..index]
            .chars()
            .next_back()
            .map(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .unwrap_or(true);
        let after = input[index + key.len()..].chars().next();
        if before_ok && matches!(after, Some('=' | ':')) {
            return Some(index);
        }
        search_from = index + key.len();
    }
    None
}

fn sensitive_value_end(assignment: &str, value_start: usize) -> usize {
    let value = &assignment[value_start..];
    if value.starts_with('{') {
        return value
            .find('}')
            .map(|offset| value_start + offset + 1)
            .unwrap_or(assignment.len());
    }
    if let Some(quote) = value.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        return value[quote.len_utf8()..]
            .find(quote)
            .map(|offset| value_start + quote.len_utf8() + offset + quote.len_utf8())
            .unwrap_or(assignment.len());
    }

    value
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '}'))
        .map(|offset| value_start + offset)
        .unwrap_or(assignment.len())
}

fn redact_json_secrets(input: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(input) else {
        return input.to_string();
    };
    redact_json_value(&mut value);
    serde_json::to_string(&value).unwrap_or_else(|_| input.to_string())
}

fn redact_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if is_sensitive_key(key) {
                    *child = serde_json::Value::String("<redacted>".into());
                } else {
                    redact_json_value(child);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                redact_json_value(child);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    SENSITIVE_KEYS
        .iter()
        .any(|sensitive| normalized == *sensitive || normalized == sensitive.replace('-', "_"))
}

