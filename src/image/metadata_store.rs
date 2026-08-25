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
use anyhow::{Context, Result};

use crate::image::ImageMeta;
use crate::storage::{StorageManager, ImageRecord, ImageRefRecord};

use super::Image;

#[derive(Debug, Clone)]
pub struct StoredImageRecord {
    pub meta: ImageMeta,
    pub record_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FilesystemImageMetadataStore {
    storage_root: PathBuf,
    additional_artifact_stores: Vec<PathBuf>,
    ledger_db_path: Option<PathBuf>,
}

impl FilesystemImageMetadataStore {
    pub fn new(
        storage_root: impl AsRef<Path>,
        additional_artifact_stores: Vec<PathBuf>,
        ledger_db_path: Option<PathBuf>,
    ) -> Self {
        Self {
            storage_root: storage_root.as_ref().to_path_buf(),
            additional_artifact_stores,
            ledger_db_path,
        }
    }
}

impl FilesystemImageMetadataStore {
    
    pub fn find_by_reference<F>(
        &self,
        requested_ref: &str,
        matcher: F,
    ) -> Result<Option<StoredImageRecord>>
    where
        F: Fn(&Image, &str) -> bool,
    {
        for record in self.load_all()? {
            let image = image_from_meta(&record.meta);
            if matcher(&image, requested_ref) {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    pub fn image_records_dir(root: &Path) -> PathBuf {
        root.join("images")
    }

    pub fn artifact_records_dir(root: &Path) -> PathBuf {
        root.join("artifacts")
    }

    pub fn load_meta_from_record_dir(record_dir: &Path) -> Option<ImageMeta> {
        let raw = std::fs::read(record_dir.join("metadata.json")).ok()?;
        serde_json::from_slice(&raw).ok()
    }

    pub fn load_all(&self) -> Result<Vec<StoredImageRecord>> {
        if let Some(db_path) = self.ledger_db_path.as_ref() {
            let storage = StorageManager::new(db_path)?;
            let records = storage.list_images()?;
            if !records.is_empty() {
                let refs = storage.list_image_refs(None)?;
                return Ok(records
                    .into_iter()
                    .map(|record| {
                        let image_id = record.id.clone();
                        StoredImageRecord {
                            meta: meta_from_record(
                                &record,
                                Some(
                                    refs.iter()
                                        .filter(|candidate| candidate.image_id == image_id)
                                        .cloned()
                                        .collect(),
                                ),
                            ),
                            record_dir: record
                                .cache_path
                                .as_deref()
                                .map(PathBuf::from)
                                .unwrap_or_else(|| {
                                    Self::image_records_dir(&self.storage_root).join(&image_id)
                                }),
                        }
                    })
                    .collect());
            }
        }
        let mut records = Vec::new();
        for root in std::iter::once(self.storage_root.as_path())
            .chain(self.additional_artifact_stores.iter().map(PathBuf::as_path))
        {
            for records_dir in [
                Self::image_records_dir(root),
                Self::artifact_records_dir(root),
            ] {
                if !records_dir.exists() {
                    continue;
                }
                for entry in std::fs::read_dir(&records_dir)
                    .with_context(|| format!("failed to read {}", records_dir.display()))?
                {
                    let entry = entry?;
                    let record_dir = entry.path();
                    let Some(meta) = Self::load_meta_from_record_dir(&record_dir) else {
                        continue;
                    };
                    records.push(StoredImageRecord { meta, record_dir });
                }
            }
        }
        Ok(records)
    }
}

fn meta_from_record(record: &ImageRecord, refs: Option<Vec<ImageRefRecord>>) -> ImageMeta {
    let refs = refs.unwrap_or_default();
    let repo_tags = refs
        .iter()
        .filter(|record| record.ref_kind == "tag")
        .map(|record| record.reference.clone())
        .collect();
    let repo_digests = refs
        .iter()
        .filter(|record| record.ref_kind == "digest")
        .map(|record| record.reference.clone())
        .collect();
    ImageMeta {
        id: record.id.clone(),
        repo_tags,
        repo_digests,
        size: record.size,
        pinned: record.pinned,
        pulled_at: record.pulled_at,
        source_reference: record.source_reference.clone(),
        os: record.os.clone(),
        architecture: record.architecture.clone(),
        config_user: record.config_user.clone(),
        config_env: serde_json::from_str(&record.config_env_json).unwrap_or_default(),
        config_entrypoint: serde_json::from_str(&record.config_entrypoint_json).unwrap_or_default(),
        config_cmd: serde_json::from_str(&record.config_cmd_json).unwrap_or_default(),
        config_working_dir: record.config_working_dir.clone(),
        annotations: serde_json::from_str(&record.annotations_json).unwrap_or_default(),
        declared_volumes: serde_json::from_str(&record.declared_volumes_json).unwrap_or_default(),
        manifest_media_type: record.manifest_media_type.clone(),
        selected_manifest_digest: record.selected_manifest_digest.clone(),
        selected_platform: record.selected_platform.clone(),
        stored_layers: serde_json::from_str(&record.stored_layers_json).unwrap_or_default(),
        artifact_type: record.artifact_type.clone(),
        artifact_blobs: serde_json::from_str(&record.artifact_blobs_json).unwrap_or_default(),
    }
}

fn image_from_meta(meta: &ImageMeta) -> Image {
    let requested = meta
        .source_reference
        .clone()
        .or_else(|| meta.repo_tags.first().cloned())
        .unwrap_or_default();
    Image {
        id: meta.id.clone(),
        repo_tags: meta.repo_tags.clone(),
        repo_digests: meta.repo_digests.clone(),
        size: meta.size,
        pinned: meta.pinned,
        uid: meta
            .config_user
            .as_ref()
            .map(|user| crate::proto::runtime::v1::Int64Value {
                value: user.parse::<i64>().unwrap_or(0),
            }),
        username: meta.config_user.clone().unwrap_or_default(),
        spec: Some(crate::proto::runtime::v1::ImageSpec {
            image: requested.clone(),
            user_specified_image: requested,
            annotations: meta.annotations.clone(),
            runtime_handler: String::new(),
        }),
    }
}