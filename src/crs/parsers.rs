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


use std::time::Duration;

use base64::Engine;

use crate::proto::runtime::v1::AuthConfig;

pub(crate) const DEFAULT_ENDPOINT: &str = "unix:///run/crius/crius.sock";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Endpoint {
    Unix(String),
    Tcp(String),
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unix(path) => write!(f, "unix://{path}"),
            Self::Tcp(uri) => f.write_str(uri),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthConfigJson {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    auth: String,
    #[serde(default, alias = "server")]
    server_address: String,
    #[serde(default)]
    identity_token: String,
    #[serde(default)]
    registry_token: String,
}

#[derive(serde::Deserialize)]
struct DockerAuthFile {
    auths: std::collections::BTreeMap<String, DockerAuthEntry>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DockerAuthEntry {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    auth: String,
    #[serde(default)]
    identity_token: String,
    #[serde(default)]
    registry_token: String,
}

impl From<AuthConfigJson> for AuthConfig {
    fn from(value: AuthConfigJson) -> Self {
        Self {
            username: value.username,
            password: value.password,
            auth: value.auth,
            server_address: value.server_address,
            identity_token: value.identity_token,
            registry_token: value.registry_token,
        }
    }
}

pub(crate) fn parse_duration(value: &str) -> Result<Duration, String> {
    let trimmed = value.trim();
    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (digits, unit) = trimmed.split_at(split_at);

    if digits.is_empty() || unit.is_empty() {
        return Err(format!(
            "invalid duration \"{value}\": expected an integer followed by ms, s, m, or h"
        ));
    }

    let amount = digits
        .parse::<u64>()
        .map_err(|_| format!("invalid duration \"{value}\": value is out of range"))?;

    match unit {
        "ms" => Ok(Duration::from_millis(amount)),
        "s" => Ok(Duration::from_secs(amount)),
        "m" => amount
            .checked_mul(60)
            .map(Duration::from_secs)
            .ok_or_else(|| format!("invalid duration \"{value}\": value is out of range")),
        "h" => amount
            .checked_mul(60 * 60)
            .map(Duration::from_secs)
            .ok_or_else(|| format!("invalid duration \"{value}\": value is out of range")),
        _ => Err(format!(
            "invalid duration \"{value}\": expected an integer followed by ms, s, m, or h"
        )),
    }
}

#[allow(dead_code)]
pub(crate) fn parse_auth_json(source: &str, value: &str) -> Result<AuthConfig, String> {
    let json: serde_json::Value = serde_json::from_str(value)
        .map_err(|error| format!("invalid auth JSON from {source}: {error}"))?;

    if json.get("auths").is_none() {
        let config: AuthConfigJson = serde_json::from_value(json)
            .map_err(|error| format!("invalid auth JSON from {source}: {error}"))?;
        return Ok(config.into());
    }

    let docker = serde_json::from_value::<DockerAuthFile>(json)
        .map_err(|error| format!("invalid auth JSON from {source}: {error}"))?;

    let Some((server, entry)) = docker.auths.into_iter().next() else {
        return Err(format!(
            "invalid auth JSON from {source}: auths must not be empty"
        ));
    };

    let (username, password) =
        if !entry.auth.is_empty() && (entry.username.is_empty() || entry.password.is_empty()) {
            decode_docker_auth(source, &entry.auth)?
        } else {
            (entry.username, entry.password)
        };

    Ok(AuthConfig {
        username,
        password,
        auth: entry.auth,
        server_address: server,
        identity_token: entry.identity_token,
        registry_token: entry.registry_token,
    })
}

fn decode_docker_auth(source: &str, auth: &str) -> Result<(String, String), String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(auth)
        .map_err(|error| {
            format!("invalid auth JSON from {source}: invalid docker auth: {error}")
        })?;
    let decoded = String::from_utf8(decoded).map_err(|error| {
        format!("invalid auth JSON from {source}: invalid docker auth: {error}")
    })?;
    let Some((username, password)) = decoded.split_once(':') else {
        return Err(format!(
            "invalid auth JSON from {source}: docker auth must decode to username:password"
        ));
    };

    Ok((username.to_string(), password.to_string()))
}