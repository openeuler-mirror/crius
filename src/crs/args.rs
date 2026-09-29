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
    Status(StatusArgs),
    Images(ImageListArgs),
    Run(Box<RunArgs>),
    Ps(ListArgs),
    Logs(ContainerLogsArgs),
    Pull(ImagePullArgs),
    Rmi {
        image: String,
    },
    Image(ImageArgs),
    Inspect(InspectArgs),
    Debug(DebugArgs),
    Completion(CompletionArgs),
}

#[derive(Debug, ClapArgs)]
pub struct ContainerLogsArgs {
    #[arg(short = 'f', long)]
    pub follow: bool,
    #[arg(short = 'n', long)]
    pub tail: Option<i64>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(short = 't', long)]
    pub timestamps: bool,
    pub container: String,
}


#[derive(Debug, Default, ClapArgs)]
pub struct ListArgs {
    #[arg(short = 'a', long)]
    pub all: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ContainerStateArg {
    Created,
    Running,
    Exited,
    Unknown,
}

#[derive(Debug, Default, ClapArgs)]
pub struct ContainerListArgs {
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long, help = "Join an existing local Pod sandbox")]
    pub pod: Option<String>,
    #[arg(long, value_enum)]
    pub state: Option<ContainerStateArg>,
    #[arg(short = 'l', long = "label")]
    pub labels: Vec<String>,
    #[arg(long)]
    pub all: bool,
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

#[derive(Debug, ClapArgs)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Show,
    ReloadStatus,
}

#[derive(Debug, Default, ClapArgs)]
pub struct StatusArgs {
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum PullPolicyArg {
    Missing,
    Always,
    Never,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ExecModeArg {
    Sync,
    Attach,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum StreamProtocolArg {
    Websocket,
    Spdy,
}

impl Default for StreamProtocolArg {
    fn default() -> Self {
        Self::Websocket
    }
}

#[derive(Clone, Debug, Default, ClapArgs)]
pub struct StreamOptions {
    #[arg(short = 'i', long = "interactive", alias = "stdin")]
    pub stdin: bool,
    #[arg(short = 't', long)]
    pub tty: bool,
    #[arg(long, default_value_t = true)]
    pub stdout: bool,
    #[arg(long, default_value_t = true)]
    pub stderr: bool,
    #[arg(long)]
    pub resize: Option<String>,
    #[arg(long, value_enum, default_value_t = StreamProtocolArg::Websocket)]
    pub protocol: StreamProtocolArg,
}

#[derive(Clone, Debug, Default, ClapArgs)]
pub struct RunPodCreateOptions {
    #[arg(long = "pod-name")]
    pub pod_name: Option<String>,
    #[arg(long)]
    pub uid: Option<String>,
    #[arg(long = "pod-attempt")]
    pub pod_attempt: Option<u32>,
    #[arg(long)]
    pub hostname: Option<String>,
    #[arg(long)]
    pub log_dir: Option<String>,
    #[arg(long = "dns-server")]
    pub dns_servers: Vec<String>,
    #[arg(long = "dns-search")]
    pub dns_searches: Vec<String>,
    #[arg(long = "dns-option")]
    pub dns_options: Vec<String>,
    #[arg(short = 'p', long = "publish")]
    pub publish: Vec<String>,
    #[arg(long = "pod-label", id = "run-pod-label")]
    pub labels: Vec<String>,
    #[arg(long = "pod-annotation", id = "run-pod-annotation")]
    pub annotations: Vec<String>,
    #[arg(long = "runtime", alias = "runtime-handler")]
    pub runtime_handler: Option<String>,
    #[arg(long)]
    pub cgroup_parent: Option<String>,
    #[arg(long = "sysctl")]
    pub sysctls: Vec<String>,
    #[arg(long)]
    pub host_network: bool,
    #[arg(long)]
    pub host_pid: bool,
    #[arg(long)]
    pub host_ipc: bool,
    #[arg(long)]
    pub userns: Option<String>,
    #[arg(long = "uid-map")]
    pub uid_maps: Vec<String>,
    #[arg(long = "gid-map")]
    pub gid_maps: Vec<String>,
    #[command(flatten)]
    pub security: SandboxSecurityArgs,
    #[arg(long = "overhead")]
    pub overhead: Vec<String>,
    #[arg(long = "pod-resource")]
    pub pod_resources: Vec<String>,
}

#[derive(Clone, Debug, Default, ClapArgs)]
pub struct SandboxSecurityArgs {
    #[arg(long)]
    pub sandbox_user: Option<String>,
    #[arg(long)]
    pub sandbox_group: Option<u32>,
    #[arg(long = "sandbox-supplemental-group")]
    pub sandbox_supplemental_groups: Vec<u32>,
    #[arg(long = "sandbox-read-only", alias = "sandbox-readonly-rootfs")]
    pub sandbox_readonly_rootfs: bool,
    #[arg(long)]
    pub sandbox_privileged: bool,
    #[arg(long)]
    pub sandbox_seccomp: Option<String>,
    #[arg(long)]
    pub sandbox_apparmor: Option<String>,
    #[arg(long)]
    pub sandbox_selinux: Option<String>,
}

#[derive(Clone, Debug, Default, ClapArgs)]
pub struct RunContainerCreateOptions {
    #[arg(long = "container-attempt")]
    pub container_attempt: Option<u32>,
    #[arg(long = "command")]
    pub commands: Vec<String>,
    #[arg(long = "arg", allow_hyphen_values = true)]
    pub args: Vec<String>,
    #[arg(short = 'w', long)]
    pub workdir: Option<String>,
    #[arg(short = 'e', long = "env")]
    pub env: Vec<String>,
    #[arg(long = "env-file")]
    pub env_files: Vec<String>,
    #[arg(short = 'l', long = "label")]
    pub labels: Vec<String>,
    #[arg(long = "annotation")]
    pub annotations: Vec<String>,
    #[arg(long = "mount")]
    pub mounts: Vec<String>,
    #[arg(long = "device")]
    pub devices: Vec<String>,
    #[arg(long = "cdi-device")]
    pub cdi_devices: Vec<String>,
    #[arg(long)]
    pub log_path: Option<String>,
    #[command(flatten)]
    pub resources: ContainerResourceArgs,
    #[command(flatten)]
    pub security: ContainerSecurityArgs,
}

#[derive(Clone, Debug, Default, ClapArgs)]
pub struct ContainerResourceArgs {
    #[arg(long = "resource")]
    pub resources: Vec<String>,
    #[arg(long)]
    pub cpu_period: Option<i64>,
    #[arg(long)]
    pub cpu_quota: Option<i64>,
    #[arg(short = 'c', long)]
    pub cpu_shares: Option<i64>,
    #[arg(short = 'm', long)]
    pub memory: Option<String>,
    #[arg(long)]
    pub memory_swap: Option<String>,
    #[arg(long)]
    pub oom_score_adj: Option<i64>,
    #[arg(long)]
    pub cpuset_cpus: Option<String>,
    #[arg(long)]
    pub cpuset_mems: Option<String>,
    #[arg(long = "hugepage")]
    pub hugepages: Vec<String>,
    #[arg(long = "unified")]
    pub unified: Vec<String>,
}

#[derive(Clone, Debug, Default, ClapArgs)]
pub struct ContainerSecurityArgs {
    #[arg(long)]
    pub privileged: bool,
    #[arg(long = "cap-add")]
    pub cap_add: Vec<String>,
    #[arg(long = "cap-drop")]
    pub cap_drop: Vec<String>,
    #[arg(long = "ambient-cap-add")]
    pub ambient_cap_add: Vec<String>,
    #[arg(short = 'u', long)]
    pub user: Option<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long = "group-add", alias = "supplemental-group")]
    pub supplemental_groups: Vec<String>,
    #[arg(long = "read-only", alias = "readonly-rootfs")]
    pub readonly_rootfs: bool,
    #[arg(long)]
    pub no_new_privs: bool,
    #[arg(long = "masked-path")]
    pub masked_paths: Vec<String>,
    #[arg(long = "readonly-path")]
    pub readonly_paths: Vec<String>,
    #[arg(long)]
    pub seccomp: Option<String>,
    #[arg(long)]
    pub apparmor: Option<String>,
    #[arg(long)]
    pub selinux: Option<String>,
    #[arg(long)]
    pub pid: Option<String>,
    #[arg(long)]
    pub ipc: Option<String>,
    #[arg(long)]
    pub blockio_class: Option<String>,
    #[arg(long)]
    pub rdt_class: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct RunArgs {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(short = 'd', long)]
    pub detach: bool,
    #[arg(long)]
    pub rm: bool,
    #[arg(short = 't', long)]
    pub tty: bool,
    #[arg(short = 'i', long = "interactive", alias = "stdin")]
    pub stdin: bool,
    #[arg(long, value_enum, default_value_t = StreamProtocolArg::Websocket)]
    pub protocol: StreamProtocolArg,
    #[arg(long)]
    pub pod: Option<String>,
    #[arg(long, value_enum, default_value_t = PullPolicyArg::Missing)]
    pub pull: PullPolicyArg,
    #[arg(long, value_enum, default_value_t = ExecModeArg::Attach)]
    pub exec_mode: ExecModeArg,
    #[command(flatten)]
    pub pod_options: RunPodCreateOptions,
    #[command(flatten)]
    pub container_options: RunContainerCreateOptions,
    pub image: String,
    pub command: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ContainerCreateArgs {
    #[command(flatten)]
    pub options: ContainerCreateOptions,
    pub pod: String,
    pub image: String,
    #[arg(last = true)]
    pub command: Vec<String>,
}

#[derive(Clone, Debug, Default, ClapArgs)]
pub struct ContainerCreateOptions {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub attempt: Option<u32>,
    #[arg(long = "command")]
    pub commands: Vec<String>,
    #[arg(long = "arg", allow_hyphen_values = true)]
    pub args: Vec<String>,
    #[arg(short = 'w', long)]
    pub workdir: Option<String>,
    #[arg(short = 'e', long = "env")]
    pub env: Vec<String>,
    #[arg(long = "env-file")]
    pub env_files: Vec<String>,
    #[arg(long = "pod-label")]
    pub labels: Vec<String>,
    #[arg(long = "pod-annotation")]
    pub annotations: Vec<String>,
    #[arg(long = "mount")]
    pub mounts: Vec<String>,
    #[arg(long = "device")]
    pub devices: Vec<String>,
    #[arg(long = "cdi-device")]
    pub cdi_devices: Vec<String>,
    #[arg(long)]
    pub log_path: Option<String>,
    #[arg(short = 'i', long = "interactive", alias = "stdin")]
    pub stdin: bool,
    #[arg(long)]
    pub tty: bool,
    #[command(flatten)]
    pub resources: ContainerResourceArgs,
    #[command(flatten)]
    pub security: ContainerSecurityArgs,
}