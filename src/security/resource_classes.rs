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
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::oci::spec::{Linux, LinuxBlockIo, LinuxIntelRdt, LinuxResources, Spec};
use crate::defaults::{
    BLOCKIO_CONFIG_ENV,
    RESCTRL_PATH,
    POD_QOS_RDT_CLASS,
    BLOCKIO_CONTAINER_ANNOTATION,
    BLOCKIO_POD_ANNOTATION,
    BLOCKIO_POD_CONTAINER_PREFIX,
    RDT_CONTAINER_ANNOTATION,
    RDT_POD_ANNOTATION,
    RDT_POD_CONTAINER_PREFIX,
};

#[derive(Debug, Error)]
pub enum ResourceClassError {
    #[error("blockio class '{0}' requested but no blockio config path is configured")]
    MissingBlockIoConfig(String),
    #[error("failed to read blockio config {path}: {source}")]
    ReadBlockIoConfig {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse blockio config {path}: {source}")]
    ParseBlockIoConfig {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("blockio class '{class_name}' not found in {path}")]
    BlockIoClassNotFound { class_name: String, path: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceClassSupport {
    pub blockio_supported: bool,
    pub blockio_config_path: Option<PathBuf>,
    pub rdt_supported: bool,
    pub rdt_resctrl_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceClassRequest {
    pub blockio_class: Option<String>,
    pub rdt_class: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct BlockIoClassConfig {
    classes: HashMap<String, LinuxBlockIo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum BlockIoConfigFile {
    Wrapped(BlockIoClassConfig),
    Flat(HashMap<String, LinuxBlockIo>),
}

pub fn effective_blockio_config_path(config_path: Option<&str>) -> Option<PathBuf> {
    config_path
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var(BLOCKIO_CONFIG_ENV)
                .ok()
                .filter(|path| !path.trim().is_empty())
                .map(PathBuf::from)
        })
}

pub fn feature_support(config_path: Option<&str>) -> ResourceClassSupport {
    let blockio_config_path = effective_blockio_config_path(config_path);
    ResourceClassSupport {
        blockio_supported: blockio_config_path.is_some(),
        blockio_config_path,
        rdt_supported: Path::new(RESCTRL_PATH).exists(),
        rdt_resctrl_path: PathBuf::from(RESCTRL_PATH),
    }
}

pub fn resolve_blockio_class(
    class_name: &str,
    config_path: Option<&str>,
) -> Result<Option<LinuxBlockIo>, ResourceClassError> {
    if class_name.trim().is_empty() {
        return Ok(None);
    }

    let path = effective_blockio_config_path(config_path)
        .ok_or_else(|| ResourceClassError::MissingBlockIoConfig(class_name.to_string()))?;
    let content =
        std::fs::read_to_string(&path).map_err(|source| ResourceClassError::ReadBlockIoConfig {
            path: path.clone(),
            source,
        })?;
    let classes = match serde_json::from_str::<BlockIoConfigFile>(&content).map_err(|source| {
        ResourceClassError::ParseBlockIoConfig {
            path: path.clone(),
            source,
        }
    })? {
        BlockIoConfigFile::Wrapped(config) => config.classes,
        BlockIoConfigFile::Flat(classes) => classes,
    };
    classes
        .get(class_name)
        .cloned()
        .ok_or_else(|| ResourceClassError::BlockIoClassNotFound {
            class_name: class_name.to_string(),
            path,
        })
        .map(Some)
}

pub fn resolve_rdt_class(class_name: &str) -> Option<LinuxIntelRdt> {
    let class_name = class_name.trim();
    if class_name.is_empty() || class_name == POD_QOS_RDT_CLASS {
        return None;
    }

    Some(LinuxIntelRdt {
        clos_id: Some(class_name.to_string()),
        l3_cache_schema: None,
        mem_bw_schema: None,
        enable_cmt: None,
        enable_mbm: None,
    })
}

fn class_from_annotations(
    container_name: &str,
    container_annotations: &HashMap<String, String>,
    pod_annotations: &HashMap<String, String>,
    container_key: &str,
    pod_key: &str,
    pod_container_prefix: &str,
) -> Option<String> {
    container_annotations
        .get(container_key)
        .or_else(|| pod_annotations.get(&format!("{pod_container_prefix}{container_name}")))
        .or_else(|| pod_annotations.get(pod_key))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn requested_classes_from_annotations(
    container_name: &str,
    container_annotations: &HashMap<String, String>,
    pod_annotations: &HashMap<String, String>,
) -> ResourceClassRequest {
    ResourceClassRequest {
        blockio_class: class_from_annotations(
            container_name,
            container_annotations,
            pod_annotations,
            BLOCKIO_CONTAINER_ANNOTATION,
            BLOCKIO_POD_ANNOTATION,
            BLOCKIO_POD_CONTAINER_PREFIX,
        ),
        rdt_class: class_from_annotations(
            container_name,
            container_annotations,
            pod_annotations,
            RDT_CONTAINER_ANNOTATION,
            RDT_POD_ANNOTATION,
            RDT_POD_CONTAINER_PREFIX,
        ),
    }
}

fn ensure_linux(spec: &mut Spec) -> &mut Linux {
    spec.linux.get_or_insert(Linux {
        namespaces: None,
        uid_mappings: None,
        gid_mappings: None,
        devices: None,
        net_devices: None,
        cgroups_path: None,
        resources: None,
        rootfs_propagation: None,
        seccomp: None,
        sysctl: None,
        mount_label: None,
        masked_paths: None,
        readonly_paths: None,
        intel_rdt: None,
    })
}

fn ensure_linux_resources(spec: &mut Spec) -> &mut LinuxResources {
    ensure_linux(spec).resources.get_or_insert(LinuxResources {
        network: None,
        pids: None,
        memory: None,
        cpu: None,
        block_io: None,
        hugepage_limits: None,
        devices: None,
        intel_rdt: None,
        unified: None,
    })
}

pub fn apply_resource_class_request(
    spec: &mut Spec,
    request: &ResourceClassRequest,
    blockio_config_path: Option<&str>,
) -> Result<(), ResourceClassError> {
    if let Some(blockio_class) = request.blockio_class.as_ref() {
        if let Some(block_io) = resolve_blockio_class(blockio_class, blockio_config_path)? {
            ensure_linux_resources(spec).block_io = Some(block_io);
        }
    }

    if let Some(rdt_class) = request.rdt_class.as_ref() {
        ensure_linux(spec).intel_rdt = resolve_rdt_class(rdt_class);
    }

    Ok(())
}