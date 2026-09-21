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


pub mod persistence;

use std::path::Path;

use anyhow::{Context, Result};
use log::{info, debug};
use rusqlite::{Connection, OptionalExtension};

/// 存储管理器
#[derive(Debug)]
pub struct StorageManager {
    conn: Connection,
    db_path: std::path::PathBuf,
}

/// shim进程记录
#[derive(Debug, Clone)]
pub struct ShimProcessRecord {
    pub container_id: String,
    pub shim_pid: u32,
    pub work_dir: String,
    pub socket_path: String,
    pub exit_code_file: String,
    pub log_file: String,
    pub bundle_path: String,
    pub state: String,
    pub last_seen_at: i64,
}

/// 状态变更事件
#[derive(Debug, Clone)]
pub struct StateEvent {
    pub id: i64,
    pub event_type: String,
    pub entity_type: String,
    pub entity_id: String,
    pub old_state: String,
    pub new_state: String,
    pub timestamp: i64,
    pub details: Option<String>,
}


impl StorageManager {
    /// 创建新的存储管理器
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();

        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create database directory")?;
        }

        let conn = Connection::open(&db_path).context("Failed to open database connection")?;

        let mut manager = Self { conn, db_path };
        manager.init_tables()?;

        info!("Storage manager initialized at {:?}", manager.db_path);
        Ok(manager)
    }

    /// 初始化数据库表
    fn init_tables(&mut self) -> Result<()> {
        // 容器表
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS containers (
                id TEXT PRIMARY KEY,
                pod_id TEXT,
                state TEXT NOT NULL,
                image TEXT NOT NULL,
                command TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                labels TEXT NOT NULL DEFAULT '{}',
                annotations TEXT NOT NULL DEFAULT '{}',
                exit_code INTEGER,
                exit_time INTEGER,
                runtime_handler TEXT,
                runtime_backend TEXT,
                snapshot_key TEXT
            )",
                [],
            )
            .context("Failed to create containers table")?;

        // 镜像表
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS images (
                id TEXT PRIMARY KEY,
                size INTEGER NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                pulled_at INTEGER NOT NULL DEFAULT 0,
                source_reference TEXT,
                os TEXT,
                architecture TEXT,
                config_user TEXT,
                config_env_json TEXT NOT NULL DEFAULT '[]',
                config_entrypoint_json TEXT NOT NULL DEFAULT '[]',
                config_cmd_json TEXT NOT NULL DEFAULT '[]',
                config_working_dir TEXT,
                annotations_json TEXT NOT NULL DEFAULT '{}',
                declared_volumes_json TEXT NOT NULL DEFAULT '[]',
                manifest_media_type TEXT,
                selected_manifest_digest TEXT,
                selected_platform TEXT,
                stored_layers_json TEXT NOT NULL DEFAULT '[]',
                artifact_type TEXT,
                artifact_blobs_json TEXT NOT NULL DEFAULT '[]',
                cache_path TEXT
            )",
                [],
            )
            .context("Failed to create images table")?;

        // 镜像层
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS content_blobs (
                digest TEXT PRIMARY KEY,
                media_type TEXT NOT NULL,
                size INTEGER NOT NULL,
                relative_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER NOT NULL
            )",
                [],
            )
            .context("Failed to create content_blobs table")?;

        // 镜像层引用
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS content_blob_refs (
                owner_kind TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                digest TEXT NOT NULL,
                ref_kind TEXT NOT NULL,
                PRIMARY KEY(owner_kind, owner_id, digest, ref_kind)
            )",
                [],
            )
            .context("Failed to create content_blob_refs table")?;

        // 传输记录
         self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS content_transfers (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                provider TEXT NOT NULL,
                state TEXT NOT NULL,
                current_stage TEXT NOT NULL,
                bytes_total INTEGER NOT NULL DEFAULT 0,
                bytes_completed INTEGER NOT NULL DEFAULT 0,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                error TEXT
            )",
                [],
            )
            .context("Failed to create content_transfers table")?;

        // 镜像引用
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS image_refs (
                reference TEXT NOT NULL,
                image_id TEXT NOT NULL,
                namespace TEXT NOT NULL DEFAULT '',
                ref_kind TEXT NOT NULL,
                PRIMARY KEY(reference, image_id, namespace, ref_kind)
            )",
                [],
            )
            .context("Failed to create image_refs table")?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_image_refs_image_id ON image_refs(image_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_content_blobs_last_used ON content_blobs(last_used_at)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_content_blob_refs_digest ON content_blob_refs(digest)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_content_transfers_state ON content_transfers(state)",
            [],
        )?;

        debug!("Database tables initialized");
        Ok(())
    }

    pub fn list_images(&self) -> Result<Vec<ImageRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, size, pinned, pulled_at, source_reference, os, architecture, config_user,
                    config_env_json, config_entrypoint_json, config_cmd_json, config_working_dir,
                    annotations_json, declared_volumes_json, manifest_media_type, selected_manifest_digest,
                    selected_platform, stored_layers_json, artifact_type, artifact_blobs_json, cache_path
             FROM images",
        )?;
        let records = stmt
            .query_map([], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    size: row.get(1)?,
                    pinned: row.get(2)?,
                    pulled_at: row.get(3)?,
                    source_reference: row.get(4)?,
                    os: row.get(5)?,
                    architecture: row.get(6)?,
                    config_user: row.get(7)?,
                    config_env_json: row.get(8)?,
                    config_entrypoint_json: row.get(9)?,
                    config_cmd_json: row.get(10)?,
                    config_working_dir: row.get(11)?,
                    annotations_json: row.get(12)?,
                    declared_volumes_json: row.get(13)?,
                    manifest_media_type: row.get(14)?,
                    selected_manifest_digest: row.get(15)?,
                    selected_platform: row.get(16)?,
                    stored_layers_json: row.get(17)?,
                    artifact_type: row.get(18)?,
                    artifact_blobs_json: row.get(19)?,
                    cache_path: row.get(20)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to list images")?;
        Ok(records)
    }

    pub fn list_image_refs(&self, image_id: Option<&str>) -> Result<Vec<ImageRefRecord>> {
        let sql = if image_id.is_some() {
            "SELECT reference, image_id, namespace, ref_kind FROM image_refs WHERE image_id = ?1"
        } else {
            "SELECT reference, image_id, namespace, ref_kind FROM image_refs"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            Ok(ImageRefRecord {
                reference: row.get(0)?,
                image_id: row.get(1)?,
                namespace: {
                    let value: String = row.get(2)?;
                    (!value.is_empty()).then_some(value)
                },
                ref_kind: row.get(3)?,
            })
        };
        let records = match image_id {
            Some(id) => stmt.query_map([id], mapper)?,
            None => stmt.query_map([], mapper)?,
        }
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to list image refs")?;
        Ok(records)
    }

    pub fn get_image(&self, image_id: &str) -> Result<Option<ImageRecord>> {
        let record = self.conn.query_row(
            "SELECT id, size, pinned, pulled_at, source_reference, os, architecture, config_user,
                    config_env_json, config_entrypoint_json, config_cmd_json, config_working_dir,
                    annotations_json, declared_volumes_json, manifest_media_type, selected_manifest_digest,
                    selected_platform, stored_layers_json, artifact_type, artifact_blobs_json, cache_path
             FROM images WHERE id = ?1",
            [image_id],
            |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    size: row.get(1)?,
                    pinned: row.get(2)?,
                    pulled_at: row.get(3)?,
                    source_reference: row.get(4)?,
                    os: row.get(5)?,
                    architecture: row.get(6)?,
                    config_user: row.get(7)?,
                    config_env_json: row.get(8)?,
                    config_entrypoint_json: row.get(9)?,
                    config_cmd_json: row.get(10)?,
                    config_working_dir: row.get(11)?,
                    annotations_json: row.get(12)?,
                    declared_volumes_json: row.get(13)?,
                    manifest_media_type: row.get(14)?,
                    selected_manifest_digest: row.get(15)?,
                    selected_platform: row.get(16)?,
                    stored_layers_json: row.get(17)?,
                    artifact_type: row.get(18)?,
                    artifact_blobs_json: row.get(19)?,
                    cache_path: row.get(20)?,
                })
            },
        ).optional().context("Failed to get image")?;
        Ok(record)
    }

    pub fn save_image(&mut self, record: &ImageRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO images
             (id, size, pinned, pulled_at, source_reference, os, architecture, config_user,
              config_env_json, config_entrypoint_json, config_cmd_json, config_working_dir,
              annotations_json, declared_volumes_json, manifest_media_type, selected_manifest_digest,
              selected_platform, stored_layers_json, artifact_type, artifact_blobs_json, cache_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            rusqlite::params![
                &record.id,
                record.size,
                record.pinned,
                record.pulled_at,
                record.source_reference.as_deref(),
                record.os.as_deref(),
                record.architecture.as_deref(),
                record.config_user.as_deref(),
                &record.config_env_json,
                &record.config_entrypoint_json,
                &record.config_cmd_json,
                record.config_working_dir.as_deref(),
                &record.annotations_json,
                &record.declared_volumes_json,
                record.manifest_media_type.as_deref(),
                record.selected_manifest_digest.as_deref(),
                record.selected_platform.as_deref(),
                &record.stored_layers_json,
                record.artifact_type.as_deref(),
                &record.artifact_blobs_json,
                record.cache_path.as_deref(),
            ],
        ).context("Failed to save image")?;
        Ok(())
    }

    pub fn delete_image(&mut self, image_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM content_blob_refs WHERE owner_kind = 'image' AND owner_id = ?1",
                [image_id],
            )
            .context("Failed to delete image content blob refs")?;
        self.conn
            .execute("DELETE FROM image_refs WHERE image_id = ?1", [image_id])
            .context("Failed to delete image refs")?;
        self.conn
            .execute("DELETE FROM images WHERE id = ?1", [image_id])
            .context("Failed to delete image")?;
        Ok(())
    }

    pub fn replace_image_refs(&mut self, image_id: &str, refs: &[ImageRefRecord]) -> Result<()> {
        self.conn
            .execute("DELETE FROM image_refs WHERE image_id = ?1", [image_id])
            .context("Failed to delete old image refs")?;
        for record in refs {
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO image_refs (reference, image_id, namespace, ref_kind)
                 VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        &record.reference,
                        &record.image_id,
                        record.namespace.as_deref().unwrap_or(""),
                        &record.ref_kind,
                    ],
                )
                .context("Failed to save image ref")?;
        }
        Ok(())
    }

    pub fn replace_content_blob_refs(
        &mut self,
        owner_kind: &str,
        owner_id: &str,
        records: &[ContentBlobRefRecord],
    ) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM content_blob_refs WHERE owner_kind = ?1 AND owner_id = ?2",
                [owner_kind, owner_id],
            )
            .context("Failed to delete content blob refs")?;
        for record in records {
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO content_blob_refs
                     (owner_kind, owner_id, digest, ref_kind)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        &record.owner_kind,
                        &record.owner_id,
                        &record.digest,
                        &record.ref_kind,
                    ],
                )
                .context("Failed to save content blob ref")?;
        }
        Ok(())
    }

    pub fn save_content_transfer(&mut self, record: &ContentTransferRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO content_transfers
                 (id, source, provider, state, current_stage, bytes_total, bytes_completed, started_at, finished_at, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                   source = excluded.source,
                   provider = excluded.provider,
                   state = excluded.state,
                   current_stage = excluded.current_stage,
                   bytes_total = excluded.bytes_total,
                   bytes_completed = excluded.bytes_completed,
                   finished_at = excluded.finished_at,
                   error = excluded.error",
                rusqlite::params![
                    &record.id,
                    &record.source,
                    &record.provider,
                    &record.state,
                    &record.current_stage,
                    record.bytes_total as i64,
                    record.bytes_completed as i64,
                    record.started_at,
                    record.finished_at,
                    record.error.as_deref(),
                ],
            )
            .context("Failed to save content transfer")?;
        Ok(())
    }

    pub fn append_typed_event_at(&mut self, input: TypedEventInput<'_>) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO events
                 (event_type, entity_type, entity_id, old_state, new_state, timestamp, details)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    input.event_type,
                    input.entity_type,
                    input.entity_id,
                    input.old_state.unwrap_or(""),
                    input.new_state.unwrap_or(""),
                    input.timestamp,
                    input.details,
                ],
            )
            .context("Failed to append state event")?;
        Ok(())
    }

    pub fn get_content_blob(&self, digest: &str) -> Result<Option<ContentBlobRecord>> {
        self.conn
            .query_row(
                "SELECT digest, media_type, size, relative_path, created_at, last_used_at
                 FROM content_blobs WHERE digest = ?1",
                [digest],
                |row| {
                    let size: i64 = row.get(2)?;
                    Ok(ContentBlobRecord {
                        digest: row.get(0)?,
                        media_type: row.get(1)?,
                        size: size.max(0) as u64,
                        relative_path: row.get(3)?,
                        created_at: row.get(4)?,
                        last_used_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .context("Failed to get content blob")
    }

    pub fn touch_content_blob(&mut self, digest: &str, last_used_at: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE content_blobs SET last_used_at = ?2 WHERE digest = ?1",
                rusqlite::params![digest, last_used_at],
            )
            .context("Failed to touch content blob")?;
        Ok(())
    }

    pub fn save_content_blob(&mut self, record: &ContentBlobRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO content_blobs
                 (digest, media_type, size, relative_path, created_at, last_used_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(digest) DO UPDATE SET
                   media_type = excluded.media_type,
                   size = excluded.size,
                   relative_path = excluded.relative_path,
                   last_used_at = excluded.last_used_at",
                rusqlite::params![
                    &record.digest,
                    &record.media_type,
                    record.size as i64,
                    &record.relative_path,
                    record.created_at,
                    record.last_used_at,
                ],
            )
            .context("Failed to save content blob")?;
        Ok(())
    }

    pub fn delete_content_blob(&mut self, digest: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM content_blob_refs WHERE digest = ?1", [digest])
            .context("Failed to delete content blob refs")?;
        self.conn
            .execute("DELETE FROM content_blobs WHERE digest = ?1", [digest])
            .context("Failed to delete content blob")?;
        Ok(())
    }

    pub fn update_snapshot_state(&self, _snapshot_key: &str, _state: &str) -> Result<()> {
        Ok(())
    }

    pub fn delete_snapshot(&self, _snapshot_key: &str) -> Result<()> {
        Ok(())
    }

    pub fn list_shim_processes(&self) -> Result<Vec<ShimProcessRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT container_id, shim_pid, work_dir, socket_path, exit_code_file, log_file, bundle_path, state, last_seen_at
             FROM shim_processes",
        )?;
        let records = stmt
            .query_map([], |row| {
                Ok(ShimProcessRecord {
                    container_id: row.get(0)?,
                    shim_pid: row.get(1)?,
                    work_dir: row.get(2)?,
                    socket_path: row.get(3)?,
                    exit_code_file: row.get(4)?,
                    log_file: row.get(5)?,
                    bundle_path: row.get(6)?,
                    state: row.get(7)?,
                    last_seen_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to list shim processes")?;
        Ok(records)
    }

    pub fn save_shim_process(&mut self, record: &ShimProcessRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO shim_processes
             (container_id, shim_pid, work_dir, socket_path, exit_code_file, log_file, bundle_path, state, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                &record.container_id,
                record.shim_pid,
                &record.work_dir,
                &record.socket_path,
                &record.exit_code_file,
                &record.log_file,
                &record.bundle_path,
                &record.state,
                record.last_seen_at,
            ],
        ).context("Failed to save shim process")?;
        Ok(())
    }

    /// 保存容器记录
    pub fn save_container(&mut self, record: &ContainerRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO containers 
             (id, pod_id, state, image, command, created_at, labels, annotations, exit_code, exit_time, runtime_handler, runtime_backend, snapshot_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                &record.id,
                &record.pod_id,
                &record.state,
                &record.image,
                &record.command,
                record.created_at,
                &record.labels,
                &record.annotations,
                record.exit_code,
                record.exit_time,
                record.runtime_handler.as_deref(),
                record.runtime_backend.as_deref(),
                record.snapshot_key.as_deref(),
            ],
        ).context("Failed to save container")?;

        // 记录状态变更事件
        self.record_state_event("container", &record.id, None, Some(&record.state))?;

        debug!("Container {} saved to database", record.id);
        Ok(())
    }

    /// 获取容器记录
    pub fn get_container(&self, container_id: &str) -> Result<Option<ContainerRecord>> {
        let record = self.conn.query_row(
            "SELECT id, pod_id, state, image, command, created_at, labels, annotations, exit_code, exit_time, runtime_handler, runtime_backend, snapshot_key
             FROM containers WHERE id = ?1",
            [container_id],
            |row| {
                Ok(ContainerRecord {
                    id: row.get(0)?,
                    pod_id: row.get(1)?,
                    state: row.get(2)?,
                    image: row.get(3)?,
                    command: row.get(4)?,
                    created_at: row.get(5)?,
                    labels: row.get(6)?,
                    annotations: row.get(7)?,
                    exit_code: row.get(8)?,
                    exit_time: row.get(9)?,
                    runtime_handler: row.get(10)?,
                    runtime_backend: row.get(11)?,
                    snapshot_key: row.get(12)?,
                })
            },
        ).optional().context("Failed to get container")?;

        Ok(record)
    }

    /// 删除容器记录
    pub fn delete_container(&mut self, container_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM containers WHERE id = ?1", [container_id])
            .context("Failed to delete container")?;

        debug!("Container {} deleted from database", container_id);
        Ok(())
    }

    /// 更新容器状态
    pub fn update_container_state(
        &mut self,
        container_id: &str,
        new_state: &str,
        exit_code: Option<i32>,
    ) -> Result<()> {
        // 获取旧状态
        let old_state: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM containers WHERE id = ?1",
                [container_id],
                |row| row.get(0),
            )
            .optional()?;

        let exit_time = if exit_code.is_some() {
            Some(chrono::Utc::now().timestamp())
        } else {
            None
        };

        self.conn
            .execute(
                "UPDATE containers SET state = ?1, exit_code = ?2, exit_time = ?3 WHERE id = ?4",
                rusqlite::params![new_state, exit_code, exit_time, container_id,],
            )
            .context("Failed to update container state")?;

        // 记录状态变更事件
        if old_state.as_deref() != Some(new_state) {
            self.record_state_event(
                "container",
                container_id,
                old_state.as_deref(),
                Some(new_state),
            )?;
        }

        debug!("Container {} state updated to {}", container_id, new_state);
        Ok(())
    }

    /// 记录状态变更事件
    fn record_state_event(
        &mut self,
        entity_type: &str,
        entity_id: &str,
        old_state: Option<&str>,
        new_state: Option<&str>,
    ) -> Result<()> {
        let event_type = Self::ledger_event_type(entity_type);
        self.append_typed_event(
            event_type,
            entity_type,
            entity_id,
            old_state,
            new_state,
            None,
        )
    }

    fn ledger_event_type(entity_type: &str) -> &'static str {
        match entity_type {
            "pod" => "pod",
            "shim_task" => "task",
            "shim_exec" => "shim",
            value if value.starts_with("reconcile") => "reconcile",
            _ => "container",
        }
    }

    pub fn append_event(
        &mut self,
        entity_type: &str,
        entity_id: &str,
        old_state: Option<&str>,
        new_state: Option<&str>,
        details: Option<&str>,
    ) -> Result<()> {
        let event_type = Self::ledger_event_type(entity_type);
        self.append_typed_event(
            event_type,
            entity_type,
            entity_id,
            old_state,
            new_state,
            details,
        )
    }

    pub fn append_typed_event(
        &mut self,
        event_type: &str,
        entity_type: &str,
        entity_id: &str,
        old_state: Option<&str>,
        new_state: Option<&str>,
        details: Option<&str>,
    ) -> Result<()> {
        let timestamp = chrono::Utc::now().timestamp();
        self.append_typed_event_at(TypedEventInput {
            event_type,
            entity_type,
            entity_id,
            old_state,
            new_state,
            details,
            timestamp,
        })
    }

    /// 获取最近的实体状态事件
    pub fn get_recent_events(&self, entity_type: &str, since: i64) -> Result<Vec<StateEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_type, entity_type, entity_id, old_state, new_state, timestamp, details
             FROM events
             WHERE entity_type = ?1 AND timestamp >= ?2
             ORDER BY timestamp DESC",
        )?;

        let events = stmt
            .query_map([entity_type, &since.to_string()], |row| {
                Ok(StateEvent {
                    id: row.get(0)?,
                    event_type: row.get(1)?,
                    entity_type: row.get(2)?,
                    entity_id: row.get(3)?,
                    old_state: row.get(4)?,
                    new_state: row.get(5)?,
                    timestamp: row.get(6)?,
                    details: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get recent events")?;

        Ok(events)
    }

    pub fn get_recent_events_for_subject(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<StateEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_type, entity_type, entity_id, old_state, new_state, timestamp, details
             FROM events
             WHERE entity_type = ?1 AND entity_id = ?2
             ORDER BY timestamp DESC, id DESC
             LIMIT ?3",
        )?;

        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let events = stmt
            .query_map(rusqlite::params![entity_type, entity_id, limit], |row| {
                Ok(StateEvent {
                    id: row.get(0)?,
                    event_type: row.get(1)?,
                    entity_type: row.get(2)?,
                    entity_id: row.get(3)?,
                    old_state: row.get(4)?,
                    new_state: row.get(5)?,
                    timestamp: row.get(6)?,
                    details: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to get recent events for subject")?;

        Ok(events)
    }

    pub fn prune_events_for_subject(
        &mut self,
        entity_type: &str,
        entity_id: &str,
        keep: usize,
    ) -> Result<usize> {
        let keep = i64::try_from(keep).unwrap_or(i64::MAX);
        let deleted = self
            .conn
            .execute(
                "DELETE FROM events
                 WHERE entity_type = ?1
                   AND entity_id = ?2
                   AND id NOT IN (
                     SELECT id FROM events
                     WHERE entity_type = ?1 AND entity_id = ?2
                     ORDER BY timestamp DESC, id DESC
                     LIMIT ?3
                   )",
                rusqlite::params![entity_type, entity_id, keep],
            )
            .context("Failed to prune events for subject")?;
        Ok(deleted)
    }

    /// 关闭数据库连接
    pub fn close(self) -> Result<()> {
        self.conn
            .close()
            .map_err(|e| anyhow::anyhow!("Failed to close database: {:?}", e))?;
        Ok(())
    }
}

/// 镜像记录
#[derive(Debug, Clone)]
pub struct ImageRecord {
    pub id: String,
    pub size: u64,
    pub pinned: bool,
    pub pulled_at: i64,
    pub source_reference: Option<String>,
    pub os: Option<String>,
    pub architecture: Option<String>,
    pub config_user: Option<String>,
    pub config_env_json: String,
    pub config_entrypoint_json: String,
    pub config_cmd_json: String,
    pub config_working_dir: Option<String>,
    pub annotations_json: String,
    pub declared_volumes_json: String,
    pub manifest_media_type: Option<String>,
    pub selected_manifest_digest: Option<String>,
    pub selected_platform: Option<String>,
    pub stored_layers_json: String,
    pub artifact_type: Option<String>,
    pub artifact_blobs_json: String,
    pub cache_path: Option<String>,
}

/// 镜像引用记录
#[derive(Debug, Clone)]
pub struct ImageRefRecord {
    pub reference: String,
    pub image_id: String,
    pub namespace: Option<String>,
    pub ref_kind: String,
}

/// 内容 blob 引用记录
#[derive(Debug, Clone)]
pub struct ContentBlobRefRecord {
    pub owner_kind: String,
    pub owner_id: String,
    pub digest: String,
    pub ref_kind: String,
}

pub struct TypedEventInput<'a> {
    pub event_type: &'a str,
    pub entity_type: &'a str,
    pub entity_id: &'a str,
    pub old_state: Option<&'a str>,
    pub new_state: Option<&'a str>,
    pub details: Option<&'a str>,
    pub timestamp: i64,
}

/// 内容 blob 记录
#[derive(Debug, Clone)]
pub struct ContentBlobRecord {
    pub digest: String,
    pub media_type: String,
    pub size: u64,
    pub relative_path: String,
    pub created_at: i64,
    pub last_used_at: i64,
}

#[derive(Debug, Clone)]
pub struct ContentTransferRecord {
    pub id: String,
    pub source: String,
    pub provider: String,
    pub state: String,
    pub current_stage: String,
    pub bytes_total: u64,
    pub bytes_completed: u64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
}

/// 容器记录
#[derive(Debug, Clone)]
pub struct ContainerRecord {
    pub id: String,
    pub pod_id: Option<String>,
    pub state: String,
    pub image: String,
    pub command: String,
    pub created_at: i64,
    pub labels: String,      // JSON
    pub annotations: String, // JSON
    pub exit_code: Option<i32>,
    pub exit_time: Option<i64>,
    pub runtime_handler: Option<String>,
    pub runtime_backend: Option<String>,
    pub snapshot_key: Option<String>,
}