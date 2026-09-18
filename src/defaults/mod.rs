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
pub const MIN_CONTAINER_CREATE_TIMEOUT_SECS: u32 = 30;

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

pub(crate) const CRS_RUN_ANNOTATION: &str = "io.crius.internal/crs-run";
pub(crate) const CRS_RUN_ANNOTATION_VALUE: &str = "true";

pub const RANDOM_NAME_LEFT: &[&str] = &[
    "admiring",
    "adoring",
    "amazing",
    "bold",
    "brave",
    "clever",
    "cool",
    "eager",
    "elastic",
    "epic",
    "focused",
    "friendly",
    "gifted",
    "happy",
    "hopeful",
    "jolly",
    "kind",
    "lucid",
    "nifty",
    "peaceful",
    "practical",
    "sharp",
    "stoic",
    "trusting",
    "vigilant",
    "wizardly",
];

pub const RANDOM_NAME_RIGHT: &[&str] = &[
    "archimedes",
    "babbage",
    "bell",
    "bohr",
    "curie",
    "darwin",
    "einstein",
    "faraday",
    "fermi",
    "franklin",
    "galileo",
    "hopper",
    "hypatia",
    "lovelace",
    "maxwell",
    "mccarthy",
    "morse",
    "newton",
    "noether",
    "pasteur",
    "ritchie",
    "tesla",
    "torvalds",
    "turing",
    "volhard",
    "yonath",
];

pub const INTERNAL_POD_STATE_KEY: &str = "io.crius.internal/pod-state";
pub const INTERNAL_ANNOTATION_PREFIX: &str = "io.crius.internal/";

pub const CRIO_SANDBOX_ID_ANNOTATION: &str = "io.kubernetes.cri-o.SandboxID";
pub const CRIO_SANDBOX_NAME_ANNOTATION: &str = "io.kubernetes.cri-o.SandboxName";
pub const CRIO_POD_NAME_ANNOTATION: &str = "io.kubernetes.cri-o.Name";
pub const CRIO_POD_NAMESPACE_ANNOTATION: &str = "io.kubernetes.cri-o.Namespace";
pub const CRIO_SECCOMP_NOTIFIER_ACTION_ANNOTATION: &str = "io.kubernetes.cri-o.seccompNotifierAction";
pub const CRIO_RUNTIME_HANDLER_ANNOTATION: &str = "io.kubernetes.cri-o.RuntimeHandler";

pub const CRIO_CONTAINER_ID_ANNOTATION: &str = "io.kubernetes.cri-o.ContainerID";
pub const CRIO_CONTAINER_NAME_ANNOTATION: &str = "io.kubernetes.cri-o.ContainerName";
pub const CRIO_CONTAINER_TYPE_ANNOTATION: &str = "io.kubernetes.cri-o.ContainerType";

pub const CRIO_USER_REQUESTED_IMAGE_ANNOTATION: &str = "io.kubernetes.cri-o.Image";
pub const CRIO_IMAGE_NAME_ANNOTATION: &str = "io.kubernetes.cri-o.ImageName";
pub const CRIO_LOG_PATH_ANNOTATION: &str = "io.kubernetes.cri-o.LogPath";

pub const CONTAINERD_SANDBOX_ID_ANNOTATION: &str = "io.kubernetes.cri.sandbox-id";
pub const CONTAINERD_SANDBOX_NAME_ANNOTATION: &str = "io.kubernetes.cri.sandbox-name";
pub const CONTAINERD_SANDBOX_NAMESPACE_ANNOTATION: &str = "io.kubernetes.cri.sandbox-namespace";
pub const CONTAINERD_SANDBOX_UID_ANNOTATION: &str = "io.kubernetes.cri.sandbox-uid";
pub const CONTAINERD_RUNTIME_HANDLER_ANNOTATION: &str = "io.containerd.cri.runtime-handler";
pub const CONTAINERD_CONTAINER_TYPE_ANNOTATION: &str = "io.kubernetes.cri.container-type";
pub const CONTAINERD_IMAGE_NAME_ANNOTATION: &str = "io.kubernetes.cri.image-name";
pub const CONTAINERD_CONTAINER_NAME_ANNOTATION: &str = "io.kubernetes.cri.container-name";

pub const KUBERNETES_CONTAINER_NAME_ANNOTATION: &str = "io.kubernetes.container.name";

pub const DEFAULT_CONTAINER_CREATE_TIMEOUT_SECS: u32 = 240;

pub const DEFAULT_CNI_TEARDOWN_TIMEOUT: Duration = Duration::from_secs(60);

pub const DEFAULT_SHIM_WORK_DIR: &str = "/var/run/crius/shims";

pub const SHIM_METADATA_FILE: &str = "shim.json";
pub const SHIM_PIDFILE_NAME: &str = "shim.pid";
pub const CHECKPOINT_LOCATION_ANNOTATION_KEY: &str = "io.crius.checkpoint.location";
pub const CONTAINER_TYPE_CONTAINER: &str = "container";