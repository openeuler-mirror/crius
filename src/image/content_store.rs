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


use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::todo;

use anyhow::{Context, Result};
use serde::{Serialize, Deserialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct FsContentStore {
    root: PathBuf,
    ledger_db_path: Option<PathBuf>,
}

impl FsContentStore {
    pub fn new_with_ledger(
        root: impl AsRef<Path>,
        ledger_db_path: Option<PathBuf>,
    ) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(root.join("blobs").join("sha256"))
            .with_context(|| format!("failed to create content store root {}", root.display()))?;
        Ok(Self {
            root,
            ledger_db_path,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ContentTransferTracker {
    inner: Arc<Mutex<ContentTransferTrackerInner>>,
}

impl Default for ContentTransferTracker {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ContentTransferTrackerInner::default())),
        }
    }
}

impl ContentTransferTracker {
    pub fn new_with_ledger(ledger_db_path: Option<PathBuf>) -> Result<Self> {
        let tracker = Self::default();
        Ok(tracker)
    }

    pub fn start(
        &self,
        source: impl Into<String>,
        provider: RemoteContentProviderKind,
        stage: impl Into<String>,
    ) -> ContentTransferGuard {
        let record = ContentTransferRecord {
            id: Uuid::new_v4().to_string(),
            source: source.into(),
            provider,
            state: TransferState::Running,
            current_stage: stage.into(),
            bytes_total: 0,
            bytes_completed: 0,
            started_at_unix_nanos: now_unix_nanos(),
            finished_at_unix_nanos: None,
            error: None,
        };
        let id = record.id.clone();
        if let Ok(mut inner) = self.inner.lock() {
            inner.active.push(record);
        }
        ContentTransferGuard {
            id,
            tracker: self.clone(),
            finished: false,
        }
    }

    pub fn record(&self, id: &str) -> Option<ContentTransferRecord> {
        let Ok(inner) = self.inner.lock() else {
            return None;
        };
        inner
            .active
            .iter()
            .chain(inner.recent.iter())
            .find(|record| record.id == id)
            .cloned()
    }
}

#[derive(Debug, Default)]
struct ContentTransferTrackerInner {
    active: Vec<ContentTransferRecord>,
    recent: Vec<ContentTransferRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentTransferRecord {
    pub id: String,
    pub source: String,
    pub provider: RemoteContentProviderKind,
    pub state: TransferState,
    pub current_stage: String,
    pub bytes_total: u64,
    pub bytes_completed: u64,
    pub started_at_unix_nanos: i64,
    pub finished_at_unix_nanos: Option<i64>,
    pub error: Option<String>,
}

impl ContentTransferRecord {
    
    pub fn to_storage(&self) -> ContentTransferRecord {
        todo!("若存储时需要对struct进行处理，再对记录中元素转换")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteContentProviderKind {
    Registry,
    Test,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferState {
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

#[derive(Debug)]
pub struct ContentTransferGuard {
    id: String,
    tracker: ContentTransferTracker,
    finished: bool,
}

impl ContentTransferGuard {
    pub fn id(&self) -> &str {
        &self.id
    }
}


fn now_unix_nanos() -> i64 {
    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
}