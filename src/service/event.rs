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


use crate::proto::runtime::v1::ContainerEventResponse;
use crate::defaults::{MAX_INTERNAL_EVENT_DETAIL_BYTES, INTERNAL_EVENT_PREFIXES, INTERNAL_EVENT_SUBJECT_KINDS};

#[derive(Debug, Clone)]
pub struct EventService {
    sender: tokio::sync::broadcast::Sender<ContainerEventResponse>,
    internal_sender: tokio::sync::broadcast::Sender<InternalEvent>,
    ledger:
        Option<std::sync::Arc<tokio::sync::Mutex<crate::storage::persistence::PersistenceManager>>>,
    internal_retention_per_subject: usize,
}

#[derive(Debug, Clone)]
pub struct InternalEvent {
    pub kind: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub severity: InternalEventSeverity,
    pub timestamp: i64,
    pub details: serde_json::Value,
}

impl InternalEvent {
    pub fn new(
        kind: impl Into<String>,
        subject_kind: impl Into<String>,
        subject_id: impl Into<String>,
        severity: InternalEventSeverity,
        details: serde_json::Value,
    ) -> Self {
        Self::with_timestamp(
            kind,
            subject_kind,
            subject_id,
            severity,
            chrono::Utc::now().timestamp(),
            details,
        )
    }

    pub fn with_timestamp(
        kind: impl Into<String>,
        subject_kind: impl Into<String>,
        subject_id: impl Into<String>,
        severity: InternalEventSeverity,
        timestamp: i64,
        details: serde_json::Value,
    ) -> Self {
        Self {
            kind: kind.into(),
            subject_kind: subject_kind.into(),
            subject_id: subject_id.into(),
            severity,
            timestamp,
            details: sanitize_details(details),
        }
    }

    pub fn validate_schema(&self) -> anyhow::Result<()> {
        validate_internal_event_kind(&self.kind)?;
        validate_internal_event_subject_kind(&self.subject_kind)?;
        if self.subject_id.trim().is_empty() {
            anyhow::bail!("internal event subject_id must not be empty");
        }
        if self.details.to_string().len() > MAX_INTERNAL_EVENT_DETAIL_BYTES {
            anyhow::bail!(
                "internal event details exceeded {} bytes after sanitization",
                MAX_INTERNAL_EVENT_DETAIL_BYTES
            );
        }
        Ok(())
    }

    fn details_for_ledger(&self) -> Option<String> {
        (!self.details.is_null()).then(|| self.details.to_string())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InternalEventSeverity {
    Debug,
    Info,
    Warning,
    Error,
}

impl InternalEventSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LedgerInternalEventSink {
    db_path: std::path::PathBuf,
}

impl LedgerInternalEventSink {
    pub fn new(db_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    pub fn publish(&self, event: &InternalEvent) -> anyhow::Result<()> {
        event.validate_schema()?;
        let mut storage = crate::storage::StorageManager::new(&self.db_path)?;
        storage.append_typed_event_at(crate::storage::TypedEventInput {
            event_type: &event.kind,
            entity_type: &event.subject_kind,
            entity_id: &event.subject_id,
            old_state: None,
            new_state: Some(event.severity.as_str()),
            details: event.details_for_ledger().as_deref(),
            timestamp: event.timestamp,
        })
    }
}

fn sanitize_details(details: serde_json::Value) -> serde_json::Value {
    if details.to_string().len() <= MAX_INTERNAL_EVENT_DETAIL_BYTES {
        return details;
    }

    serde_json::json!({
        "truncated": true,
        "reason": "details too large",
        "maxBytes": MAX_INTERNAL_EVENT_DETAIL_BYTES,
    })
}

fn validate_internal_event_kind(kind: &str) -> anyhow::Result<()> {
    let kind = kind.trim();
    if kind.is_empty() {
        anyhow::bail!("internal event kind must not be empty");
    }
    if !INTERNAL_EVENT_PREFIXES
        .iter()
        .any(|prefix| kind.starts_with(prefix))
    {
        anyhow::bail!("unsupported internal event kind: {kind}");
    }
    Ok(())
}

fn validate_internal_event_subject_kind(subject_kind: &str) -> anyhow::Result<()> {
    let subject_kind = subject_kind.trim();
    if subject_kind.is_empty() {
        anyhow::bail!("internal event subject_kind must not be empty");
    }
    if !INTERNAL_EVENT_SUBJECT_KINDS.contains(&subject_kind) {
        anyhow::bail!("unsupported internal event subject_kind: {subject_kind}");
    }
    Ok(())
}