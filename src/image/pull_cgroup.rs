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


use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Serialize};
use anyhow::{Context, Result};

use crate::config::CgroupDriverConfig;

#[derive(Debug, Clone)]
pub enum PullCgroupTarget {
    Disabled,
    Pod { cgroup_parent: String },
    Path(String),
}

impl PullCgroupTarget {
    fn mode(&self) -> PullCgroupMode {
        match self {
            PullCgroupTarget::Disabled => PullCgroupMode::Disabled,
            PullCgroupTarget::Pod { .. } => PullCgroupMode::Pod,
            PullCgroupTarget::Path(_) => PullCgroupMode::Path,
        }
    }

    fn raw_path(&self) -> Option<&str> {
        match self {
            PullCgroupTarget::Disabled => None,
            PullCgroupTarget::Pod { cgroup_parent } => Some(cgroup_parent),
            PullCgroupTarget::Path(path) => Some(path),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PullCgroupExecutor {
    configured: String,
    mode: PullCgroupMode,
    disabled_by_disable_cgroup: bool,
    cgroup_driver: CgroupDriverConfig,
    cgroup_root: PathBuf,
    last_scope: Arc<RwLock<Option<PullCgroupScopeRecord>>>,
}

impl PullCgroupExecutor {
    pub fn new(
        configured: impl Into<String>,
        cgroup_driver: CgroupDriverConfig,
        disable_cgroup: bool,
    ) -> Self {
        Self::new_with_root(
            configured,
            cgroup_driver,
            disable_cgroup,
            PathBuf::from("/sys/fs/cgroup"),
        )
    }

    pub fn new_with_root(
        configured: impl Into<String>,
        cgroup_driver: CgroupDriverConfig,
        disable_cgroup: bool,
        cgroup_root: PathBuf,
    ) -> Self {
        let configured = configured.into();
        Self {
            mode: parse_pull_cgroup_mode(&configured),
            configured,
            disabled_by_disable_cgroup: disable_cgroup,
            cgroup_driver,
            cgroup_root,
            last_scope: Arc::new(RwLock::new(None)),
        }
    }

    pub fn effective_config(&self) -> PullCgroupEffectiveConfig {
        let enabled = self.mode != PullCgroupMode::Disabled
            && !self.disabled_by_disable_cgroup;
        PullCgroupEffectiveConfig {
            configured: self.configured.clone(),
            mode: self.mode.clone(),
            enabled,
            disable_cgroup_degraded: self.mode != PullCgroupMode::Disabled
                && self.disabled_by_disable_cgroup,
            cgroup_driver: self.cgroup_driver,
        }
    }

    pub fn target_for_pod(&self, pod_cgroup_parent: Option<&str>) -> Result<PullCgroupTarget> {
        if self.effective_config().enabled {
            match self.mode {
                PullCgroupMode::Disabled => Ok(PullCgroupTarget::Disabled),
                PullCgroupMode::Path => Ok(PullCgroupTarget::Path(self.configured.clone())),
                PullCgroupMode::Pod => {
                    let cgroup_parent = pod_cgroup_parent
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "runtime.separate_pull_cgroup=pod requires sandbox linux.cgroup_parent"
                            )
                        })?;
                    Ok(PullCgroupTarget::Pod {
                        cgroup_parent: cgroup_parent.to_string(),
                    })
                }
            }
        } else {
            Ok(PullCgroupTarget::Disabled)
        }
    }

    pub fn enter(&self, target: &PullCgroupTarget) -> Result<PullCgroupScopeGuard> {
        let mode = target.mode();
        if matches!(target, PullCgroupTarget::Disabled) {
            let now = chrono::Utc::now().timestamp_millis();
            let record = PullCgroupScopeRecord {
                configured: self.configured.clone(),
                mode,
                effective_path: None,
                entered: false,
                active: false,
                restored: false,
                error: None,
                at_unix_millis: now,
                started_at_unix_millis: now,
                ended_at_unix_millis: Some(now),
            };
            self.record_scope(record);
            return Ok(PullCgroupScopeGuard::inactive());
        }

        match self.enter_active(target) {
            Ok(guard) => Ok(guard),
            Err(error) => {
                let now = chrono::Utc::now().timestamp_millis();
                self.record_scope(PullCgroupScopeRecord {
                    configured: self.configured.clone(),
                    mode,
                    effective_path: target.raw_path().map(ToOwned::to_owned),
                    entered: false,
                    active: false,
                    restored: false,
                    error: Some(error.to_string()),
                    at_unix_millis: now,
                    started_at_unix_millis: now,
                    ended_at_unix_millis: Some(now),
                });
                Err(error)
            }
        }
    }

    fn enter_active(&self, target: &PullCgroupTarget) -> Result<PullCgroupScopeGuard> {
        let relative = self.relative_cgroup_path(target)?;
        let procs_files = self.target_procs_files(&relative);
        for procs_file in &procs_files {
            if let Some(parent) = procs_file.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create pull cgroup {}", parent.display())
                })?;
            }
        }

        let previous = current_process_cgroup_relative_path();
        let pid = std::process::id().to_string();
        for procs_file in &procs_files {
            std::fs::write(procs_file, &pid).with_context(|| {
                format!(
                    "failed to move pull process {} into cgroup {}",
                    pid,
                    procs_file.display()
                )
            })?;
        }

        let started_at = chrono::Utc::now().timestamp_millis();
        self.record_scope(PullCgroupScopeRecord {
            configured: self.configured.clone(),
            mode: target.mode(),
            effective_path: procs_files
                .first()
                .and_then(|path| path.parent())
                .map(|path| path.display().to_string()),
            entered: true,
            active: true,
            restored: false,
            error: None,
            at_unix_millis: started_at,
            started_at_unix_millis: started_at,
            ended_at_unix_millis: None,
        });

        Ok(PullCgroupScopeGuard {
            restore_procs_files: previous
                .as_ref()
                .map(|path| self.target_procs_files(path))
                .unwrap_or_default(),
            last_scope: self.last_scope.clone(),
            active: true,
        })
    }

    fn record_scope(&self, record: PullCgroupScopeRecord) {
        if let Ok(mut last_scope) = self.last_scope.write() {
            *last_scope = Some(record);
        }
    }

    fn target_procs_files(&self, relative: &Path) -> Vec<PathBuf> {
        if self.cgroup_root.join("cgroup.controllers").exists() {
            return vec![self.cgroup_root.join(relative).join("cgroup.procs")];
        }

        let controller_files = ["cpu", "memory", "pids"]
            .into_iter()
            .filter_map(|controller| {
                let controller_root = self.cgroup_root.join(controller);
                controller_root
                    .exists()
                    .then(|| controller_root.join(relative).join("cgroup.procs"))
            })
            .collect::<Vec<_>>();
        if controller_files.is_empty() {
            vec![self.cgroup_root.join(relative).join("cgroup.procs")]
        } else {
            controller_files
        }
    }

    fn relative_cgroup_path(&self, target: &PullCgroupTarget) -> Result<PathBuf> {
        let raw = target.raw_path().unwrap_or_default();
        match self.cgroup_driver {
            CgroupDriverConfig::Systemd => {
                let trimmed = raw.trim();
                let systemd_path = if trimmed.ends_with(".slice") {
                    let basename = Path::new(trimmed)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(trimmed);
                    PathBuf::from(trimmed.trim_start_matches('/'))
                        .join(format!("crius-pull-{}.scope", std::process::id()))
                        .components()
                        .fold(PathBuf::new(), |mut acc, component| {
                            if matches!(component, Component::Normal(_)) {
                                acc.push(component.as_os_str());
                            }
                            if acc.as_os_str().is_empty() && !basename.is_empty() {
                                acc.push(basename);
                            }
                            acc
                        })
                } else {
                    sanitize_relative_cgroup_path(trimmed)
                };
                Ok(systemd_path)
            }
            CgroupDriverConfig::Cgroupfs => Ok(sanitize_relative_cgroup_path(raw)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PullCgroupMode {
    Disabled,
    Pod,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullCgroupScopeRecord {
    pub configured: String,
    pub mode: PullCgroupMode,
    pub effective_path: Option<String>,
    pub entered: bool,
    pub active: bool,
    pub restored: bool,
    pub error: Option<String>,
    pub at_unix_millis: i64,
    pub started_at_unix_millis: i64,
    pub ended_at_unix_millis: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullCgroupEffectiveConfig {
    pub configured: String,
    pub mode: PullCgroupMode,
    pub enabled: bool,
    pub disable_cgroup_degraded: bool,
    pub cgroup_driver: CgroupDriverConfig,
}

pub struct PullCgroupScopeGuard {
    restore_procs_files: Vec<PathBuf>,
    last_scope: Arc<RwLock<Option<PullCgroupScopeRecord>>>,
    active: bool,
}

impl PullCgroupScopeGuard {
    fn inactive() -> Self {
        Self {
            restore_procs_files: Vec::new(),
            last_scope: Arc::new(RwLock::new(None)),
            active: false,
        }
    }
}

pub fn parse_pull_cgroup_mode(raw: &str) -> PullCgroupMode {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        PullCgroupMode::Disabled
    } else if trimmed == "pod" {
        PullCgroupMode::Pod
    } else {
        PullCgroupMode::Path
    }
}

fn current_process_cgroup_relative_path() -> Option<PathBuf> {
    let raw = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    raw.lines().find_map(|line| {
        let mut parts = line.splitn(3, ':');
        let _hierarchy = parts.next()?;
        let controllers = parts.next()?;
        let path = parts.next()?;
        if controllers.is_empty() || controllers.split(',').any(|value| value == "memory") {
            Some(PathBuf::from(path.trim_start_matches('/')))
        } else {
            None
        }
    })
}

fn sanitize_relative_cgroup_path(raw: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for component in Path::new(raw.trim_start_matches('/')).components() {
        if let Component::Normal(value) = component {
            path.push(value);
        }
    }
    if path.as_os_str().is_empty() {
        path.push(format!("crius-pull-{}", std::process::id()));
    }
    path
}