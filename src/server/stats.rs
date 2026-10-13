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


use tonic::{Request, Response, Status};

use crate::server::service::RuntimeServiceImpl;
use crate::proto::runtime::v1::{
    ContainerStatsRequest, ContainerStatsResponse,
    ListContainerStatsRequest, ListContainerStatsResponse,
    ContainerStats, ContainerAttributes, CpuUsage, MemoryUsage, UInt64Value,
    Container, ContainerState, ContainerFilter,
};

impl RuntimeServiceImpl {

    pub(super) async fn container_stats(
        &self,
        request: Request<ContainerStatsRequest>,
    ) -> Result<Response<ContainerStatsResponse>, Status> {
        let req = request.into_inner();
        let container_id = self.resolve_container_id(&req.container_id).await?;

        let container = {
            let containers = self.containers.lock().await;
            containers.get(&container_id).cloned()
        }
        .ok_or_else(|| Status::not_found("Container not found"))?;

        let stats = self
            .build_stats_for_container(&container_id, &container)
            .await;

        Ok(Response::new(ContainerStatsResponse { stats: Some(stats) }))
    }

    /// 构建单个容器的 stats 快照；cgroup 用量读取失败时回退为零值。
    async fn build_stats_for_container(
        &self,
        container_id: &str,
        container: &Container,
    ) -> ContainerStats {
        let (cpu_nanos, memory_bytes, working_set) = self
            .container_cgroup_usage(container_id)
            .await
            .unwrap_or((0, 0, 0));
        let timestamp = now_unix_nanos();

        ContainerStats {
            attributes: Some(ContainerAttributes {
                id: container_id.to_string(),
                metadata: container.metadata.clone(),
                labels: container.labels.clone(),
                annotations: container.annotations.clone(),
            }),
            cpu: Some(CpuUsage {
                timestamp,
                usage_core_nano_seconds: Some(UInt64Value { value: cpu_nanos }),
                usage_nano_cores: None,
            }),
            memory: Some(MemoryUsage {
                timestamp,
                working_set_bytes: Some(UInt64Value { value: working_set }),
                available_bytes: None,
                usage_bytes: Some(UInt64Value { value: memory_bytes }),
                rss_bytes: None,
                page_faults: None,
                major_page_faults: None,
            }),
            writable_layer: None,
            swap: None,
        }
    }

    /// 读取容器 cgroup v2 用量：(cpu_nanos, memory_bytes, working_set_bytes)。
    /// 读取失败（容器已停止/无 PID/无 cgroup）返回 None，调用方回退为零值。
    async fn container_cgroup_usage(&self, container_id: &str) -> Option<(u64, u64, u64)> {
        let runtime = self
            .runtime_for_container_request(container_id)
            .await
            .ok()?;
        let container_id = container_id.to_string();
        let pid = match tokio::task::spawn_blocking(move || {
            runtime.task_controller().container_pid(&container_id)
        })
        .await
        {
            Ok(Ok(Some(pid))) => pid,
            _ => return None,
        };

        // /proc/<pid>/cgroup 在 cgroup v2 下为 "0::<相对路径>"
        let cgroup_content = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
        let relative = cgroup_content
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .map(str::trim)?
            .trim_start_matches('/');
        let cgroup_dir = std::path::Path::new("/sys/fs/cgroup").join(relative);

        // cpu.stat: "usage_usec <n>"
        let cpu_nanos = std::fs::read_to_string(cgroup_dir.join("cpu.stat"))
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|l| l.starts_with("usage_usec"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .map(|usec| usec * 1000)
            .unwrap_or(0);

        // memory.current: "<bytes>"
        let memory_bytes = std::fs::read_to_string(cgroup_dir.join("memory.current"))
            .ok()
            .and_then(|content| content.trim().parse::<u64>().ok())
            .unwrap_or(0);

        // memory.stat: "inactive_file <bytes>"；working set = current - inactive_file
        let inactive_file = std::fs::read_to_string(cgroup_dir.join("memory.stat"))
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|l| l.starts_with("inactive_file"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .unwrap_or(0);

        Some((
            cpu_nanos,
            memory_bytes,
            memory_bytes.saturating_sub(inactive_file),
        ))
    }

    pub(super) async fn list_container_stats(
        &self,
        request: Request<ListContainerStatsRequest>,
    ) -> Result<Response<ListContainerStatsResponse>, Status> {
        let req = request.into_inner();
        let filter = if let Some(mut filter) = req.filter {
            if !filter.id.is_empty() {
                let Some(resolved_id) =
                    self.resolve_container_id_for_filter(&filter.id).await
                else {
                    return Ok(Response::new(ListContainerStatsResponse {
                        stats: Vec::new(),
                    }));
                };
                filter.id = resolved_id;
            }
            // ContainerStatsFilter 与 ContainerFilter 字段兼容，转换后复用匹配逻辑
            Some(ContainerFilter {
                id: filter.id,
                pod_sandbox_id: filter.pod_sandbox_id,
                label_selector: filter.label_selector,
                ..Default::default()
            })
        } else {
            None
        };

        let containers: Vec<(String, Container)> = {
            let containers = self.containers.lock().await;
            containers
                .iter()
                .map(|(id, container)| (id.clone(), container.clone()))
                .collect()
        };

        let mut all_stats = Vec::new();
        for (container_id, container) in containers {
            // 跳过已退出/未知状态的容器
            if container.state == ContainerState::ContainerExited as i32
                || container.state == ContainerState::ContainerUnknown as i32
            {
                continue;
            }
            if let Some(f) = &filter {
                if !Self::container_matches_filter(&container, f) {
                    continue;
                }
            }
            all_stats.push(
                self.build_stats_for_container(&container_id, &container)
                    .await,
            );
        }

        Ok(Response::new(ListContainerStatsResponse { stats: all_stats }))
    }
}

fn now_unix_nanos() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}