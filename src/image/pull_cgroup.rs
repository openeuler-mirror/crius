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


use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde::{Serialize, Deserialize};
use anyhow::Result;

use crate::config::CgroupDriverConfig;

#[derive(Debug, Clone)]
pub enum PullCgroupTarget {
    Disabled,
    Pod { cgroup_parent: String },
    Path(String),
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