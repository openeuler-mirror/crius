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


use std::unimplemented;

use tonic::Status;
use tokio_stream::wrappers::ReceiverStream;

use crate::proto::runtime::v1::ContainerEventResponse;
use crate::defaults::{
    MAX_INTERNAL_EVENT_DETAIL_BYTES, INTERNAL_EVENT_PREFIXES, 
    INTERNAL_EVENT_SUBJECT_KINDS, DEFAULT_INTERNAL_EVENT_RETENTION_PER_SUBJECT,
};

#[derive(Debug, Clone)]
pub struct EventService {
    sender: tokio::sync::broadcast::Sender<ContainerEventResponse>,
    internal_sender: tokio::sync::broadcast::Sender<InternalEvent>,
    ledger:
        Option<std::sync::Arc<tokio::sync::Mutex<crate::storage::persistence::PersistenceManager>>>,
    internal_retention_per_subject: usize,
}

impl EventService {
    pub fn stream(&self) -> ReceiverStream<Result<ContainerEventResponse, Status>> {
        let mut events = self.subscribe();
        let (tx, rx) = tokio::sync::mpsc::channel(128);

        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if tx.send(Ok(event)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if tx
                            .send(Err(Status::resource_exhausted(
                                "CRI event stream lagged behind producer",
                            )))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        ReceiverStream::new(rx)
    }

    pub fn from_sender(sender: tokio::sync::broadcast::Sender<ContainerEventResponse>) -> Self {
        let (internal_sender, _) = tokio::sync::broadcast::channel(256);
        Self {
            sender,
            internal_sender,
            ledger: None,
            internal_retention_per_subject: DEFAULT_INTERNAL_EVENT_RETENTION_PER_SUBJECT,
        }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ContainerEventResponse> {
        self.sender.subscribe()
    }

    pub fn with_ledger(
        mut self,
        ledger: std::sync::Arc<tokio::sync::Mutex<crate::storage::persistence::PersistenceManager>>,
    ) -> Self {
        self.ledger = Some(ledger);
        self
    }

    pub async fn publish_internal(&self, event: InternalEvent) -> anyhow::Result<()> {
        event.validate_schema()?;
        let persist_result = self.persist_internal_event(&event).await;
        if let Err(err) = self.internal_sender.send(event) {
            log::debug!("Dropping internal event without subscribers: {}", err);
        }
        persist_result
    }

    async fn persist_internal_event(&self, event: &InternalEvent) -> anyhow::Result<()> {
        let Some(ledger) = &self.ledger else {
            return Ok(());
        };
        event.validate_schema()?;
        let mut persistence = ledger.lock().await;
        let mut ledger = crate::state::StateLedgerWriter::new(&mut persistence);
        ledger.append_typed_event_at(crate::storage::TypedEventInput {
            event_type: &event.kind,
            entity_type: &event.subject_kind,
            entity_id: &event.subject_id,
            old_state: None,
            new_state: Some(event.severity.as_str()),
            details: event.details_for_ledger().as_deref(),
            timestamp: event.timestamp,
        })?;
        ledger.prune_events_for_subject(
            &event.subject_kind,
            &event.subject_id,
            self.internal_retention_per_subject,
        )?;
        Ok(())
    }

    pub fn publish(&self, event: ContainerEventResponse) {
        if let Err(err) = self.sender.send(event) {
            log::debug!("Dropping CRI event without subscribers: {}", err);
        }
    }
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