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
}