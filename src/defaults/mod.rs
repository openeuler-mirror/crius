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


use std::time::Duration;

pub const DEFAULT_CRI_SOCKET_URI :&str = "unix:///run/crius/crius.sock";
pub const DEFAULT_CONTAINER_STORAGE_DIR: &str = "/var/lib/containers/storage";
pub const DEFAULT_STORAGE_DRIVER: &str = "overlay";
pub const DEFAULT_GRPC_MAX_MESSAGE_SIZE_BYTES: u32 = 80 * 1024 * 1024;

pub const DEFAULT_RUNTIME_STATE_DIR: &str = "/run/crius";
pub const DEFAULT_RUNTIME_SHIM_DIR: &str = "/run/crius/shims";
pub const DEFAULT_RUNTIME_ATTACH_SOCKET_DIR: &str = "/run/crius/attach";
pub const DEFAULT_RUNTIME_CONTAINER_EXITS_DIR: &str = "/run/crius/exits";
pub const DEFAULT_RUNTIME_CLEAN_SHUTDOWN_FILE: &str = "/var/lib/crius/clean.shutdown";
pub const DEFAULT_RUNTIME_VERSION_FILE: &str = "/run/crius/version";
pub const DEFAULT_RUNTIME_VERSION_FILE_PERSIST: &str = "/var/lib/crius/version";

pub const MIN_CONTAINER_STOP_TIMEOUT_SECS: u32 = 30;

pub const LOCAL_LOG_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.6f%:z";

pub const SERVER_SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(5);

pub const MAX_INTERNAL_EVENT_DETAIL_BYTES: usize = 16 * 1024;

pub const INTERNAL_EVENT_PREFIXES: &[&str] = &[
    "pod.",
    "container.",
    "image.",
    "network.",
    "gc.",
    "backend.",
    "task.",
    "shim.",
    "exec.",
    "attach.",
    "reconcile.",
    "orphan_cleanup.",
];

pub const INTERNAL_EVENT_SUBJECT_KINDS: &[&str] = &[
    "pod",
    "container",
    "image",
    "network",
    "gc",
    "backend",
    "task",
    "shim",
    "reconcile",
    "orphan_cleanup",
];