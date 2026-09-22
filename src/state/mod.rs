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

use crate::runtime::ContainerStatus;
use crate::storage::persistence::PersistenceManager;

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