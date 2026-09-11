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

use crate::crs::{args::{Args, OutputArg}, parsers::Endpoint};

#[derive(Clone, Debug)]
pub(crate) struct CliContext {
    endpoint: Endpoint,
    connect_timeout: Duration,
    rpc_timeout: Duration,
    output: OutputArg,
    quiet: bool,
    no_trunc: bool,
    debug: bool,
}

impl CliContext {
    pub(crate) fn from_args(args: &Args) -> Result<Self, String> {
        let endpoint = parse_endpoint(&args.address)?;

        Ok(Self {
            endpoint,
            connect_timeout: args.connect_timeout,
            rpc_timeout: args.timeout,
            output: args.output,
            quiet: args.quiet,
            no_trunc: args.no_trunc,
            debug: args.debug,
        })
    }

    pub(crate) fn output(&self) -> OutputArg {
        self.output
    }

    #[allow(dead_code)]
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub(crate) fn endpoint_display(&self) -> String {
        self.endpoint.to_string()
    }

    pub(crate) fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub(crate) fn rpc_timeout(&self) -> Duration{
        self.rpc_timeout
    }
}


pub(crate) fn parse_endpoint(value: &str) -> Result<Endpoint, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(
            "invalid endpoint \"\": expected unix path, unix://, http://, or https://".into(),
        );
    }

    if let Some(path) = value.strip_prefix("unix://") {
        if path.is_empty() {
            return Err(format!(
                "invalid endpoint \"{value}\": unix endpoint requires a socket path"
            ));
        }
        return Ok(Endpoint::Unix(path.to_string()));
    }

    if value.starts_with('/') {
        return Ok(Endpoint::Unix(value.to_string()));
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        return Ok(Endpoint::Tcp(value.to_string()));
    }

    Err(format!(
        "invalid endpoint \"{value}\": expected unix path, unix://, http://, or https://"
    ))
}