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

use clap::{Args as ClapArgs, Subcommand, ValueEnum, Parser};

use crate::crs::parsers::{parse_duration, DEFAULT_ENDPOINT};

#[derive(Debug, Parser)]
#[command(name = "crs", version, about = "Local command-line client for crius")]
pub struct Args{
    #[arg(
        short = 'H',
        long = "host",
        alias = "address",
        env = "CRIUS_ADDRESS",
        default_value = DEFAULT_ENDPOINT,
        global = true
    )]
    pub address: String,
    #[arg(long, default_value = "5s", value_parser = parse_duration, global = true)]
    pub connect_timeout: Duration,
    #[arg(long, default_value = "0s", value_parser = parse_duration)]
    pub timeout: Duration,
    #[arg(short = 'D', long, global = true)]
    pub debug: bool,
    #[arg(long, value_enum, default_value_t = OutputArg::Table, global = true)]
    pub output: OutputArg,
    #[arg(long, global = true)]
    pub quiet: bool,
    #[arg(long, global = true)]
    pub no_trunc: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Copy, Clone, Debug, ValueEnum, Eq, PartialEq)]
pub enum OutputArg {
    Table,
    Json,
    Text,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Version(VersionArgs),
    Images(ImageListArgs),
    Pull(ImagePullArgs),
    Rmi {
        image: String,
    },
    Image(ImageArgs),
    Inspect(InspectArgs),
    Debug(DebugArgs),
    Completion(CompletionArgs),
}

#[derive(Debug, Default, ClapArgs)]
pub struct VersionArgs {}

#[derive(Debug, Default, ClapArgs)]
pub struct ImageListArgs {
    #[arg(long)]
    pub image: Option<String>,
}

#[derive(Debug, Default, ClapArgs)]
pub struct ImageAuthArgs {
    #[arg(
        long,
        conflicts_with_all = [
            "auth_file",
            "username",
            "password",
            "server",
            "identity_token",
            "registry_token"
        ]
    )]
    pub auth_json: Option<String>,
    #[arg(
        long,
        conflicts_with_all = [
            "auth_json",
            "username",
            "password",
            "server",
            "identity_token",
            "registry_token"
        ]
    )]
    pub auth_file: Option<String>,
    #[arg(long)]
    pub username: Option<String>,
    #[arg(long, requires = "username")]
    pub password: Option<String>,
    #[arg(long, requires = "username")]
    pub server: Option<String>,
    #[arg(long, requires = "username")]
    pub identity_token: Option<String>,
    #[arg(long, requires = "username")]
    pub registry_token: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ImagePullArgs {
    #[command(flatten)]
    pub auth: ImageAuthArgs,
    #[arg(long)]
    pub pod: Option<String>,
    pub image: String,
}

#[derive(Debug, Subcommand)]
pub enum ImageCommand {
    List(ImageListArgs),
    Pull(ImagePullArgs),
    Inspect { image: String },
    Remove { image: String },
    FsInfo,
    Transfers,
    Config,
}

#[derive(Debug, ClapArgs)]
pub struct ImageArgs {
    #[command(subcommand)]
    pub command: ImageCommand,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ObjectType {
    Container,
    Pod,
    Image,
}

#[derive(Debug, ClapArgs)]
pub struct InspectArgs {
    #[arg(long = "type", value_enum)]
    pub object_type: Option<ObjectType>,
    pub target: String,
}

#[derive(Debug, Subcommand)]
pub enum DebugCommand {
    Network,
    Runtime,
    Shims,
    Nri,
    Security,
    Cgroups,
    Streaming,
    Metrics,
    Tracing,
    Rootless,
}

#[derive(Debug, ClapArgs)]
pub struct DebugArgs {
    #[command(subcommand)]
    pub command: DebugCommand,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

#[derive(Debug, ClapArgs)]
pub struct CompletionArgs {
    #[arg(value_enum)]
    pub shell: CompletionShell,
}