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
use std::{todo, unimplemented};
use std::fs::File;
use std::io::Write;

use anyhow::{Context, Result};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use sha2::{Digest, Sha256};

use crate::storage::{ContentBlobRecord, StorageManager};

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

    fn storage(&self) -> Result<Option<StorageManager>> {
        self.ledger_db_path
            .as_ref()
            .map(StorageManager::new)
            .transpose()
    }

    pub fn compute_digest(bytes: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(bytes))
    }

    pub fn relative_blob_path_for_digest(digest: &str) -> PathBuf {
        let clean = digest.trim_start_matches("sha256:");
        let (prefix, suffix) = clean.split_at(clean.len().min(2));
        PathBuf::from("blobs")
            .join("sha256")
            .join(prefix)
            .join(suffix)
    }

    pub fn blob_path_for_digest(&self, digest: &str) -> PathBuf {
        self.root.join(Self::relative_blob_path_for_digest(digest))
    }

    fn read_ledger_info(&self, digest: &str) -> Result<Option<BlobInfo>> {
        let Some(mut storage) = self.storage()? else {
            return Ok(None);
        };
        let Some(record) = storage.get_content_blob(digest)? else {
            return Ok(None);
        };
        storage.touch_content_blob(digest, chrono::Utc::now().timestamp())?;
        Ok(Some(BlobInfo {
            digest: record.digest,
            media_type: record.media_type,
            size: record.size,
            relative_path: PathBuf::from(record.relative_path),
        }))
    }

    fn save_ledger_info(&self, info: &BlobInfo) -> Result<()> {
        let Some(mut storage) = self.storage()? else {
            return Ok(());
        };
        let now = chrono::Utc::now().timestamp();
        let created_at = storage
            .get_content_blob(&info.digest)?
            .map(|record| record.created_at)
            .unwrap_or(now);
        storage.save_content_blob(&ContentBlobRecord {
            digest: info.digest.clone(),
            media_type: info.media_type.clone(),
            size: info.size,
            relative_path: info.relative_path.display().to_string(),
            created_at,
            last_used_at: now,
        })
    }

    fn delete_ledger_info(&self, digest: &str) -> Result<()> {
        if let Some(mut storage) = self.storage()? {
            storage.delete_content_blob(digest)?;
        }
        Ok(())
    }

    fn meta_path_for_digest(&self, digest: &str) -> PathBuf {
        let blob_path = self.blob_path_for_digest(digest);
        let file_name = blob_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("blob");
        blob_path.with_file_name(format!("{file_name}.meta.json"))
    }

    fn read_meta(&self, digest: &str) -> Result<BlobInfo> {
        let path = self.meta_path_for_digest(digest);
        let raw = std::fs::read(&path)
            .with_context(|| format!("failed to read blob metadata {}", path.display()))?;
        let meta: BlobMeta = serde_json::from_slice(&raw)
            .with_context(|| format!("failed to parse blob metadata {}", path.display()))?;
        Ok(BlobInfo {
            digest: meta.digest,
            media_type: meta.media_type,
            size: meta.size,
            relative_path: meta.relative_path,
        })
    }

    fn write_meta(&self, info: &BlobInfo) -> Result<()> {
        let meta = BlobMeta {
            digest: info.digest.clone(),
            media_type: info.media_type.clone(),
            size: info.size,
            relative_path: info.relative_path.clone(),
        };
        let path = self.meta_path_for_digest(&info.digest);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&meta)?)
            .with_context(|| format!("failed to write blob metadata {}", path.display()))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlobMeta {
    digest: String,
    media_type: String,
    size: u64,
    relative_path: PathBuf,
}

pub trait ContentStore: Send + Sync {
    fn put_blob(&self, digest: &str, media_type: &str, bytes: &[u8]) -> Result<BlobInfo>;
    fn get_blob(&self, digest: &str) -> Result<BlobHandle>;
    fn delete_blob(&self, digest: &str) -> Result<()>;
    fn stat_blob(&self, digest: &str) -> Result<BlobInfo>;
}

#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub digest: String,
    pub media_type: String,
    pub size: u64,
    pub relative_path: PathBuf,
}

#[derive(Debug)]
pub struct BlobHandle {
    pub info: BlobInfo,
    pub file: File,
}

impl ContentStore for FsContentStore {
    fn put_blob(&self, digest: &str, media_type: &str, bytes: &[u8]) -> Result<BlobInfo> {
        let computed = Self::compute_digest(bytes);
        let digest = if digest.trim().is_empty() {
            computed
        } else {
            digest.trim().to_string()
        };
        let path = self.blob_path_for_digest(&digest);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        if !path.exists() {
            let temp = path.with_extension("tmp");
            let mut file = File::create(&temp)
                .with_context(|| format!("failed to create {}", temp.display()))?;
            file.write_all(bytes)
                .with_context(|| format!("failed to write {}", temp.display()))?;
            drop(file);
            std::fs::rename(&temp, &path).with_context(|| {
                format!("failed to rename {} -> {}", temp.display(), path.display())
            })?;
        }
        let size = std::fs::metadata(&path)
            .with_context(|| format!("failed to stat {}", path.display()))?
            .len();
        let info = BlobInfo {
            digest: digest.clone(),
            media_type: media_type.trim().to_string(),
            size,
            relative_path: Self::relative_blob_path_for_digest(&digest),
        };
        self.save_ledger_info(&info)?;
        self.write_meta(&info)?;
        Ok(info)
    }

    fn get_blob(&self, digest: &str) -> Result<BlobHandle> {
        let info = self.stat_blob(digest)?;
        let path = self.root.join(&info.relative_path);
        let file =
            File::open(&path).with_context(|| format!("failed to open blob {}", path.display()))?;
        Ok(BlobHandle { info, file })
    }

    fn delete_blob(&self, digest: &str) -> Result<()> {
        let blob_path = self.blob_path_for_digest(digest);
        if blob_path.exists() {
            std::fs::remove_file(&blob_path)
                .with_context(|| format!("failed to delete blob {}", blob_path.display()))?;
        }
        let meta_path = self.meta_path_for_digest(digest);
        if meta_path.exists() {
            std::fs::remove_file(&meta_path).with_context(|| {
                format!("failed to delete blob metadata {}", meta_path.display())
            })?;
        }
        self.delete_ledger_info(digest)?;
        Ok(())
    }

    fn stat_blob(&self, digest: &str) -> Result<BlobInfo> {
        if let Some(info) = self.read_ledger_info(digest)? {
            return Ok(info);
        }
        match self.read_meta(digest) {
            Ok(info) => Ok(info),
            Err(_) => {
                let path = self.blob_path_for_digest(digest);
                let size = std::fs::metadata(&path)
                    .with_context(|| format!("failed to stat blob {}", path.display()))?
                    .len();
                Ok(BlobInfo {
                    digest: digest.to_string(),
                    media_type: String::new(),
                    size,
                    relative_path: Self::relative_blob_path_for_digest(digest),
                })
            }
        }
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
    const RECENT_LIMIT: usize = 16;

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

    fn update(&self, id: &str, stage: impl Into<String>, bytes_completed: u64, bytes_total: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if let Some(record) = inner.active.iter_mut().find(|record| record.id == id) {
            record.current_stage = stage.into();
            record.bytes_completed = bytes_completed;
            record.bytes_total = bytes_total;
        }
    }

    fn finish(&self, id: &str, state: TransferState, error: Option<String>) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let Some(index) = inner.active.iter().position(|record| record.id == id) else {
            return;
        };
        let mut record = inner.active.remove(index);
        record.state = state;
        record.finished_at_unix_nanos = Some(now_unix_nanos());
        record.error = error;
        inner.recent.insert(0, record);
        inner.recent.truncate(Self::RECENT_LIMIT);
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

impl RemoteContentProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Registry => "registry",
            Self::Test => "test",
            Self::Local => "local",
        }
    }
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

    pub fn update(&self, stage: impl Into<String>, bytes_completed: u64, bytes_total: u64) {
        self.tracker
            .update(&self.id, stage, bytes_completed, bytes_total);
    }

    pub fn succeed(mut self) {
        self.finished = true;
        self.tracker
            .finish(&self.id, TransferState::Succeeded, None);
    }

    pub fn fail(mut self, error: impl Into<String>) {
        self.finished = true;
        self.tracker
            .finish(&self.id, TransferState::Failed, Some(error.into()));
    }
}


fn now_unix_nanos() -> i64 {
    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
}