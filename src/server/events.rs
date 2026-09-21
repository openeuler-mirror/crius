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


use crate::proto::runtime::v1::{
    ContainerEventType, Container,
    PodSandboxStatus, ContainerEventResponse,
};
use crate::server::service::RuntimeServiceImpl;
use crate::service::event::{InternalEvent, InternalEventSeverity};

impl RuntimeServiceImpl {
    pub(super) async fn publish_container_lifecycle_event(
        &self,
        container_id: &str,
        action: &str,
        severity: InternalEventSeverity,
        details: serde_json::Value,
    ) {
        self.publish_lifecycle_internal_event("container", container_id, action, severity, details)
            .await;
    }

    async fn publish_lifecycle_internal_event(
        &self,
        subject_kind: &str,
        subject_id: &str,
        action: &str,
        severity: InternalEventSeverity,
        details: serde_json::Value,
    ) {
        let event = InternalEvent::new(
            format!("{subject_kind}.{action}"),
            subject_kind,
            subject_id,
            severity,
            details,
        );
        if let Err(err) = self.internal_services.events.publish_internal(event).await {
            log::debug!("Failed to publish {subject_kind} lifecycle event for {subject_id}: {err}");
        }
    }

    pub(super) fn publish_event(&self, event: ContainerEventResponse) {
        self.internal_services.events.publish(event);
    }

    pub(super) async fn current_pod_status_snapshot(
        &self,
        pod_id: &str,
    ) -> Option<PodSandboxStatus> {
        let pod = {
            let pod_sandboxes = self.pod_sandboxes.lock().await;
            pod_sandboxes.get(pod_id).cloned()
        }?;
        Some(self.build_pod_sandbox_status_snapshot(&pod))
    }

    pub(super) async fn emit_container_event(
        &self,
        event_type: ContainerEventType,
        container: &Container,
        runtime_state: Option<i32>,
    ) {
        if self.events.receiver_count() == 0 {
            return;
        }
        let pod_status = self
            .current_pod_status_snapshot(&container.pod_sandbox_id)
            .await;
        let snapshot = Self::build_container_status_snapshot(
            container,
            runtime_state.unwrap_or(container.state),
        );
        self.publish_event(ContainerEventResponse {
            container_id: container.id.clone(),
            container_event_type: event_type as i32,
            created_at: Self::now_nanos(),
            pod_sandbox_status: pod_status,
            containers_statuses: vec![snapshot],
        });
    }
}