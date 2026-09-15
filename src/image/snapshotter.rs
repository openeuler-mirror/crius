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
// Minimal snapshotter types for crius-shim.
//
// Only the data types that `shim/daemon.rs` and `shim_rpc/proto.rs` actually
// read/write are defined here. The full github snapshotter (probe/prepare/
// mount/commit/remove, ~740 lines) is intentionally NOT ported: it pulls in
// `config::ExternalSnapshotterConfig`, `storage::SnapshotRecord` and a set of
// `StorageManager` snapshot methods that the minimal shim path does not need.
// `daemon.rs` implements rootfs mounting itself (`mount_rootfs_spec`,
// `apply_rootfs_handle_mounts`) by reading these struct fields directly.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootfsHandleKind {
    InternalPath,
    ExternalMountSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootfsOwner {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootfsMountSpec {
    #[serde(rename = "type")]
    pub mount_type: String,
    pub source: PathBuf,
    pub target: PathBuf,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootfsHandle {
    pub kind: RootfsHandleKind,
    pub snapshot_key: Option<String>,
    pub owner: RootfsOwner,
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub mounts: Vec<RootfsMountSpec>,
    #[serde(default)]
    pub readonly: bool,
}
