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

use serde::{Serialize, Deserialize};

use crate::proto::runtime::v1::NamespaceOption;
use crate::server::service::RuntimeServiceImpl;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(super) struct StoredNamespaceOptions {
    pub(super) network: i32,
    pub(super) pid: i32,
    pub(super) ipc: i32,
    pub(super) target_id: String,
    pub(super) userns_options: Option<StoredUserNamespace>,
}

impl StoredNamespaceOptions {
    pub(super) fn to_proto(&self) -> NamespaceOption {
        NamespaceOption {
            network: self.network,
            pid: self.pid,
            ipc: self.ipc,
            target_id: self.target_id.clone(),
            userns_options: self
                .userns_options
                .as_ref()
                .map(StoredUserNamespace::to_proto),
        }
    }
}

impl From<&NamespaceOption> for StoredNamespaceOptions {
    fn from(value: &NamespaceOption) -> Self {
        Self {
            network: value.network,
            pid: value.pid,
            ipc: value.ipc,
            target_id: value.target_id.clone(),
            userns_options: value.userns_options.as_ref().map(StoredUserNamespace::from),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
#[serde(default)]
pub(super) struct StoredPodState {
    pub(super) port_mappings: Vec<StoredPortMapping>,
    pub(super) raw_cni_result: Option<serde_json::Value>,
    pub(super) hostname: Option<String>,
    pub(super) log_directory: Option<String>,
    pub(super) runtime_handler: String,
    pub(super) runtime_pod_cidr: Option<String>,
    pub(super) netns_path: Option<String>,
    pub(super) pause_container_id: Option<String>,
    pub(super) ip: Option<String>,
    pub(super) additional_ips: Vec<String>,
    pub(super) cgroup_parent: Option<String>,
    pub(super) sysctls: HashMap<String, String>,
    pub(super) namespace_options: Option<StoredNamespaceOptions>,
    pub(super) privileged: bool,
    pub(super) run_as_user: Option<String>,
    pub(super) run_as_group: Option<u32>,
    pub(super) supplemental_groups: Vec<u32>,
    pub(super) readonly_rootfs: bool,
    pub(super) no_new_privileges: Option<bool>,
    pub(super) apparmor_profile: Option<String>,
    pub(super) selinux_label: Option<String>,
    pub(super) seccomp_profile: Option<StoredSecurityProfile>,
    pub(super) overhead_linux_resources: Option<StoredLinuxResources>,
    pub(super) linux_resources: Option<StoredLinuxResources>,
    pub(super) stop_notified: bool,
    pub(super) broken: Option<StoredBrokenState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
#[serde(default)]
pub(super) struct StoredPortMapping {
    pub(super) protocol: String,
    pub(super) container_port: i32,
    pub(super) host_port: i32,
    pub(super) host_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
#[serde(default)]
pub(super) struct StoredUserNamespace {
    pub(super) mode: i32,
    pub(super) uids: Vec<StoredIdMapping>,
    pub(super) gids: Vec<StoredIdMapping>,
}

impl StoredUserNamespace {
    pub(super) fn to_proto(&self) -> crate::proto::runtime::v1::UserNamespace {
        crate::proto::runtime::v1::UserNamespace {
            mode: self.mode,
            uids: self.uids.iter().map(StoredIdMapping::to_proto).collect(),
            gids: self.gids.iter().map(StoredIdMapping::to_proto).collect(),
        }
    }
}

impl From<&crate::proto::runtime::v1::UserNamespace> for StoredUserNamespace {
    fn from(value: &crate::proto::runtime::v1::UserNamespace) -> Self {
        Self {
            mode: value.mode,
            uids: value.uids.iter().map(StoredIdMapping::from).collect(),
            gids: value.gids.iter().map(StoredIdMapping::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
#[serde(default)]
pub(super) struct StoredIdMapping {
    pub(super) host_id: u32,
    pub(super) container_id: u32,
    pub(super) length: u32,
}

impl StoredIdMapping {
    pub(super) fn to_proto(&self) -> crate::proto::runtime::v1::IdMapping {
        crate::proto::runtime::v1::IdMapping {
            host_id: self.host_id,
            container_id: self.container_id,
            length: self.length,
        }
    }
}

impl From<&crate::proto::runtime::v1::IdMapping> for StoredIdMapping {
    fn from(value: &crate::proto::runtime::v1::IdMapping) -> Self {
        Self {
            host_id: value.host_id,
            container_id: value.container_id,
            length: value.length,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
#[serde(default)]
pub(super) struct StoredSecurityProfile {
    pub(super) profile_type: i32,
    pub(super) localhost_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
#[serde(default)]
pub(super) struct StoredLinuxResources {
    pub(super) cpu_period: i64,
    pub(super) cpu_quota: i64,
    pub(super) cpu_shares: i64,
    pub(super) memory_limit_in_bytes: i64,
    pub(super) oom_score_adj: i64,
    pub(super) cpuset_cpus: String,
    pub(super) cpuset_mems: String,
    pub(super) hugepage_limits: Vec<StoredHugepageLimit>,
    pub(super) unified: HashMap<String, String>,
    pub(super) memory_swap_limit_in_bytes: i64,
    pub(super) memory_reservation_in_bytes: Option<i64>,
    pub(super) memory_kernel_limit_in_bytes: Option<i64>,
    pub(super) memory_kernel_tcp_limit_in_bytes: Option<i64>,
    pub(super) memory_swappiness: Option<u64>,
    pub(super) memory_disable_oom_killer: Option<bool>,
    pub(super) memory_use_hierarchy: Option<bool>,
    pub(super) cpu_realtime_runtime: Option<i64>,
    pub(super) cpu_realtime_period: Option<u64>,
    pub(super) pids_limit: Option<i64>,
    pub(super) devices: Vec<StoredLinuxDeviceCgroup>,
    pub(super) blockio_class: Option<String>,
    pub(super) rdt_class: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
#[serde(default)]
pub(super) struct StoredHugepageLimit {
    pub(super) page_size: String,
    pub(super) limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
#[serde(default)]
pub(super) struct StoredLinuxDeviceCgroup {
    pub(super) allow: bool,
    pub(super) device_type: Option<String>,
    pub(super) major: Option<i64>,
    pub(super) minor: Option<i64>,
    pub(super) access: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)] 
pub(super) struct StoredBrokenState {
    pub(super) kind: String,
    pub(super) details: String,
    pub(super) detected_at: i64,
}