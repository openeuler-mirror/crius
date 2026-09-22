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
// Minimal snapshotter types for crius-shim.
//
// Only the data types that `shim/daemon.rs` and `shim_rpc/proto.rs` actually
// read/write are defined here. The full github snapshotter (probe/prepare/
// mount/commit/remove, ~740 lines) is intentionally NOT ported: it pulls in
// `config::ExternalSnapshotterConfig`, `storage::SnapshotRecord` and a set of
// `StorageManager` snapshot methods that the minimal shim path does not need.
// `daemon.rs` implements rootfs mounting itself (`mount_rootfs_spec`,
// `apply_rootfs_handle_mounts`) by reading these struct fields directly.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::content_store::FsContentStore;
use super::metadata_store::FilesystemImageMetadataStore;
use super::ImageMeta;
use crate::config::ExternalSnapshotterConfig;
use crate::storage::{SnapshotRecord, StorageManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotMode {
    InternalOverlayUntar,
    InternalCachedRootfs,
}

pub trait Snapshotter: Send + Sync {
    fn prepare(&self, key: &str, image_ref: &str, destination: &Path) -> Result<PreparedSnapshot>;
    fn mount(&self, key: &str) -> Result<MountView>;
    fn commit(&self, key: &str) -> Result<SnapshotInfo>;
    fn remove(&self, key: &str) -> Result<()>;
    fn usage_for(&self, key: &str) -> Result<SnapshotUsage>;
    fn usage(&self) -> Result<SnapshotUsage>;
}

pub const INTERNAL_OVERLAY_UNTAR_SNAPSHOTTER: &str = "internal-overlay-untar";
pub const INTERNAL_CACHED_ROOTFS_SNAPSHOTTER: &str = "internal-cached-rootfs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotterProbe {
    pub name: String,
    #[serde(rename = "type")]
    pub snapshotter_type: String,
    pub endpoint: Option<String>,
    pub path: Option<String>,
    pub capabilities: Vec<String>,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

impl SnapshotterProbe {
    pub fn internal(name: &str, capabilities: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            snapshotter_type: "internal".to_string(),
            endpoint: None,
            path: None,
            capabilities: capabilities
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            available: true,
            unavailable_reason: None,
        }
    }

    pub fn external(name: &str, config: &ExternalSnapshotterConfig) -> Self {
        let path = non_empty_value(&config.path);
        let endpoint = non_empty_value(&config.endpoint);
        let path_error = path.as_deref().and_then(|value| {
            (!Path::new(value).exists()).then(|| format!("configured path does not exist: {value}"))
        });
        let endpoint_error = endpoint.as_deref().and_then(probe_endpoint);
        let unavailable_reason = path_error.or(endpoint_error);
        let mut capabilities = normalize_capabilities(&config.capabilities);
        if capabilities.is_empty() {
            capabilities.push("mount-spec".to_string());
        }

        Self {
            name: name.to_string(),
            snapshotter_type: config.snapshotter_type.trim().to_string(),
            endpoint,
            path,
            capabilities,
            available: unavailable_reason.is_none(),
            unavailable_reason,
        }
    }

    pub fn resolved_name(&self) -> String {
        if self.available {
            self.name.clone()
        } else {
            INTERNAL_OVERLAY_UNTAR_SNAPSHOTTER.to_string()
        }
    }
}

pub fn probe_configured_snapshotter(
    requested: &str,
    external: &std::collections::HashMap<String, ExternalSnapshotterConfig>,
) -> SnapshotterProbe {
    match requested.trim() {
        "" | INTERNAL_OVERLAY_UNTAR_SNAPSHOTTER => SnapshotterProbe::internal(
            INTERNAL_OVERLAY_UNTAR_SNAPSHOTTER,
            &["rootfs-path", "local-untar"],
        ),
        INTERNAL_CACHED_ROOTFS_SNAPSHOTTER => SnapshotterProbe::internal(
            INTERNAL_CACHED_ROOTFS_SNAPSHOTTER,
            &["rootfs-path", "local-untar", "cached-rootfs"],
        ),
        name => external
            .get(name)
            .map(|config| SnapshotterProbe::external(name, config))
            .unwrap_or_else(|| SnapshotterProbe {
                name: name.to_string(),
                snapshotter_type: "unknown".to_string(),
                endpoint: None,
                path: None,
                capabilities: Vec::new(),
                available: false,
                unavailable_reason: Some("snapshotter is not configured".to_string()),
            }),
    }
}

pub fn probe_all_configured_snapshotters(
    external: &std::collections::HashMap<String, ExternalSnapshotterConfig>,
) -> Vec<SnapshotterProbe> {
    let mut probes = vec![
        probe_configured_snapshotter(INTERNAL_OVERLAY_UNTAR_SNAPSHOTTER, external),
        probe_configured_snapshotter(INTERNAL_CACHED_ROOTFS_SNAPSHOTTER, external),
    ];
    let mut external_names: Vec<_> = external.keys().cloned().collect();
    external_names.sort();
    probes.extend(
        external_names
            .iter()
            .map(|name| probe_configured_snapshotter(name, external)),
    );
    probes
}

fn non_empty_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn normalize_capabilities(values: &[String]) -> Vec<String> {
    let mut capabilities = Vec::new();
    for capability in values {
        let trimmed = capability.trim();
        if !trimmed.is_empty() && !capabilities.iter().any(|existing| existing == trimmed) {
            capabilities.push(trimmed.to_string());
        }
    }
    capabilities
}

fn probe_endpoint(endpoint: &str) -> Option<String> {
    let socket_path = endpoint.strip_prefix("unix://").map(Path::new).or_else(|| {
        Path::new(endpoint)
            .is_absolute()
            .then(|| Path::new(endpoint))
    });
    socket_path.and_then(|path| {
        (!path.exists()).then(|| format!("configured endpoint does not exist: {}", path.display()))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotState {
    Prepared,
    Mounted,
    Committed,
    Deleted,
    Broken,
}

impl SnapshotState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Mounted => "mounted",
            Self::Committed => "committed",
            Self::Deleted => "deleted",
            Self::Broken => "broken",
        }
    }

    pub fn parse_state(value: &str) -> Self {
        match value {
            "prepared" => Self::Prepared,
            "mounted" => Self::Mounted,
            "committed" => Self::Committed,
            "deleted" => Self::Deleted,
            "broken" => Self::Broken,
            _ => Self::Broken,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedSnapshot {
    pub key: String,
    pub image_id: String,
    pub rootfs_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountView {
    pub key: String,
    pub mountpoint: PathBuf,
    pub readonly: bool,
    pub rootfs: RootfsHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInfo {
    pub key: String,
    pub image_id: String,
    pub state: SnapshotState,
    pub mountpoint: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct SnapshotUsage {
    pub used_bytes: u64,
    pub inodes_used: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootfsHandleKind {
    InternalPath,
    ExternalMountSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootfsOwner {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootfsMountSpec {
    #[serde(rename = "type")]
    pub mount_type: String,
    pub source: PathBuf,
    pub target: PathBuf,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootfsHandle {
    pub kind: RootfsHandleKind,
    pub snapshot_key: Option<String>,
    pub owner: RootfsOwner,
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub mounts: Vec<RootfsMountSpec>,
    pub readonly: bool,
}

impl RootfsHandle {
    pub fn internal_path(
        snapshot_key: impl Into<String>,
        owner_kind: impl Into<String>,
        owner_id: impl Into<String>,
        path: impl Into<PathBuf>,
        readonly: bool,
    ) -> Self {
        Self {
            kind: RootfsHandleKind::InternalPath,
            snapshot_key: Some(snapshot_key.into()),
            owner: RootfsOwner {
                kind: owner_kind.into(),
                id: owner_id.into(),
            },
            path: Some(path.into()),
            mounts: Vec::new(),
            readonly,
        }
    }

    pub fn external_mount_spec(
        snapshot_key: impl Into<String>,
        owner_kind: impl Into<String>,
        owner_id: impl Into<String>,
        target: impl Into<PathBuf>,
        mounts: Vec<RootfsMountSpec>,
        readonly: bool,
    ) -> Self {
        Self {
            kind: RootfsHandleKind::ExternalMountSpec,
            snapshot_key: Some(snapshot_key.into()),
            owner: RootfsOwner {
                kind: owner_kind.into(),
                id: owner_id.into(),
            },
            path: Some(target.into()),
            mounts,
            readonly,
        }
    }

    pub fn rootfs_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct FilesystemSnapshotter {
    mode: SnapshotMode,
    storage_root: PathBuf,
    metadata_store: FilesystemImageMetadataStore,
    content_store: FsContentStore,
    ledger_db_path: Option<PathBuf>,
}

impl FilesystemSnapshotter {
    pub fn new(
        mode: SnapshotMode,
        storage_root: impl AsRef<Path>,
        metadata_store: FilesystemImageMetadataStore,
        content_store: FsContentStore,
        ledger_db_path: Option<PathBuf>,
    ) -> Self {
        Self {
            mode,
            storage_root: storage_root.as_ref().to_path_buf(),
            metadata_store,
            content_store,
            ledger_db_path,
        }
    }

    fn snapshot_root(&self) -> PathBuf {
        self.storage_root.join("snapshots")
    }

    fn snapshotter_name(&self) -> &'static str {
        match self.mode {
            SnapshotMode::InternalOverlayUntar => INTERNAL_OVERLAY_UNTAR_SNAPSHOTTER,
            SnapshotMode::InternalCachedRootfs => INTERNAL_CACHED_ROOTFS_SNAPSHOTTER,
        }
    }

    fn cached_rootfs_dir(&self, image_id: &str) -> PathBuf {
        self.snapshot_root().join(image_id).join("rootfs")
    }

    fn resolve_image(&self, image_ref: &str) -> Result<(ImageMeta, PathBuf)> {
        self.metadata_store
            .find_by_reference(image_ref, |image, requested_ref| {
                if image.id == requested_ref {
                    return true;
                }
                image.repo_tags.iter().any(|tag| tag == requested_ref)
                    || image
                        .repo_digests
                        .iter()
                        .any(|digest| digest == requested_ref)
                    || image.id.starts_with(requested_ref)
            })?
            .map(|record| (record.meta, record.record_dir))
            .ok_or_else(|| anyhow::anyhow!("image {image_ref} is not present locally"))
    }

    fn materialize_layers(
        &self,
        metadata: &ImageMeta,
        record_dir: &Path,
        destination: &Path,
    ) -> Result<()> {
        if destination.exists() {
            std::fs::remove_dir_all(destination)
                .with_context(|| format!("failed to clean {}", destination.display()))?;
        }
        std::fs::create_dir_all(destination)
            .with_context(|| format!("failed to create {}", destination.display()))?;

        let mut layer_paths = Vec::new();
        for layer in &metadata.stored_layers {
            let path = if !layer.digest.trim().is_empty() {
                self.content_store
                    .root()
                    .join(FsContentStore::relative_blob_path_for_digest(&layer.digest))
            } else {
                record_dir.join(&layer.path)
            };
            layer_paths.push((layer.path.clone(), path));
        }
        if layer_paths.is_empty() {
            for entry in std::fs::read_dir(record_dir)? {
                let entry = entry?;
                let path = entry.path();
                if matches!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("gz" | "tar")
                ) {
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                        .to_string();
                    layer_paths.push((name, path));
                }
            }
        }
        layer_paths.sort_by_key(|(name, _)| {
            name.split('.')
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(u32::MAX)
        });
        if layer_paths.is_empty() {
            return Err(anyhow::anyhow!(
                "image {} has no stored layers or archives",
                metadata.id
            ));
        }

        for (_, layer_path) in layer_paths {
            unpack_layer_with_tar(&layer_path, destination)?;
        }
        Ok(())
    }

    fn copy_rootfs_tree(source: &Path, destination: &Path) -> Result<()> {
        if destination.exists() {
            std::fs::remove_dir_all(destination)
                .with_context(|| format!("failed to clean {}", destination.display()))?;
        }
        std::fs::create_dir_all(destination)
            .with_context(|| format!("failed to create {}", destination.display()))?;
        let output = Command::new("cp")
            .arg("-a")
            .arg(format!("{}/.", source.display()))
            .arg(destination)
            .output()
            .with_context(|| format!("failed to execute cp for {}", source.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(anyhow::anyhow!(
                "failed to copy cached rootfs from {} to {}: {}",
                source.display(),
                destination.display(),
                stderr
            ));
        }
        Ok(())
    }

    fn storage(&self) -> Result<StorageManager> {
        let db_path = self
            .ledger_db_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("snapshot ledger is not configured"))?;
        StorageManager::new(db_path)
    }

    fn snapshot_record(&self, key: &str) -> Result<SnapshotRecord> {
        self.storage()?
            .list_snapshots()?
            .into_iter()
            .find(|record| record.key == key)
            .ok_or_else(|| anyhow::anyhow!("snapshot {key} not found"))
    }

    fn save_snapshot_state(&self, mut record: SnapshotRecord, state: SnapshotState) -> Result<()> {
        record.state = state.as_str().to_string();
        self.storage()?.save_snapshot(&record)
    }

    fn info_from_record(record: SnapshotRecord) -> SnapshotInfo {
        SnapshotInfo {
            key: record.key,
            image_id: record.image_id,
            state: SnapshotState::parse_state(&record.state),
            mountpoint: PathBuf::from(record.mountpoint),
        }
    }
}

impl Snapshotter for FilesystemSnapshotter {
    fn prepare(&self, key: &str, image_ref: &str, destination: &Path) -> Result<PreparedSnapshot> {
        let (metadata, record_dir) = self.resolve_image(image_ref)?;
        match self.mode {
            SnapshotMode::InternalOverlayUntar => {
                self.materialize_layers(&metadata, &record_dir, destination)?;
            }
            SnapshotMode::InternalCachedRootfs => {
                let cached_rootfs = self.cached_rootfs_dir(&metadata.id);
                if !cached_rootfs.exists() {
                    self.materialize_layers(&metadata, &record_dir, &cached_rootfs)?;
                }
                Self::copy_rootfs_tree(&cached_rootfs, destination)?;
            }
        }
        if let Some(db_path) = self.ledger_db_path.as_ref() {
            let mut storage = StorageManager::new(db_path)?;
            storage.save_snapshot(&SnapshotRecord {
                key: key.to_string(),
                image_id: metadata.id.clone(),
                owner_kind: "container".to_string(),
                owner_id: key.to_string(),
                state: SnapshotState::Prepared.as_str().to_string(),
                mountpoint: destination.display().to_string(),
                snapshotter: self.snapshotter_name().to_string(),
                runtime_managed: true,
            })?;
        }
        Ok(PreparedSnapshot {
            key: key.to_string(),
            image_id: metadata.id,
            rootfs_path: destination.to_path_buf(),
        })
    }

    fn mount(&self, key: &str) -> Result<MountView> {
        let record = self.snapshot_record(key)?;
        let mountpoint = PathBuf::from(&record.mountpoint);
        if !mountpoint.exists() {
            self.save_snapshot_state(record, SnapshotState::Broken)?;
            return Err(anyhow::anyhow!(
                "snapshot {key} mountpoint does not exist: {}",
                mountpoint.display()
            ));
        }
        self.save_snapshot_state(record.clone(), SnapshotState::Mounted)?;
        let rootfs = RootfsHandle::internal_path(
            key.to_string(),
            record.owner_kind.clone(),
            record.owner_id.clone(),
            mountpoint.clone(),
            false,
        );
        Ok(MountView {
            key: key.to_string(),
            mountpoint,
            readonly: false,
            rootfs,
        })
    }

    fn commit(&self, key: &str) -> Result<SnapshotInfo> {
        let record = self.snapshot_record(key)?;
        self.save_snapshot_state(record.clone(), SnapshotState::Committed)?;
        Ok(Self::info_from_record(SnapshotRecord {
            state: SnapshotState::Committed.as_str().to_string(),
            ..record
        }))
    }

    fn remove(&self, key: &str) -> Result<()> {
        let record = self.snapshot_record(key)?;
        let mountpoint = PathBuf::from(&record.mountpoint);
        if mountpoint.exists() {
            std::fs::remove_dir_all(&mountpoint)
                .with_context(|| format!("failed to remove snapshot {}", mountpoint.display()))?;
        }
        self.storage()?.delete_snapshot(key)
    }

    fn usage_for(&self, key: &str) -> Result<SnapshotUsage> {
        let record = self.snapshot_record(key)?;
        let (used_bytes, inodes_used) =
            crate::image::content_store::collect_path_usage(Path::new(&record.mountpoint))?;
        Ok(SnapshotUsage {
            used_bytes,
            inodes_used,
        })
    }

    fn usage(&self) -> Result<SnapshotUsage> {
        let (used_bytes, inodes_used) =
            crate::image::content_store::collect_path_usage(&self.snapshot_root())?;
        Ok(SnapshotUsage {
            used_bytes,
            inodes_used,
        })
    }
}

fn unpack_layer_with_tar(layer_file: &Path, rootfs_dir: &Path) -> Result<()> {
    let mut command = Command::new("tar");
    if layer_file.extension().and_then(|ext| ext.to_str()) == Some("gz") {
        command.arg("-xzf");
    } else {
        command.arg("-xf");
    }
    let output = command
        .arg(layer_file)
        .arg("-C")
        .arg(rootfs_dir)
        .arg("--no-same-owner")
        .arg("--no-same-permissions")
        .output()
        .with_context(|| format!("failed to execute tar for {}", layer_file.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "failed to unpack layer archive {}: {}",
            layer_file.display(),
            stderr.trim()
        ));
    }
    Ok(())
}