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


#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use crate::{
    crs::{
        args::{
            ContainerCreateArgs, ContainerCreateOptions, ContainerResourceArgs,
            ContainerSecurityArgs, ImageAuthArgs,
        },
        parsers::{
            parse_auth_json, parse_byte_size, parse_device, parse_env_file, parse_hugepage,
            parse_key_value, parse_mount,
            parse_resource_spec, parse_security_profile, parse_selinux_option, parse_user,
            KeyValuePair, ParsedUser, ResourceFragment,
        },
    },
    proto::runtime::v1::{
        AuthConfig, Capability, CdiDevice, ContainerConfig, ContainerMetadata,
        ImageSpec, Int64Value, KeyValue, LinuxContainerConfig, LinuxContainerResources,
        LinuxContainerSecurityContext, NamespaceMode, NamespaceOption,
    },
};

pub(crate) fn build_container_config(
    args: &ContainerCreateArgs,
) -> Result<ContainerConfig, String> {
    build_container_config_from_parts(&args.image, &args.container, &args.command, &args.options)
}

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

pub(crate) fn parse_local_sysctls(sysctls: &[String]) -> Result<Vec<String>, String> {
    key_value_map("--sysctl", sysctls).map(|values| {
        values
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect()
    })
}

fn build_container_config_from_parts(
    image: &str,
    name: &str,
    positional_command: &[String],
    options: &ContainerCreateOptions,
) -> Result<ContainerConfig, String> {
    if image.is_empty() {
        return Err("container image must not be empty".into());
    }

    let metadata = ContainerMetadata {
        name: name.to_string(),
        attempt: options.attempt.unwrap_or_default(),
    };

    let (command, args) = container_command_and_args(options, positional_command);
    let linux = LinuxContainerConfig {
        resources: build_container_resources(&options.resources)?,
        security_context: Some(build_container_security_context(&options.security)?),
    };

    Ok(ContainerConfig {
        metadata: Some(metadata),
        image: Some(ImageSpec {
            image: image.to_string(),
            user_specified_image: image.to_string(),
            ..Default::default()
        }),
        command,
        args,
        working_dir: options.workdir.clone().unwrap_or_default(),
        envs: build_envs(options)?,
        mounts: options
            .mounts
            .iter()
            .map(|mount| parse_mount(mount))
            .collect::<Result<Vec<_>, _>>()?,
        devices: options
            .devices
            .iter()
            .map(|device| parse_device(device))
            .collect::<Result<Vec<_>, _>>()?,
        labels: key_value_map("--label", &options.labels)?,
        annotations: build_container_annotations(options)?,
        log_path: options.log_path.clone().unwrap_or_default(),
        stdin: options.stdin,
        stdin_once: options.stdin,
        tty: options.tty,
        linux: Some(linux),
        windows: None,
        cdi_devices: options
            .cdi_devices
            .iter()
            .cloned()
            .map(|name| CdiDevice { name })
            .collect(),
    })
}

fn container_command_and_args(
    options: &ContainerCreateOptions,
    positional_command: &[String],
) -> (Vec<String>, Vec<String>) {
    if !options.commands.is_empty() {
        let mut args = options.args.clone();
        args.extend_from_slice(positional_command);
        (options.commands.clone(), args)
    } else if !positional_command.is_empty() {
        (positional_command.to_vec(), options.args.clone())
    } else {
        (Vec::new(), options.args.clone())
    }
}

fn build_envs(options: &ContainerCreateOptions) -> Result<Vec<KeyValue>, String> {
    let mut envs = BTreeMap::new();
    for file in &options.env_files {
        for pair in parse_env_file(file)? {
            envs.insert(pair.key, pair.value);
        }
    }
    for pair in options
        .env
        .iter()
        .map(|value| parse_key_value("--env", value))
        .collect::<Result<Vec<_>, _>>()?
    {
        envs.insert(pair.key, pair.value);
    }

    Ok(envs
        .into_iter()
        .map(|(key, value)| KeyValue { key, value })
        .collect())
}

fn build_container_annotations(
    options: &ContainerCreateOptions,
) -> Result<HashMap<String, String>, String> {
    let mut annotations = key_value_map("--annotation", &options.annotations)?;
    if let Some(value) = &options.security.blockio_class {
        annotations.insert("crius.io/blockio-class".into(), value.clone());
    }
    if let Some(value) = &options.security.rdt_class {
        annotations.insert("crius.io/rdt-class".into(), value.clone());
    }
    Ok(annotations)
}

fn build_container_resources(
    args: &ContainerResourceArgs,
) -> Result<Option<LinuxContainerResources>, String> {
    let mut fragment = ResourceFragment {
        cpu_period: args.cpu_period,
        cpu_quota: args.cpu_quota,
        cpu_shares: args.cpu_shares,
        memory_limit_in_bytes: args
            .memory
            .as_deref()
            .map(|value| parse_byte_size_as_i64("--memory", value))
            .transpose()?,
        memory_swap_limit_in_bytes: args
            .memory_swap
            .as_deref()
            .map(|value| parse_byte_size_as_i64("--memory-swap", value))
            .transpose()?,
        oom_score_adj: args.oom_score_adj,
        cpuset_cpus: args.cpuset_cpus.clone(),
        cpuset_mems: args.cpuset_mems.clone(),
        hugepages: args
            .hugepages
            .iter()
            .map(|value| parse_hugepage(value))
            .collect::<Result<Vec<_>, _>>()?,
        unified: args
            .unified
            .iter()
            .map(|value| parse_key_value("--unified", value))
            .collect::<Result<Vec<_>, _>>()?,
    };

    dedupe_resource_keys(&mut fragment);
    for value in &args.resources {
        merge_resource_fragment(&mut fragment, parse_resource_spec(value)?);
    }
    validate_resource_fragment(&fragment)?;
    if is_empty_resource_fragment(&fragment) {
        return Ok(None);
    }

    Ok(Some(resources_from_fragment(fragment)))
}

fn build_container_security_context(
    args: &ContainerSecurityArgs,
) -> Result<LinuxContainerSecurityContext, String> {
    let (run_as_user, run_as_group_from_user, run_as_username) = match args.user.as_deref() {
        Some(user) => match parse_user(user)? {
            ParsedUser::Id { uid, gid } => (
                Some(Int64Value { value: uid }),
                gid.map(|value| Int64Value { value }),
                String::new(),
            ),
            ParsedUser::Name(name) => (None, None, name),
        },
        None => (None, None, String::new()),
    };

    if args.group.is_some() && run_as_user.is_none() && run_as_username.is_empty() {
        return Err("--group requires --user".into());
    }

    Ok(LinuxContainerSecurityContext {
        capabilities: if args.cap_add.is_empty()
            && args.cap_drop.is_empty()
            && args.ambient_cap_add.is_empty()
        {
            None
        } else {
            Some(Capability {
                add_capabilities: args.cap_add.clone(),
                drop_capabilities: args.cap_drop.clone(),
                add_ambient_capabilities: args.ambient_cap_add.clone(),
            })
        },
        privileged: args.privileged,
        namespace_options: Some(build_container_namespace_options(args)?),
        selinux_options: optional_profile(args.selinux.as_deref(), parse_selinux_option)?,
        run_as_user,
        run_as_group: args
            .group
            .as_deref()
            .map(|value| parse_non_negative_i64("--group", value))
            .transpose()?
            .map(|value| Int64Value { value })
            .or(run_as_group_from_user),
        run_as_username,
        readonly_rootfs: args.readonly_rootfs,
        supplemental_groups: args
            .supplemental_groups
            .iter()
            .map(|value| parse_non_negative_i64("--supplemental-group", value))
            .collect::<Result<Vec<_>, _>>()?,
        no_new_privs: args.no_new_privs,
        masked_paths: args.masked_paths.clone(),
        readonly_paths: args.readonly_paths.clone(),
        seccomp: optional_profile(args.seccomp.as_deref(), parse_security_profile)?,
        apparmor: optional_profile(args.apparmor.as_deref(), parse_security_profile)?,
        ..Default::default()
    })
}

fn build_container_namespace_options(
    args: &ContainerSecurityArgs,
) -> Result<NamespaceOption, String> {
    Ok(NamespaceOption {
        network: NamespaceMode::Pod as i32,
        pid: parse_namespace_mode("--pid", args.pid.as_deref(), NamespaceMode::Container)?,
        ipc: parse_namespace_mode("--ipc", args.ipc.as_deref(), NamespaceMode::Pod)?,
        target_id: String::new(),
        userns_options: None,
    })
}

pub(crate) fn build_resources_from_specs(
    values: &[String],
) -> Result<Option<LinuxContainerResources>, String> {
    if values.is_empty() {
        return Ok(None);
    }

    let mut merged = ResourceFragment::default();
    for value in values {
        merge_resource_fragment(&mut merged, parse_resource_spec(value)?);
    }
    validate_resource_fragment(&merged)?;

    Ok(Some(resources_from_fragment(merged)))
}

fn validate_resource_fragment(fragment: &ResourceFragment) -> Result<(), String> {
    let fields = [
        ("cpu-period", fragment.cpu_period),
        ("cpu-quota", fragment.cpu_quota),
        ("cpu-shares", fragment.cpu_shares),
        ("memory", fragment.memory_limit_in_bytes),
        ("memory-swap", fragment.memory_swap_limit_in_bytes),
        ("oom-score-adj", fragment.oom_score_adj),
    ];

    for (name, value) in fields {
        if value.is_some_and(|value| value < 0) {
            return Err(format!("resource field {name} must be non-negative"));
        }
    }

    Ok(())
}

fn is_empty_resource_fragment(fragment: &ResourceFragment) -> bool {
    fragment.cpu_period.is_none()
        && fragment.cpu_quota.is_none()
        && fragment.cpu_shares.is_none()
        && fragment.memory_limit_in_bytes.is_none()
        && fragment.memory_swap_limit_in_bytes.is_none()
        && fragment.oom_score_adj.is_none()
        && fragment.cpuset_cpus.is_none()
        && fragment.cpuset_mems.is_none()
        && fragment.hugepages.is_empty()
        && fragment.unified.is_empty()
}

fn merge_resource_fragment(target: &mut ResourceFragment, fragment: ResourceFragment) {
    if fragment.cpu_period.is_some() {
        target.cpu_period = fragment.cpu_period;
    }
    if fragment.cpu_quota.is_some() {
        target.cpu_quota = fragment.cpu_quota;
    }
    if fragment.cpu_shares.is_some() {
        target.cpu_shares = fragment.cpu_shares;
    }
    if fragment.memory_limit_in_bytes.is_some() {
        target.memory_limit_in_bytes = fragment.memory_limit_in_bytes;
    }
    if fragment.memory_swap_limit_in_bytes.is_some() {
        target.memory_swap_limit_in_bytes = fragment.memory_swap_limit_in_bytes;
    }
    if fragment.oom_score_adj.is_some() {
        target.oom_score_adj = fragment.oom_score_adj;
    }
    if fragment.cpuset_cpus.is_some() {
        target.cpuset_cpus = fragment.cpuset_cpus;
    }
    if fragment.cpuset_mems.is_some() {
        target.cpuset_mems = fragment.cpuset_mems;
    }
    merge_keyed(&mut target.hugepages, fragment.hugepages, |item| {
        item.page_size.as_str()
    });
    merge_keyed(&mut target.unified, fragment.unified, |item| {
        item.key.as_str()
    });
}

fn dedupe_resource_keys(fragment: &mut ResourceFragment) {
    let hugepages = std::mem::take(&mut fragment.hugepages);
    merge_keyed(&mut fragment.hugepages, hugepages, |item| {
        item.page_size.as_str()
    });
    let unified = std::mem::take(&mut fragment.unified);
    merge_keyed(&mut fragment.unified, unified, |item| item.key.as_str());
}

fn resources_from_fragment(fragment: ResourceFragment) -> LinuxContainerResources {
    LinuxContainerResources {
        cpu_period: fragment.cpu_period.unwrap_or_default(),
        cpu_quota: fragment.cpu_quota.unwrap_or_default(),
        cpu_shares: fragment.cpu_shares.unwrap_or_default(),
        memory_limit_in_bytes: fragment.memory_limit_in_bytes.unwrap_or_default(),
        oom_score_adj: fragment.oom_score_adj.unwrap_or_default(),
        cpuset_cpus: fragment.cpuset_cpus.unwrap_or_default(),
        cpuset_mems: fragment.cpuset_mems.unwrap_or_default(),
        hugepage_limits: fragment.hugepages,
        unified: fragment
            .unified
            .into_iter()
            .map(|pair| (pair.key, pair.value))
            .collect(),
        memory_swap_limit_in_bytes: fragment.memory_swap_limit_in_bytes.unwrap_or_default(),
    }
}

fn key_value_map(flag: &str, values: &[String]) -> Result<HashMap<String, String>, String> {
    values
        .iter()
        .map(|value| parse_key_value(flag, value))
        .collect::<Result<Vec<_>, _>>()
        .map(key_value_pairs_to_map)
}

fn key_value_pairs_to_map(pairs: Vec<KeyValuePair>) -> HashMap<String, String> {
    pairs
        .into_iter()
        .map(|pair| (pair.key, pair.value))
        .collect()
}

fn required_or_default(value: Option<&str>, name: &str) -> Result<String, String> {
    match value {
        Some(value) if !value.is_empty() => Ok(value.to_string()),
        Some(_) => Err(format!("{name} must not be empty")),
        None => Ok(String::new()),
    }
}

fn optional_profile<T>(
    value: Option<&str>,
    parse: impl FnOnce(&str) -> Result<T, String>,
) -> Result<Option<T>, String> {
    value.map(parse).transpose()
}

fn merge_keyed<T>(target: &mut Vec<T>, values: Vec<T>, key: impl Fn(&T) -> &str) {
    for value in values {
        if let Some(index) = target.iter().position(|item| key(item) == key(&value)) {
            target[index] = value;
        } else {
            target.push(value);
        }
    }
}

fn parse_namespace_mode(
    flag: &str,
    value: Option<&str>,
    default: NamespaceMode,
) -> Result<i32, String> {
    let mode = match value {
        None => default,
        Some("pod") => NamespaceMode::Pod,
        Some("container") => NamespaceMode::Container,
        Some("node") => NamespaceMode::Node,
        Some(value) => {
            return Err(format!(
                "invalid {flag} \"{value}\": expected pod, container, or node"
            ));
        }
    };
    Ok(mode as i32)
}

fn parse_non_negative_i64(flag: &str, value: &str) -> Result<i64, String> {
    let parsed = value
        .parse::<i64>()
        .map_err(|_| format!("invalid {flag} \"{value}\": expected non-negative integer"))?;
    if parsed < 0 {
        return Err(format!(
            "invalid {flag} \"{value}\": expected non-negative integer"
        ));
    }
    Ok(parsed)
}

fn parse_byte_size_as_i64(flag: &str, value: &str) -> Result<i64, String> {
    let bytes = parse_byte_size(value)?;
    i64::try_from(bytes).map_err(|_| format!("invalid {flag} \"{value}\": value is out of range"))
}