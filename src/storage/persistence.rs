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

use anyhow::Result;

use crate::storage::{
    StorageManager, ContainerRecord,
    ContentGcCandidate, ImageRecord,
    ImageRefRecord, ContentTransferRecord,
    SchemaMigrationRecord, SnapshotRecord,
    RuntimeArtifactRecord, ShimProcessRecord,
};
use crate::runtime::ContainerStatus;

/// 状态持久化配置
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    /// 数据库文件路径
    pub db_path: std::path::PathBuf,
    /// 是否启用状态恢复
    pub enable_recovery: bool,
    /// 自动保存间隔（秒）
    pub auto_save_interval: u64,
}

/// 持久化管理器
#[derive(Debug)]
pub struct PersistenceManager {
    storage: StorageManager,
    _config: PersistenceConfig,
}

impl PersistenceManager {
    /// 创建新的持久化管理器
    pub fn new(config: PersistenceConfig) -> Result<Self> {
        let storage = StorageManager::new(&config.db_path)?;

        Ok(Self {
            storage,
            _config: config,
        })
    }

    /// 获取存储管理器的可变引用
    pub fn storage_mut(&mut self) -> &mut StorageManager {
        &mut self.storage
    }

    pub fn update_container_ledger_metadata(
        &mut self,
        container_id: &str,
        runtime_handler: Option<&str>,
        runtime_backend: Option<&str>,
        snapshot_key: Option<&str>,
    ) -> Result<()> {
        let Some(mut record) = self.storage.get_container(container_id)? else {
            return Ok(());
        };
        record.runtime_handler = runtime_handler.map(ToString::to_string);
        record.runtime_backend = runtime_backend.map(ToString::to_string);
        record.snapshot_key = snapshot_key.map(ToString::to_string);
        self.storage.save_container(&record)
    }

    /// 保存容器状态
    #[allow(clippy::too_many_arguments)]
    pub fn save_container(
        &mut self,
        id: &str,
        pod_id: Option<&str>,
        state: ContainerStatus,
        image: &str,
        command: &[String],
        labels: &HashMap<String, String>,
        annotations: &HashMap<String, String>,
    ) -> Result<()> {
        let record = container_to_record(id, pod_id, state, image, command, labels, annotations);
        self.storage.save_container(&record)
    }

    /// 更新容器状态
    pub fn update_container_state(
        &mut self,
        container_id: &str,
        new_state: ContainerStatus,
    ) -> Result<()> {
        let (state_str, exit_code) = match new_state {
            ContainerStatus::Created => ("created", None),
            ContainerStatus::Running => ("running", None),
            ContainerStatus::Stopped(code) => ("stopped", Some(code)),
            ContainerStatus::Unknown => ("unknown", None),
        };

        self.storage
            .update_container_state(container_id, state_str, exit_code)
    }

    /// 删除容器记录
    pub fn delete_container(&mut self, container_id: &str) -> Result<()> {
        self.storage.delete_container(container_id)
    }

    /// 恢复所有容器状态
    pub fn recover_containers(&self) -> Result<Vec<(String, ContainerStatus, ContainerRecord)>> {
        let records = self.storage.list_containers()?;

        let mut result = Vec::new();
        for record in records {
            let status = record_to_container_status(&record);
            result.push((record.id.clone(), status, record));
        }

        Ok(result)
    }

    pub fn list_content_gc_candidates(&self) -> Result<Vec<ContentGcCandidate>> {
        self.storage.list_content_gc_candidates()
    }

    /// 获取存储管理器的引用
    pub fn storage(&self) -> &StorageManager {
        &self.storage
    }

    pub fn save_image_record(
        &mut self,
        record: &ImageRecord,
        refs: &[ImageRefRecord],
    ) -> Result<()> {
        self.storage.save_image(record)?;
        self.storage.replace_image_refs(&record.id, refs)
    }

    pub fn get_image_record(&self, image_id: &str) -> Result<Option<ImageRecord>> {
        self.storage.get_image(image_id)
    }

    pub fn list_image_records(&self) -> Result<Vec<ImageRecord>> {
        self.storage.list_images()
    }

    pub fn list_image_refs(&self, image_id: Option<&str>) -> Result<Vec<ImageRefRecord>> {
        self.storage.list_image_refs(image_id)
    }

    pub fn list_content_transfer_records(&self) -> Result<Vec<ContentTransferRecord>> {
        self.storage.list_content_transfers()
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.storage.schema_version()
    }

    pub fn latest_schema_migration(&self) -> Result<Option<SchemaMigrationRecord>> {
        self.storage.latest_schema_migration()
    }

    pub fn list_snapshot_records(&self) -> Result<Vec<SnapshotRecord>> {
        self.storage.list_snapshots()
    }

    pub fn list_runtime_artifacts(&self) -> Result<Vec<RuntimeArtifactRecord>> {
        self.storage.list_runtime_artifacts()
    }

    pub fn list_shim_process_records(&self) -> Result<Vec<ShimProcessRecord>> {
        self.storage.list_shim_processes()
    }

    pub fn get_shim_process_record(&self, container_id: &str) -> Result<Option<ShimProcessRecord>> {
        self.storage.get_shim_process(container_id)
    }
}

/// 将容器配置转换为存储记录
pub fn container_to_record(
    id: &str,
    pod_id: Option<&str>,
    state: ContainerStatus,
    image: &str,
    command: &[String],
    labels: &HashMap<String, String>,
    annotations: &HashMap<String, String>,
) -> ContainerRecord {
    let state_str = match state {
        ContainerStatus::Created => "created",
        ContainerStatus::Running => "running",
        ContainerStatus::Stopped(code) => {
            return ContainerRecord {
                id: id.to_string(),
                pod_id: pod_id.map(ToString::to_string),
                state: "stopped".to_string(),
                image: image.to_string(),
                command: command.join(" "),
                created_at: chrono::Utc::now().timestamp(),
                labels: serde_json::to_string(labels).unwrap_or_default(),
                annotations: serde_json::to_string(annotations).unwrap_or_default(),
                exit_code: Some(code),
                exit_time: Some(chrono::Utc::now().timestamp()),
                runtime_handler: None,
                runtime_backend: None,
                snapshot_key: None,
            }
        }
        ContainerStatus::Unknown => "unknown",
    };

    ContainerRecord {
        id: id.to_string(),
        pod_id: pod_id.map(ToString::to_string),
        state: state_str.to_string(),
        image: image.to_string(),
        command: command.join(" "),
        created_at: chrono::Utc::now().timestamp(),
        labels: serde_json::to_string(labels).unwrap_or_default(),
        annotations: serde_json::to_string(annotations).unwrap_or_default(),
        exit_code: None,
        exit_time: None,
        runtime_handler: None,
        runtime_backend: None,
        snapshot_key: None,
    }
}

/// 从存储记录恢复容器状态
pub fn record_to_container_status(record: &ContainerRecord) -> ContainerStatus {
    match record.state.as_str() {
        "created" => ContainerStatus::Created,
        "running" => ContainerStatus::Running,
        "stopped" => ContainerStatus::Stopped(record.exit_code.unwrap_or(-1)),
        _ => ContainerStatus::Unknown,
    }
}