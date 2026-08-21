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

use crate::config::CgroupDriverConfig;

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