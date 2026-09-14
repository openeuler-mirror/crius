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



use crate::proto::runtime::v1::AuthConfig;
use crate::crs::{
    args::ImageAuthArgs,
    parsers::parse_auth_json,
};

pub(crate) fn build_auth_config(args: &ImageAuthArgs) -> Result<Option<AuthConfig>, String> {
    let sources = [
        args.auth_json.is_some(),
        args.auth_file.is_some(),
        args.username.is_some()
            || args.password.is_some()
            || args.server.is_some()
            || args.identity_token.is_some()
            || args.registry_token.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();

    if sources > 1 {
        return Err(
            "auth options must use only one source: --auth-json, --auth-file, or username flags"
                .into(),
        );
    }

    if let Some(value) = &args.auth_json {
        return parse_auth_json("--auth-json", value).map(Some);
    }

    if let Some(path) = &args.auth_file {
        let content = std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read auth file \"{path}\": {error}"))?;
        return parse_auth_json(path, &content).map(Some);
    }

    if args.username.is_none()
        && args.password.is_none()
        && args.server.is_none()
        && args.identity_token.is_none()
        && args.registry_token.is_none()
    {
        return Ok(None);
    }

    Ok(Some(AuthConfig {
        username: args.username.clone().unwrap_or_default(),
        password: args.password.clone().unwrap_or_default(),
        auth: String::new(),
        server_address: args.server.clone().unwrap_or_default(),
        identity_token: args.identity_token.clone().unwrap_or_default(),
        registry_token: args.registry_token.clone().unwrap_or_default(),
    }))
}