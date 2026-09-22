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
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use std::path::PathBuf;

use anyhow::{Context, Result};
use nix::sys::stat::{major, minor, stat, SFlag};

use crate::oci::spec::{Device as OciDevice, LinuxDeviceCgroup, Spec};

#[derive(Debug, Clone)]
pub struct DeviceMapping {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub permissions: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceOwnership {
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ResolvedDeviceMappings {
    pub devices: Vec<OciDevice>,
    pub cgroup_rules: Vec<LinuxDeviceCgroup>,
}

#[derive(Debug, Clone)]
pub struct DeviceResolverInput<'a> {
    pub privileged: bool,
    pub tty: bool,
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
}

#[derive(Debug, Clone)]
pub struct ResolvedDeviceSet {
    pub devices: Vec<OciDevice>,
    pub cgroup_rules: Vec<LinuxDeviceCgroup>,
    pub degraded_reasons: Vec<String>,
}

pub fn ownership_from_security_context(
    enabled: bool,
    user: Option<&str>,
    run_as_group: Option<u32>,
) -> DeviceOwnership {
    if !enabled {
        return DeviceOwnership {
            uid: None,
            gid: None,
        };
    }

    DeviceOwnership {
        uid: user
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|value| *value > 0),
        gid: run_as_group.filter(|value| *value > 0),
    }
}

pub fn validate_allowed_devices(
    devices: &[DeviceMapping],
    allowed_devices: &HashSet<PathBuf>,
) -> Result<()> {
    if allowed_devices.is_empty() {
        return Ok(());
    }

    for device in devices {
        if !allowed_devices.contains(&device.source) {
            return Err(anyhow::anyhow!(
                "device {} is not allowed by runtime.allowed_devices",
                device.source.display()
            ));
        }
    }
    Ok(())
}

pub fn mappings_to_oci(
    devices: &[DeviceMapping],
    ownership: DeviceOwnership,
) -> Result<ResolvedDeviceMappings> {
    let mut oci_devices = Vec::new();
    let mut cgroup_rules = Vec::new();

    for device in devices {
        let file_stat = stat(&device.source)
            .with_context(|| format!("Failed to stat device path {:?}", device.source))?;
        let file_type = SFlag::from_bits_truncate(file_stat.st_mode);
        let device_type = if file_type.contains(SFlag::S_IFCHR) {
            "c"
        } else if file_type.contains(SFlag::S_IFBLK) {
            "b"
        } else {
            return Err(anyhow::anyhow!(
                "Unsupported device type for {:?}",
                device.source
            ));
        };

        let major_id = major(file_stat.st_rdev) as i64;
        let minor_id = minor(file_stat.st_rdev) as i64;
        let access = if device.permissions.trim().is_empty() {
            "rwm".to_string()
        } else {
            device.permissions.clone()
        };

        oci_devices.push(OciDevice {
            device_type: device_type.to_string(),
            path: device.destination.to_string_lossy().to_string(),
            major: Some(major_id),
            minor: Some(minor_id),
            file_mode: Some((file_stat.st_mode & 0o777) as u32),
            uid: ownership.uid.or(Some(file_stat.st_uid)),
            gid: ownership.gid.or(Some(file_stat.st_gid)),
        });
        cgroup_rules.push(LinuxDeviceCgroup {
            allow: true,
            device_type: Some(device_type.to_string()),
            major: Some(major_id),
            minor: Some(minor_id),
            access: Some(access),
        });
    }

    Ok(ResolvedDeviceMappings {
        devices: oci_devices,
        cgroup_rules,
    })
}

pub fn resolve_devices(input: DeviceResolverInput<'_>) -> Result<ResolvedDeviceSet> {
    validate_rootless_requests(&input)?;

    let ownership = ownership_from_security_context(
        input.device_ownership_from_security_context,
        input.user,
        input.run_as_group,
    );

    let mut devices = if input.privileged {
        if input.privileged_without_host_devices {
            Vec::new()
        } else {
            host_devices()?
        }
    } else {
        Spec::default_devices(input.tty)
    };
    let mut cgroup_rules = input.existing_cgroup_rules.to_vec();

    append_device_mappings(
        &mut devices,
        &mut cgroup_rules,
        input.additional_devices,
        ownership,
    )?;

    validate_allowed_devices(input.requested_devices, input.allowed_devices)?;
    append_device_mappings(
        &mut devices,
        &mut cgroup_rules,
        input.requested_devices,
        ownership,
    )?;

    if input.privileged {
        if !input.privileged_without_host_devices
            || input.privileged_without_host_devices_all_devices_allowed
        {
            cgroup_rules = vec![allow_all_devices_rule()];
        }
    } else if input.existing_cgroup_rules.is_empty() {
        if input.requested_devices.is_empty() && input.additional_devices.is_empty() {
            cgroup_rules = vec![allow_all_devices_rule()];
        } else {
            append_default_device_cgroup_rules(&devices, &mut cgroup_rules);
        }
    }

    let mut degraded_reasons = Vec::new();
    if input.privileged && input.privileged_without_host_devices {
        degraded_reasons
            .push("privileged host device injection skipped by runtime handler policy".to_string());
    }

    Ok(ResolvedDeviceSet {
        devices,
        cgroup_rules,
        degraded_reasons,
    })
}

fn append_default_device_cgroup_rules(
    devices: &[OciDevice],
    cgroup_rules: &mut Vec<LinuxDeviceCgroup>,
) {
    for device in devices {
        let (Some(major), Some(minor)) = (device.major, device.minor) else {
            continue;
        };
        let access = "rwm".to_string();
        if cgroup_rules.iter().any(|rule| {
            rule.allow
                && rule.device_type.as_deref() == Some(device.device_type.as_str())
                && rule.major == Some(major)
                && rule.minor == Some(minor)
                && rule.access.as_deref() == Some(access.as_str())
        }) {
            continue;
        }
        cgroup_rules.push(LinuxDeviceCgroup {
            allow: true,
            device_type: Some(device.device_type.clone()),
            major: Some(major),
            minor: Some(minor),
            access: Some(access),
        });
    }
}

fn validate_rootless_requests(input: &DeviceResolverInput<'_>) -> Result<()> {
    if !input.rootless {
        return Ok(());
    }

    if input.privileged {
        return Err(anyhow::anyhow!(
            "privileged containers are not supported in rootless mode"
        ));
    }

    if !input.requested_devices.is_empty() {
        return Err(anyhow::anyhow!(
            "explicit device requests are not supported in rootless mode because device cgroup rules are disabled"
        ));
    }

    if !input.additional_devices.is_empty() {
        return Err(anyhow::anyhow!(
            "runtime.additional_devices cannot be applied in rootless mode because device cgroup rules are disabled"
        ));
    }

    Ok(())
}

fn append_device_mappings(
    devices: &mut Vec<OciDevice>,
    cgroup_rules: &mut Vec<LinuxDeviceCgroup>,
    mappings: &[DeviceMapping],
    ownership: DeviceOwnership,
) -> Result<()> {
    if mappings.is_empty() {
        return Ok(());
    }

    let resolved = mappings_to_oci(mappings, ownership)?;
    devices.extend(resolved.devices);
    cgroup_rules.extend(resolved.cgroup_rules);
    Ok(())
}

pub fn allow_all_devices_rule() -> LinuxDeviceCgroup {
    LinuxDeviceCgroup {
        allow: true,
        device_type: None,
        major: None,
        minor: None,
        access: Some("rwm".to_string()),
    }
}

pub fn host_devices() -> Result<Vec<OciDevice>> {
    let mut devices = Vec::new();
    collect_host_devices(Path::new("/dev"), &mut devices)?;
    Ok(devices)
}

pub fn should_skip_host_device_dir(name: &str) -> bool {
    matches!(
        name,
        "pts" | "shm" | "fd" | "mqueue" | ".lxc" | ".lxd-mounts" | ".udev"
    )
}

pub fn should_skip_host_device_file(name: &str) -> bool {
    name == "console"
}

fn collect_host_devices(path: &Path, devices: &mut Vec<OciDevice>) -> Result<()> {
    for entry in std::fs::read_dir(path)
        .with_context(|| format!("failed to read host device directory {}", path.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let entry_name = entry.file_name();
        let entry_name = entry_name.to_string_lossy();
        let metadata = std::fs::symlink_metadata(&entry_path)
            .with_context(|| format!("failed to stat host device path {}", entry_path.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            if should_skip_host_device_dir(&entry_name) {
                continue;
            }
            collect_host_devices(&entry_path, devices)?;
            continue;
        }
        if file_type.is_symlink() {
            continue;
        }
        if should_skip_host_device_file(&entry_name) {
            continue;
        }
        if !(file_type.is_char_device() || file_type.is_block_device()) {
            continue;
        }

        let mode = metadata.mode();
        let sflag = SFlag::from_bits_truncate(mode);
        let device_type = if sflag.contains(SFlag::S_IFCHR) {
            "c"
        } else if sflag.contains(SFlag::S_IFBLK) {
            "b"
        } else {
            continue;
        };
        let major_id = major(metadata.rdev()) as i64;
        let minor_id = minor(metadata.rdev()) as i64;
        if major_id == 0 && minor_id == 0 {
            continue;
        }

        devices.push(OciDevice {
            device_type: device_type.to_string(),
            path: entry_path.display().to_string(),
            major: Some(major_id),
            minor: Some(minor_id),
            file_mode: Some(mode & 0o777),
            uid: Some(metadata.uid()),
            gid: Some(metadata.gid()),
        });
    }

    Ok(())
}
