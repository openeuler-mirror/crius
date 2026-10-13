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

use crate::proto::runtime::v1::{Container, ContainerMetadata, ContainerState, ImageSpec};
use crate::server::service::RuntimeServiceImpl;

impl RuntimeServiceImpl {
    /// 启动恢复：把 db 中的容器记录加载回内存 map，使 ps/exec 等操作在 daemon 重启后可用。
    /// 已在内存中的记录（本轮启动周期内创建的）不会被覆盖。
    pub async fn recover_containers_from_ledger(&self) {
        let entries = {
            let persistence = self.persistence.lock().await;
            persistence.recover_containers()
        };
        let entries = match entries {
            Ok(entries) => entries,
            Err(err) => {
                log::warn!(
                    "container recovery: failed to list containers from ledger: {}",
                    err
                );
                return;
            }
        };

        let mut recovered = 0usize;
        {
            let mut containers = self.containers.lock().await;
            for (id, status, record) in entries {
                if containers.contains_key(&id) {
                    continue;
                }
                let labels: HashMap<String, String> =
                    serde_json::from_str(&record.labels).unwrap_or_default();
                let annotations: HashMap<String, String> =
                    serde_json::from_str(&record.annotations).unwrap_or_default();
                // 从内部状态注解恢复容器名称/尝试次数（随 annotations 持久化落库）
                let metadata = annotations
                    .get(crate::defaults::INTERNAL_CONTAINER_STATE_KEY)
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .and_then(|state| {
                        let name = state
                            .get("metadata_name")
                            .and_then(|v| v.as_str())?
                            .to_string();
                        let attempt = state
                            .get("metadata_attempt")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(1) as u32;
                        Some(ContainerMetadata { name, attempt })
                    })
                    .unwrap_or_else(|| ContainerMetadata {
                        name: id.clone(),
                        attempt: 1,
                    });
                let container = Container {
                    id: id.clone(),
                    pod_sandbox_id: record.pod_id.clone().unwrap_or_default(),
                    state: runtime_status_to_proto_state(&status),
                    // db 持久化的是秒级时间戳，proto 约定纳秒
                    created_at: record.created_at.saturating_mul(1_000_000_000),
                    metadata: Some(metadata),
                    image: Some(ImageSpec {
                        image: record.image.clone(),
                        user_specified_image: record.image.clone(),
                        ..Default::default()
                    }),
                    image_ref: record.image.clone(),
                    labels,
                    annotations,
                };
                containers.insert(id, container);
                recovered += 1;
            }
        }
        log::info!(
            "container recovery: loaded {} containers from ledger",
            recovered
        );
    }
}

fn runtime_status_to_proto_state(status: &crate::runtime::ContainerStatus) -> i32 {
    match status {
        crate::runtime::ContainerStatus::Created => ContainerState::ContainerCreated as i32,
        crate::runtime::ContainerStatus::Running => ContainerState::ContainerRunning as i32,
        crate::runtime::ContainerStatus::Stopped(_) => ContainerState::ContainerExited as i32,
        crate::runtime::ContainerStatus::Unknown => ContainerState::ContainerUnknown as i32,
    }
}
