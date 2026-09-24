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
use serde::Serialize;

use crate::runtime::ContainerStatus;
use crate::storage::persistence::PersistenceManager;
use crate::storage::{
    ContainerRecord, ImageRecord,
    ImageRefRecord, ContentTransferRecord,
    ContentGcCandidate, SchemaMigrationRecord,
    StateEvent, SnapshotRecord, RuntimeArtifactRecord,
    ShimProcessRecord,
};

#[derive(Debug, Clone)]
pub struct RecoveryContainerEntry {
    pub status: ContainerStatus,
    pub record: ContainerRecord,
}

#[derive(Debug, Clone, Copy)]
pub struct StateLedger<'a> {
    persistence: &'a PersistenceManager,
}

impl<'a> StateLedger<'a> {
    pub fn new(persistence: &'a PersistenceManager) -> Self {
        Self { persistence }
    }

    pub fn containers(&self) -> Result<Vec<RecoveryContainerEntry>> {
        Ok(self
            .persistence
            .recover_containers()?
            .into_iter()
            .map(|(_id, status, record)| RecoveryContainerEntry { status, record })
            .collect())
    }

    pub fn container(&self, container_id: &str) -> Result<Option<ContainerRecord>> {
        self.persistence.storage().get_container(container_id)
    }

    pub fn container_records(&self) -> Result<Vec<ContainerRecord>> {
        self.persistence.recover_containers().map(|records| {
            records
                .into_iter()
                .map(|(_id, _status, record)| record)
                .collect()
        })
    }

    pub fn images(&self) -> Result<Vec<ImageRecord>> {
        self.persistence.list_image_records()
    }

    pub fn image(&self, image_id: &str) -> Result<Option<ImageRecord>> {
        self.persistence.get_image_record(image_id)
    }

    pub fn image_refs(&self, image_id: Option<&str>) -> Result<Vec<ImageRefRecord>> {
        self.persistence.list_image_refs(image_id)
    }

    pub fn content_transfers(&self) -> Result<Vec<ContentTransferRecord>> {
        self.persistence.list_content_transfer_records()
    }

    pub fn content_gc_candidates(&self) -> Result<Vec<ContentGcCandidate>> {
        self.persistence.list_content_gc_candidates()
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.persistence.schema_version()
    }

    pub fn latest_schema_migration(&self) -> Result<Option<SchemaMigrationRecord>> {
        self.persistence.latest_schema_migration()
    }

    pub fn snapshots(&self) -> Result<Vec<SnapshotRecord>> {
        self.persistence.list_snapshot_records()
    }

    pub fn runtime_artifacts(&self) -> Result<Vec<RuntimeArtifactRecord>> {
        self.persistence.list_runtime_artifacts()
    }

    pub fn shim_processes(&self) -> Result<Vec<ShimProcessRecord>> {
        self.persistence.list_shim_process_records()
    }

    pub fn shim_process(&self, container_id: &str) -> Result<Option<ShimProcessRecord>> {
        self.persistence.get_shim_process_record(container_id)
    }

    pub fn recent_events(&self, entity_type: &str, since: i64) -> Result<Vec<StateEvent>> {
        self.persistence
            .storage()
            .get_recent_events(entity_type, since)
    }

    pub fn recent_events_for_subject(
        &self,
        subject_kind: &str,
        subject_id: &str,
        limit: usize,
    ) -> Result<Vec<StateEvent>> {
        self.persistence
            .storage()
            .get_recent_events_for_subject(subject_kind, subject_id, limit)
    }
}

#[derive(Debug)]
pub struct StateLedgerWriter<'a> {
    persistence: &'a mut PersistenceManager,
}


impl<'a> StateLedgerWriter<'a> {
    pub fn new(persistence: &'a mut PersistenceManager) -> Self {
        Self { persistence }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_container_state(
        &mut self,
        id: &str,
        pod_id: Option<&str>,
        state: ContainerStatus,
        image: &str,
        command: &[String],
        labels: &HashMap<String, String>,
        annotations: &HashMap<String, String>,
    ) -> Result<()> {
        self.persistence
            .save_container(id, pod_id, state, image, command, labels, annotations)
    }

    pub fn update_container_state(
        &mut self,
        container_id: &str,
        status: ContainerStatus,
    ) -> Result<()> {
        self.persistence
            .update_container_state(container_id, status)
    }

    pub fn update_container_ledger_metadata(
        &mut self,
        container_id: &str,
        runtime_handler: Option<&str>,
        runtime_backend: Option<&str>,
        snapshot_key: Option<&str>,
    ) -> Result<()> {
        self.persistence.update_container_ledger_metadata(
            container_id,
            runtime_handler,
            runtime_backend,
            snapshot_key,
        )
    }

    pub fn prune_events_for_subject(
        &mut self,
        subject_kind: &str,
        subject_id: &str,
        keep: usize,
    ) -> Result<usize> {
        self.persistence
            .storage_mut()
            .prune_events_for_subject(subject_kind, subject_id, keep)
    }

    pub fn append_typed_event_at(
        &mut self,
        input: crate::storage::TypedEventInput<'_>,
    ) -> Result<()> {
        self.persistence.storage_mut().append_typed_event_at(input)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LedgerCheckIssue {
    pub kind: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub severity: String,
    pub repairable: bool,
    pub message: String,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LedgerRepairAction {
    pub action: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub dry_run: bool,
    pub applied: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LedgerCheckReport {
    pub dry_run: bool,
    pub checked_at_unix_millis: i64,
    pub issue_count: usize,
    pub repairable_issue_count: usize,
    pub action_count: usize,
    pub applied_action_count: usize,
    pub issues: Vec<LedgerCheckIssue>,
    pub actions: Vec<LedgerRepairAction>,
}