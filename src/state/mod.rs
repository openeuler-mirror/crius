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


use std::collections::{
    HashMap, HashSet,
};
use std::fmt;
use std::path::Path;
use std::unimplemented;

use anyhow::{
    Result, Context,
};
use serde::Serialize;
use serde_json::json;

use crate::runtime::ContainerStatus;
use crate::storage::persistence::PersistenceManager;
use crate::storage::{
    ContainerRecord, ImageRecord,
    ImageRefRecord, ContentTransferRecord,
    ContentGcCandidate, SchemaMigrationRecord,
    StateEvent, SnapshotRecord, RuntimeArtifactRecord,
    ShimProcessRecord, PodSandboxRecord,
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

    pub fn pod_sandboxes(&self) -> Result<Vec<PodSandboxRecord>> {
        unimplemented!()
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

    pub fn recovery_snapshot(&self) -> Result<RecoveryLedgerSnapshot> {
        let containers = self.containers()?;
        let pods = self.pod_sandboxes()?;
        let images = self.images()?;
        let image_refs = self.image_refs(None)?;
        let snapshots = self.snapshots()?;
        let runtime_artifacts = self.runtime_artifacts()?;
        let shim_processes = self.shim_processes()?;

        Ok(RecoveryLedgerSnapshot {
            containers,
            pods,
            images,
            image_refs,
            snapshots,
            runtime_artifacts,
            shim_processes,
        })
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
    
    pub fn repair(
        &mut self,
        options: LedgerRepairOptions,
        dry_run: bool,
    ) -> Result<LedgerCheckReport> {
        let snapshot = StateLedger::new(self.persistence).recovery_snapshot()?;
        let mut plan = snapshot.check_plan(
            LedgerCheckOptions {
                check_files: options.check_files,
            },
            dry_run,
        );

        if dry_run {
            return Ok(plan.report);
        }

        let mut actions = Vec::with_capacity(plan.operations.len());
        for operation in plan.operations {
            actions.push(self.apply_repair_operation(operation)?);
        }
        plan.report.dry_run = false;
        plan.report.action_count = actions.len();
        plan.report.applied_action_count = actions.iter().filter(|action| action.applied).count();
        plan.report.actions = actions;
        Ok(plan.report)
    }

    fn apply_repair_operation(
        &mut self,
        operation: LedgerRepairOperation,
    ) -> Result<LedgerRepairAction> {
        match operation {
            LedgerRepairOperation::DeleteOrphanSnapshot { key } => {
                let mut applied = false;
                if let Some(record) = self
                    .persistence
                    .list_snapshot_records()?
                    .into_iter()
                    .find(|record| record.key == key)
                {
                    if !record.mountpoint.trim().is_empty() {
                        let path = std::path::Path::new(&record.mountpoint);
                        if path.exists() {
                            if path.is_dir() {
                                std::fs::remove_dir_all(path).with_context(|| {
                                    format!(
                                        "failed to remove snapshot mountpoint {}",
                                        record.mountpoint
                                    )
                                })?;
                            } else {
                                std::fs::remove_file(path).with_context(|| {
                                    format!(
                                        "failed to remove snapshot mountpoint {}",
                                        record.mountpoint
                                    )
                                })?;
                            }
                        }
                    }
                    self.persistence.storage_mut().delete_snapshot(&key)?;
                    applied = true;
                }
                Ok(LedgerRepairAction {
                    action: "deleteOrphanSnapshot".to_string(),
                    subject_kind: "snapshot".to_string(),
                    subject_id: key.clone(),
                    dry_run: false,
                    applied,
                    message: format!("deleted orphan snapshot {key}"),
                })
            }
            LedgerRepairOperation::MarkRuntimeArtifactBroken {
                owner_kind,
                owner_id,
                artifact_kind,
                path,
            } => {
                let mut applied = false;
                if let Some(record) =
                    self.persistence
                        .list_runtime_artifacts()?
                        .into_iter()
                        .find(|record| {
                            record.owner_kind == owner_kind
                                && record.owner_id == owner_id
                                && record.artifact_kind == artifact_kind
                                && record.path == path
                        })
                {
                    if record.state != RuntimeArtifactLedgerState::Broken.as_str() {
                        self.persistence
                            .storage_mut()
                            .update_runtime_artifact_state(
                                &owner_kind,
                                &owner_id,
                                &artifact_kind,
                                &path,
                                RuntimeArtifactLedgerState::Broken.as_str(),
                            )?;
                        applied = true;
                    }
                }
                Ok(LedgerRepairAction {
                    action: "markRuntimeArtifactBroken".to_string(),
                    subject_kind: "runtimeArtifact".to_string(),
                    subject_id: path.clone(),
                    dry_run: false,
                    applied,
                    message: format!("marked {artifact_kind} runtime artifact {path} broken"),
                })
            }
            LedgerRepairOperation::MarkShimDead { container_id } => {
                let mut applied = false;
                if let Some(record) = self.persistence.get_shim_process_record(&container_id)? {
                    if record.state != ShimLedgerState::Dead.as_str() {
                        self.persistence.storage_mut().update_shim_process_state(
                            &container_id,
                            ShimLedgerState::Dead.as_str(),
                        )?;
                        applied = true;
                    }
                }
                Ok(LedgerRepairAction {
                    action: "markShimDead".to_string(),
                    subject_kind: "shim".to_string(),
                    subject_id: container_id.clone(),
                    dry_run: false,
                    applied,
                    message: format!("marked shim {container_id} dead"),
                })
            }
            LedgerRepairOperation::DeleteDanglingShim { container_id } => {
                let existed = self
                    .persistence
                    .get_shim_process_record(&container_id)?
                    .is_some();
                if existed {
                    self.persistence.delete_shim_process_record(&container_id)?;
                }
                Ok(LedgerRepairAction {
                    action: "deleteDanglingShim".to_string(),
                    subject_kind: "shim".to_string(),
                    subject_id: container_id.clone(),
                    dry_run: false,
                    applied: existed,
                    message: format!("deleted dangling shim record for {container_id}"),
                })
            }
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerCheckOptions {
    pub check_files: bool,
}

impl Default for LedgerCheckOptions {
    fn default() -> Self {
        Self { check_files: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LedgerRepairOperation {
    DeleteOrphanSnapshot {
        key: String,
    },
    MarkRuntimeArtifactBroken {
        owner_kind: String,
        owner_id: String,
        artifact_kind: String,
        path: String,
    },
    MarkShimDead {
        container_id: String,
    },
    DeleteDanglingShim {
        container_id: String,
    },
}

impl LedgerRepairOperation {
    fn dry_run_action(&self) -> LedgerRepairAction {
        let (action, subject_kind, subject_id, message) = match self {
            Self::DeleteOrphanSnapshot { key } => (
                "deleteOrphanSnapshot",
                "snapshot",
                key.as_str(),
                format!("would delete orphan snapshot {key}"),
            ),
            Self::MarkRuntimeArtifactBroken {
                artifact_kind,
                path,
                ..
            } => (
                "markRuntimeArtifactBroken",
                "runtimeArtifact",
                path.as_str(),
                format!("would mark {artifact_kind} runtime artifact {path} broken"),
            ),
            Self::MarkShimDead { container_id } => (
                "markShimDead",
                "shim",
                container_id.as_str(),
                format!("would mark shim {container_id} dead"),
            ),
            Self::DeleteDanglingShim { container_id } => (
                "deleteDanglingShim",
                "shim",
                container_id.as_str(),
                format!("would delete dangling shim record for {container_id}"),
            ),
        };
        LedgerRepairAction {
            action: action.to_string(),
            subject_kind: subject_kind.to_string(),
            subject_id: subject_id.to_string(),
            dry_run: true,
            applied: false,
            message,
        }
    }
}

#[derive(Debug, Clone)]
struct LedgerCheckPlan {
    report: LedgerCheckReport,
    operations: Vec<LedgerRepairOperation>,
}

#[derive(Debug, Clone, Default)]
pub struct RecoveryLedgerSnapshot {
    pub containers: Vec<RecoveryContainerEntry>,
    pub pods: Vec<PodSandboxRecord>,
    pub images: Vec<ImageRecord>,
    pub image_refs: Vec<ImageRefRecord>,
    pub snapshots: Vec<SnapshotRecord>,
    pub runtime_artifacts: Vec<RuntimeArtifactRecord>,
    pub shim_processes: Vec<ShimProcessRecord>,
}

impl RecoveryLedgerSnapshot {
    pub fn load(persistence: &PersistenceManager) -> Result<Self> {
        StateLedger::new(persistence).recovery_snapshot()
    }

    fn check_plan(&self, options: LedgerCheckOptions, dry_run: bool) -> LedgerCheckPlan {
        let pod_ids: HashSet<_> = self.pods.iter().map(|pod| pod.id.as_str()).collect();
        let container_ids: HashSet<_> = self
            .containers
            .iter()
            .map(|entry| entry.record.id.as_str())
            .collect();
        let image_ids: HashSet<_> = self.images.iter().map(|image| image.id.as_str()).collect();
        let snapshot_keys: HashSet<_> = self
            .snapshots
            .iter()
            .map(|snapshot| snapshot.key.as_str())
            .collect();

        let mut issues = Vec::new();
        let mut operations = Vec::new();

        for entry in &self.containers {
            let container = &entry.record;
            if let Some(pod_id) = container.pod_id.as_deref() {
                if !pod_ids.contains(pod_id) {
                    push_issue(
                        &mut issues,
                        "ownerGraphBroken",
                        "container",
                        &container.id,
                        false,
                        format!(
                            "container {} references missing pod {}",
                            container.id, pod_id
                        ),
                        json!({ "missingOwnerKind": "pod", "missingOwnerId": pod_id }),
                    );
                }
            }

            if let Some(snapshot_key) = &container.snapshot_key {
                if !snapshot_keys.contains(snapshot_key.as_str()) {
                    push_issue(
                        &mut issues,
                        "danglingRef",
                        "container",
                        &container.id,
                        false,
                        format!(
                            "container {} references missing snapshot {}",
                            container.id, snapshot_key
                        ),
                        json!({ "missingRefKind": "snapshot", "missingRefId": snapshot_key }),
                    );
                }
            }
        }

        for pod in &self.pods {
            if let Some(pause_container_id) = &pod.pause_container_id {
                if !container_ids.contains(pause_container_id.as_str()) {
                    push_issue(
                        &mut issues,
                        "danglingRef",
                        "pod",
                        &pod.id,
                        false,
                        format!(
                            "pod {} references missing pause container {}",
                            pod.id, pause_container_id
                        ),
                        json!({
                            "missingRefKind": "container",
                            "missingRefId": pause_container_id,
                            "refField": "pauseContainerId",
                        }),
                    );
                }
            }
        }

        for image_ref in &self.image_refs {
            if !image_ids.contains(image_ref.image_id.as_str()) {
                push_issue(
                    &mut issues,
                    "danglingRef",
                    "imageRef",
                    &image_ref.reference,
                    false,
                    format!(
                        "image ref {} points to missing image {}",
                        image_ref.reference, image_ref.image_id
                    ),
                    json!({ "missingRefKind": "image", "missingRefId": image_ref.image_id }),
                );
            }
        }

        for snapshot in &self.snapshots {
            if snapshot.state == SnapshotLedgerState::Broken.as_str() {
                push_issue(
                    &mut issues,
                    "brokenSnapshot",
                    "snapshot",
                    &snapshot.key,
                    false,
                    format!("snapshot {} is marked broken", snapshot.key),
                    json!({ "state": snapshot.state }),
                );
            } else if snapshot.state == SnapshotLedgerState::Stale.as_str() {
                push_issue(
                    &mut issues,
                    "staleSnapshot",
                    "snapshot",
                    &snapshot.key,
                    false,
                    format!("snapshot {} is marked stale", snapshot.key),
                    json!({ "state": snapshot.state }),
                );
            }

            if !image_ids.contains(snapshot.image_id.as_str()) {
                push_issue(
                    &mut issues,
                    "danglingRef",
                    "snapshot",
                    &snapshot.key,
                    true,
                    format!(
                        "snapshot {} references missing image {}",
                        snapshot.key, snapshot.image_id
                    ),
                    json!({ "missingRefKind": "image", "missingRefId": snapshot.image_id }),
                );
                operations.push(LedgerRepairOperation::DeleteOrphanSnapshot {
                    key: snapshot.key.clone(),
                });
            }

            let owner_exists = match snapshot.owner_kind.as_str() {
                "container" => container_ids.contains(snapshot.owner_id.as_str()),
                "pod" => pod_ids.contains(snapshot.owner_id.as_str()),
                "image" => image_ids.contains(snapshot.owner_id.as_str()),
                _ => false,
            };
            if !owner_exists {
                push_issue(
                    &mut issues,
                    "ownerGraphBroken",
                    "snapshot",
                    &snapshot.key,
                    snapshot.state != SnapshotLedgerState::Broken.as_str(),
                    format!(
                        "snapshot {} references missing {} owner {}",
                        snapshot.key, snapshot.owner_kind, snapshot.owner_id
                    ),
                    json!({
                        "missingOwnerKind": snapshot.owner_kind,
                        "missingOwnerId": snapshot.owner_id,
                    }),
                );
                if snapshot.state != SnapshotLedgerState::Broken.as_str() {
                    operations.push(LedgerRepairOperation::DeleteOrphanSnapshot {
                        key: snapshot.key.clone(),
                    });
                }
            }
        }

        for artifact in &self.runtime_artifacts {
            let owner_exists = match artifact.owner_kind.as_str() {
                "container" => container_ids.contains(artifact.owner_id.as_str()),
                "pod" => pod_ids.contains(artifact.owner_id.as_str()),
                "image" => image_ids.contains(artifact.owner_id.as_str()),
                _ => false,
            };
            let can_mark_broken = artifact.state != RuntimeArtifactLedgerState::Broken.as_str()
                && artifact.state != RuntimeArtifactLedgerState::Deleted.as_str();

            if !owner_exists {
                push_issue(
                    &mut issues,
                    "ownerGraphBroken",
                    "runtimeArtifact",
                    &artifact.path,
                    can_mark_broken,
                    format!(
                        "runtime artifact {} references missing {} owner {}",
                        artifact.path, artifact.owner_kind, artifact.owner_id
                    ),
                    json!({
                        "missingOwnerKind": artifact.owner_kind,
                        "missingOwnerId": artifact.owner_id,
                        "artifactKind": artifact.artifact_kind,
                    }),
                );
                if can_mark_broken {
                    operations.push(LedgerRepairOperation::MarkRuntimeArtifactBroken {
                        owner_kind: artifact.owner_kind.clone(),
                        owner_id: artifact.owner_id.clone(),
                        artifact_kind: artifact.artifact_kind.clone(),
                        path: artifact.path.clone(),
                    });
                }
            }

            if artifact.state == RuntimeArtifactLedgerState::Broken.as_str() {
                push_issue(
                    &mut issues,
                    "brokenRuntimeArtifact",
                    "runtimeArtifact",
                    &artifact.path,
                    false,
                    format!("runtime artifact {} is marked broken", artifact.path),
                    json!({
                        "ownerKind": artifact.owner_kind,
                        "ownerId": artifact.owner_id,
                        "artifactKind": artifact.artifact_kind,
                        "state": artifact.state,
                    }),
                );
            } else if options.check_files
                && can_mark_broken
                && !artifact.path.is_empty()
                && !Path::new(&artifact.path).exists()
            {
                push_issue(
                    &mut issues,
                    "missingArtifact",
                    "runtimeArtifact",
                    &artifact.path,
                    true,
                    format!("runtime artifact path does not exist: {}", artifact.path),
                    json!({
                        "ownerKind": artifact.owner_kind,
                        "ownerId": artifact.owner_id,
                        "artifactKind": artifact.artifact_kind,
                        "state": artifact.state,
                    }),
                );
                operations.push(LedgerRepairOperation::MarkRuntimeArtifactBroken {
                    owner_kind: artifact.owner_kind.clone(),
                    owner_id: artifact.owner_id.clone(),
                    artifact_kind: artifact.artifact_kind.clone(),
                    path: artifact.path.clone(),
                });
            }
        }

        for shim in &self.shim_processes {
            if !container_ids.contains(shim.container_id.as_str()) {
                push_issue(
                    &mut issues,
                    "ownerGraphBroken",
                    "shim",
                    &shim.container_id,
                    true,
                    format!(
                        "shim process record references missing container {}",
                        shim.container_id
                    ),
                    json!({ "state": shim.state, "pid": shim.shim_pid }),
                );
                operations.push(LedgerRepairOperation::DeleteDanglingShim {
                    container_id: shim.container_id.clone(),
                });
                continue;
            }

            if shim.state == ShimLedgerState::Dead.as_str() {
                push_issue(
                    &mut issues,
                    "deadShim",
                    "shim",
                    &shim.container_id,
                    false,
                    format!("shim {} is marked dead", shim.container_id),
                    json!({ "state": shim.state, "pid": shim.shim_pid }),
                );
            } else if shim.state == ShimLedgerState::Broken.as_str() {
                push_issue(
                    &mut issues,
                    "brokenShim",
                    "shim",
                    &shim.container_id,
                    false,
                    format!("shim {} is marked broken", shim.container_id),
                    json!({ "state": shim.state, "pid": shim.shim_pid }),
                );
            } else if shim.state == ShimLedgerState::Degraded.as_str() {
                push_issue(
                    &mut issues,
                    "degradedShim",
                    "shim",
                    &shim.container_id,
                    true,
                    format!("shim {} is marked degraded", shim.container_id),
                    json!({ "state": shim.state, "pid": shim.shim_pid }),
                );
                operations.push(LedgerRepairOperation::MarkShimDead {
                    container_id: shim.container_id.clone(),
                });
            } else if options.check_files && shim_requires_liveness_probe(&shim.state) {
                let socket_exists =
                    shim.socket_path.is_empty() || Path::new(&shim.socket_path).exists();
                let process_exists = shim_process_exists(shim.shim_pid);
                if !socket_exists || !process_exists {
                    push_issue(
                        &mut issues,
                        "brokenShim",
                        "shim",
                        &shim.container_id,
                        true,
                        format!(
                            "shim {} is not reachable: socket_exists={}, process_exists={}",
                            shim.container_id, socket_exists, process_exists
                        ),
                        json!({
                            "state": shim.state,
                            "pid": shim.shim_pid,
                            "socketPath": shim.socket_path,
                            "socketExists": socket_exists,
                            "processExists": process_exists,
                        }),
                    );
                    operations.push(LedgerRepairOperation::MarkShimDead {
                        container_id: shim.container_id.clone(),
                    });
                }
            }
        }

        let mut seen_operations = HashSet::new();
        operations.retain(|operation| seen_operations.insert(operation.clone()));

        let repairable_issue_count = issues.iter().filter(|issue| issue.repairable).count();
        let actions = operations
            .iter()
            .map(|operation| operation.dry_run_action())
            .collect::<Vec<_>>();

        LedgerCheckPlan {
            report: LedgerCheckReport {
                dry_run,
                checked_at_unix_millis: chrono::Utc::now().timestamp_millis(),
                issue_count: issues.len(),
                repairable_issue_count,
                action_count: actions.len(),
                applied_action_count: 0,
                issues,
                actions,
            },
            operations,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotLedgerState {
    Preparing,
    Prepared,
    Mounted,
    Stale,
    Committed,
    Removing,
    Deleted,
    Broken,
}

impl SnapshotLedgerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Prepared => "prepared",
            Self::Mounted => "mounted",
            Self::Stale => "stale",
            Self::Committed => "committed",
            Self::Removing => "removing",
            Self::Deleted => "deleted",
            Self::Broken => "broken",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "preparing" => Ok(Self::Preparing),
            "prepared" => Ok(Self::Prepared),
            "mounted" => Ok(Self::Mounted),
            "stale" => Ok(Self::Stale),
            "committed" => Ok(Self::Committed),
            "removing" => Ok(Self::Removing),
            "deleted" => Ok(Self::Deleted),
            "broken" => Ok(Self::Broken),
            other => anyhow::bail!("invalid snapshot ledger state: {other}"),
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (
                    Self::Preparing,
                    Self::Prepared | Self::Removing | Self::Broken
                ) | (
                    Self::Prepared,
                    Self::Mounted | Self::Committed | Self::Removing | Self::Broken
                ) | (
                    Self::Mounted,
                    Self::Stale | Self::Committed | Self::Removing | Self::Broken
                ) | (Self::Stale, Self::Mounted | Self::Removing | Self::Broken)
                    | (Self::Committed, Self::Removing | Self::Broken)
                    | (Self::Removing, Self::Deleted | Self::Broken)
                    | (Self::Broken, Self::Removing | Self::Deleted)
            )
    }
}

impl fmt::Display for SnapshotLedgerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeArtifactLedgerState {
    Planned,
    Created,
    Active,
    Stale,
    Deleted,
    Broken,
}

impl RuntimeArtifactLedgerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Created => "created",
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Deleted => "deleted",
            Self::Broken => "broken",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "planned" => Ok(Self::Planned),
            "created" => Ok(Self::Created),
            "active" => Ok(Self::Active),
            "stale" => Ok(Self::Stale),
            "deleted" => Ok(Self::Deleted),
            "broken" => Ok(Self::Broken),
            other => anyhow::bail!("invalid runtime artifact ledger state: {other}"),
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Planned, Self::Created | Self::Deleted | Self::Broken)
                    | (
                        Self::Created,
                        Self::Active | Self::Stale | Self::Deleted | Self::Broken
                    )
                    | (Self::Active, Self::Stale | Self::Deleted | Self::Broken)
                    | (Self::Stale, Self::Active | Self::Deleted | Self::Broken)
                    | (Self::Broken, Self::Stale | Self::Deleted)
            )
    }
}

impl fmt::Display for RuntimeArtifactLedgerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimLedgerState {
    Starting,
    Running,
    Exited,
    Dead,
    Broken,
    Degraded,
}

impl ShimLedgerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Dead => "dead",
            Self::Broken => "broken",
            Self::Degraded => "degraded",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "planned" | "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "exited" => Ok(Self::Exited),
            "dead" => Ok(Self::Dead),
            "broken" => Ok(Self::Broken),
            "degraded" => Ok(Self::Degraded),
            other => anyhow::bail!("invalid shim ledger state: {other}"),
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (
                    Self::Starting,
                    Self::Running | Self::Exited | Self::Dead | Self::Broken
                ) | (
                    Self::Running,
                    Self::Exited | Self::Dead | Self::Broken | Self::Degraded
                ) | (Self::Exited, Self::Dead | Self::Broken)
                    | (Self::Dead, Self::Starting | Self::Broken)
                    | (Self::Broken, Self::Starting | Self::Dead)
                    | (Self::Degraded, Self::Running | Self::Dead | Self::Broken)
            )
    }
}

impl fmt::Display for ShimLedgerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerRepairOptions {
    pub check_files: bool,
}

impl Default for LedgerRepairOptions {
    fn default() -> Self {
        Self { check_files: true }
    }
}

fn push_issue(
    issues: &mut Vec<LedgerCheckIssue>,
    kind: &str,
    subject_kind: &str,
    subject_id: &str,
    repairable: bool,
    message: String,
    details: serde_json::Value,
) {
    issues.push(LedgerCheckIssue {
        kind: kind.to_string(),
        subject_kind: subject_kind.to_string(),
        subject_id: subject_id.to_string(),
        severity: "warning".to_string(),
        repairable,
        message,
        details,
    });
}

fn shim_requires_liveness_probe(state: &str) -> bool {
    matches!(state, "starting" | "running" | "degraded")
}

fn shim_process_exists(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        Path::new("/proc").join(pid.to_string()).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}