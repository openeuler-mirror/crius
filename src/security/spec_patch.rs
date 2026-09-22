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


use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;

use crate::oci::spec::{
    Linux, LinuxCapabilities, LinuxDeviceCgroup, LinuxResources, Process, Root, Seccomp, Spec,
};
use crate::proto::runtime::v1::Capability;
use crate::security::devices::{self, DeviceMapping};

type ProcPathOverrides = (Option<Vec<String>>, Option<Vec<String>>);

#[derive(Debug, Clone)]
pub struct SpecPatchInput<'a> {
    pub privileged: bool,
    pub tty: bool,
    pub readonly_rootfs: bool,
    pub no_new_privileges: Option<bool>,
    pub apparmor_profile: Option<&'a str>,
    pub selinux_label: Option<&'a str>,
    pub seccomp: Option<Seccomp>,
    pub capabilities: Option<&'a Capability>,
    pub default_capabilities: &'a [String],
    pub add_inheritable_capabilities: bool,
    pub requested_devices: &'a [DeviceMapping],
    pub additional_devices: &'a [DeviceMapping],
    pub existing_cgroup_rules: &'a [LinuxDeviceCgroup],
    pub allowed_devices: &'a HashSet<PathBuf>,
    pub device_ownership_from_security_context: bool,
    pub user: Option<&'a str>,
    pub run_as_group: Option<u32>,
    pub privileged_without_host_devices: bool,
    pub privileged_without_host_devices_all_devices_allowed: bool,
    pub rootless: bool,
    pub cgroup_devices_enabled: bool,
    pub disable_proc_mount: bool,
    pub masked_paths: &'a [String],
    pub readonly_paths: &'a [String],
}

#[derive(Debug, Clone)]
pub struct SpecSecurityPatch {
    pub root_readonly: bool,
    pub process: ProcessSecurityPatch,
    pub linux: LinuxSecurityPatch,
    pub degraded_reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProcessSecurityPatch {
    pub capabilities: LinuxCapabilities,
    pub no_new_privileges: bool,
    pub apparmor_profile: Option<String>,
    pub selinux_label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinuxSecurityPatch {
    pub devices: Vec<crate::oci::spec::Device>,
    pub device_cgroup_rules: Vec<LinuxDeviceCgroup>,
    pub seccomp: Option<Seccomp>,
    pub mount_label: Option<String>,
    pub masked_paths: Option<Vec<String>>,
    pub readonly_paths: Option<Vec<String>>,
    pub cgroup_devices_enabled: bool,
}

impl SpecSecurityPatch {
    pub fn from_input(input: SpecPatchInput<'_>) -> Result<Self> {
        let devices = devices::resolve_devices(devices::DeviceResolverInput {
            privileged: input.privileged,
            tty: input.tty,
            requested_devices: input.requested_devices,
            additional_devices: input.additional_devices,
            existing_cgroup_rules: input.existing_cgroup_rules,
            allowed_devices: input.allowed_devices,
            device_ownership_from_security_context: input.device_ownership_from_security_context,
            user: input.user,
            run_as_group: input.run_as_group,
            privileged_without_host_devices: input.privileged_without_host_devices,
            privileged_without_host_devices_all_devices_allowed: input
                .privileged_without_host_devices_all_devices_allowed,
            rootless: input.rootless,
        })?;
        let (masked_paths, readonly_paths) = effective_proc_paths(
            input.privileged,
            input.disable_proc_mount,
            input.masked_paths,
            input.readonly_paths,
        )?;

        Ok(Self {
            root_readonly: input.readonly_rootfs,
            process: ProcessSecurityPatch {
                capabilities: apply_capability_overrides(
                    &capability_baseline(input.default_capabilities, input.privileged),
                    input.capabilities,
                    input.add_inheritable_capabilities,
                ),
                no_new_privileges: input.no_new_privileges.unwrap_or(!input.privileged),
                apparmor_profile: input.apparmor_profile.map(ToString::to_string),
                selinux_label: input.selinux_label.map(ToString::to_string),
            },
            linux: LinuxSecurityPatch {
                devices: devices.devices,
                device_cgroup_rules: devices.cgroup_rules,
                seccomp: input.seccomp,
                mount_label: input.selinux_label.map(ToString::to_string),
                masked_paths,
                readonly_paths,
                cgroup_devices_enabled: input.cgroup_devices_enabled,
            },
            degraded_reasons: devices.degraded_reasons,
        })
    }

    pub fn apply_root(&self, root: &mut Root) {
        root.readonly = Some(self.root_readonly);
    }

    pub fn apply_process(&self, process: &mut Process) {
        process.capabilities = Some(self.process.capabilities.clone());
        process.no_new_privileges = Some(self.process.no_new_privileges);
        process.apparmor_profile = self.process.apparmor_profile.clone();
        process.selinux_label = self.process.selinux_label.clone();
    }

    pub fn apply_linux(&self, linux: &mut Linux) {
        linux.devices = Some(self.linux.devices.clone());
        linux.seccomp = self.linux.seccomp.clone();
        linux.mount_label = self.linux.mount_label.clone();
        linux.masked_paths = self.linux.masked_paths.clone();
        linux.readonly_paths = self.linux.readonly_paths.clone();

        if self.linux.cgroup_devices_enabled {
            let resources = linux.resources.get_or_insert_with(empty_linux_resources);
            resources.devices = Some(self.linux.device_cgroup_rules.clone());
        }
    }

    pub fn apply_spec(&self, spec: &mut Spec) {
        if let Some(root) = spec.root.as_mut() {
            self.apply_root(root);
        }
        if let Some(process) = spec.process.as_mut() {
            self.apply_process(process);
        }
        if let Some(linux) = spec.linux.as_mut() {
            self.apply_linux(linux);
        }
    }
}

pub fn validate_rootless_requests(input: &SpecPatchInput<'_>) -> Result<()> {
    devices::resolve_devices(devices::DeviceResolverInput {
        privileged: input.privileged,
        tty: input.tty,
        requested_devices: input.requested_devices,
        additional_devices: input.additional_devices,
        existing_cgroup_rules: input.existing_cgroup_rules,
        allowed_devices: input.allowed_devices,
        device_ownership_from_security_context: input.device_ownership_from_security_context,
        user: input.user,
        run_as_group: input.run_as_group,
        privileged_without_host_devices: input.privileged_without_host_devices,
        privileged_without_host_devices_all_devices_allowed: input
            .privileged_without_host_devices_all_devices_allowed,
        rootless: true,
    })
    .map(|_| ())
}

pub fn normalize_capability_name(name: &str) -> String {
    let upper = name.trim().to_ascii_uppercase();
    if upper.starts_with("CAP_") {
        upper
    } else {
        format!("CAP_{upper}")
    }
}

pub fn default_capabilities() -> Vec<String> {
    vec![
        "CAP_CHOWN".to_string(),
        "CAP_DAC_OVERRIDE".to_string(),
        "CAP_FSETID".to_string(),
        "CAP_FOWNER".to_string(),
        "CAP_MKNOD".to_string(),
        "CAP_NET_RAW".to_string(),
        "CAP_SETGID".to_string(),
        "CAP_SETUID".to_string(),
        "CAP_SETFCAP".to_string(),
        "CAP_SETPCAP".to_string(),
        "CAP_NET_BIND_SERVICE".to_string(),
        "CAP_SYS_CHROOT".to_string(),
        "CAP_KILL".to_string(),
        "CAP_AUDIT_WRITE".to_string(),
    ]
}

pub fn capability_baseline(default_capabilities: &[String], privileged: bool) -> Vec<String> {
    if privileged {
        privileged_capabilities()
    } else {
        default_capabilities.to_vec()
    }
}

pub fn apply_capability_overrides(
    default_caps: &[String],
    overrides: Option<&Capability>,
    add_inheritable_capabilities: bool,
) -> LinuxCapabilities {
    let mut base = default_caps.to_vec();
    let mut ambient = Vec::new();

    if let Some(capabilities) = overrides {
        let normalized_drops: Vec<String> = capabilities
            .drop_capabilities
            .iter()
            .map(|cap| normalize_capability_name(cap))
            .collect();

        if normalized_drops.iter().any(|cap| cap == "CAP_ALL") {
            base.clear();
        } else {
            base.retain(|cap| !normalized_drops.iter().any(|drop| drop == cap));
        }

        for cap in &capabilities.add_capabilities {
            let normalized = normalize_capability_name(cap);
            if !base.contains(&normalized) {
                base.push(normalized);
            }
        }

        ambient = capabilities
            .add_ambient_capabilities
            .iter()
            .map(|cap| normalize_capability_name(cap))
            .collect();

        for cap in &ambient {
            if !base.contains(cap) {
                base.push(cap.clone());
            }
        }
    }

    LinuxCapabilities {
        bounding: Some(base.clone()),
        effective: Some(base.clone()),
        inheritable: Some(if add_inheritable_capabilities {
            base.clone()
        } else {
            Vec::new()
        }),
        permitted: Some(base),
        ambient: Some(ambient),
    }
}

pub fn effective_proc_paths(
    privileged: bool,
    disable_proc_mount: bool,
    requested_masked_paths: &[String],
    requested_readonly_paths: &[String],
) -> Result<ProcPathOverrides> {
    if privileged {
        return Ok((None, None));
    }

    if disable_proc_mount {
        if !requested_masked_paths.is_empty() || !requested_readonly_paths.is_empty() {
            return Err(anyhow::anyhow!(
                "Kubernetes ProcMount support is disabled by runtime.disable_proc_mount"
            ));
        }
        return Ok((
            Some(Spec::default_masked_paths()),
            Some(Spec::default_readonly_paths()),
        ));
    }

    Ok((
        Some(requested_masked_paths.to_vec()),
        Some(requested_readonly_paths.to_vec()),
    ))
}

fn privileged_capabilities() -> Vec<String> {
    vec![
        "CAP_AUDIT_CONTROL".to_string(),
        "CAP_AUDIT_READ".to_string(),
        "CAP_AUDIT_WRITE".to_string(),
        "CAP_BLOCK_SUSPEND".to_string(),
        "CAP_BPF".to_string(),
        "CAP_CHECKPOINT_RESTORE".to_string(),
        "CAP_CHOWN".to_string(),
        "CAP_DAC_OVERRIDE".to_string(),
        "CAP_DAC_READ_SEARCH".to_string(),
        "CAP_FOWNER".to_string(),
        "CAP_FSETID".to_string(),
        "CAP_IPC_LOCK".to_string(),
        "CAP_IPC_OWNER".to_string(),
        "CAP_KILL".to_string(),
        "CAP_LEASE".to_string(),
        "CAP_LINUX_IMMUTABLE".to_string(),
        "CAP_MAC_ADMIN".to_string(),
        "CAP_MAC_OVERRIDE".to_string(),
        "CAP_MKNOD".to_string(),
        "CAP_NET_ADMIN".to_string(),
        "CAP_NET_BIND_SERVICE".to_string(),
        "CAP_NET_BROADCAST".to_string(),
        "CAP_NET_RAW".to_string(),
        "CAP_PERFMON".to_string(),
        "CAP_SETFCAP".to_string(),
        "CAP_SETGID".to_string(),
        "CAP_SETPCAP".to_string(),
        "CAP_SETUID".to_string(),
        "CAP_SYSLOG".to_string(),
        "CAP_SYS_ADMIN".to_string(),
        "CAP_SYS_BOOT".to_string(),
        "CAP_SYS_CHROOT".to_string(),
        "CAP_SYS_MODULE".to_string(),
        "CAP_SYS_NICE".to_string(),
        "CAP_SYS_PACCT".to_string(),
        "CAP_SYS_PTRACE".to_string(),
        "CAP_SYS_RAWIO".to_string(),
        "CAP_SYS_RESOURCE".to_string(),
        "CAP_SYS_TIME".to_string(),
        "CAP_SYS_TTY_CONFIG".to_string(),
        "CAP_WAKE_ALARM".to_string(),
    ]
}

fn empty_linux_resources() -> LinuxResources {
    LinuxResources {
        network: None,
        pids: None,
        memory: None,
        cpu: None,
        block_io: None,
        hugepage_limits: None,
        devices: None,
        intel_rdt: None,
        unified: None,
    }
}