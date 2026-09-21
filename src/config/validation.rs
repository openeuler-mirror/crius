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


use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::config::CgroupDriverConfig;

pub(super) fn current_platform_key() -> String {
    format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)
}

pub(super) fn resolve_platform_runtime_path(
    default_runtime_path: &str,
    platform_runtime_paths: &HashMap<String, String>,
) -> Result<String> {
    let default_runtime_path = default_runtime_path.trim();
    let selected = platform_runtime_paths
        .get(&current_platform_key())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(default_runtime_path);
    if selected.is_empty() {
        return Err(Error::Config(format!(
            "runtime path for platform {} must not be empty",
            current_platform_key()
        )));
    }
    Ok(selected.to_string())
}

pub(super) fn resolve_monitor_cgroup(
    raw: &str,
    cgroup_driver: CgroupDriverConfig,
) -> Result<String> {
    let trimmed = raw.trim();
    match cgroup_driver {
        CgroupDriverConfig::Systemd => {
            if trimmed.is_empty() {
                return Ok("system.slice".to_string());
            }
            if trimmed == "pod" || trimmed.ends_with(".slice") {
                return Ok(trimmed.to_string());
            }
            Err(Error::Config(format!(
                "monitor cgroup should be \"pod\", empty, or a systemd slice ending with .slice, got {trimmed}"
            )))
        }
        CgroupDriverConfig::Cgroupfs => {
            if trimmed.is_empty() || trimmed == "pod" {
                return Ok(trimmed.to_string());
            }
            Err(Error::Config(format!(
                "monitor cgroup should be \"pod\" or empty for cgroupfs, got {trimmed}"
            )))
        }
    }
}

pub(super) fn detect_system_cgroup_driver() -> CgroupDriverConfig {
    let systemd_active = Path::new("/run/systemd/system").exists()
        || std::fs::read_to_string("/proc/1/comm")
            .map(|content| content.trim() == "systemd")
            .unwrap_or(false);
    let cgroup_v2 = Path::new("/sys/fs/cgroup/cgroup.controllers").exists();
    let systemd_cgroup_layout = Path::new("/sys/fs/cgroup/system.slice").exists()
        || Path::new("/sys/fs/cgroup/user.slice").exists()
        || Path::new("/sys/fs/cgroup/systemd").exists();

    if systemd_active && (cgroup_v2 || systemd_cgroup_layout) {
        CgroupDriverConfig::Systemd
    } else {
        CgroupDriverConfig::Cgroupfs
    }
}