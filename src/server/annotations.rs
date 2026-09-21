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
use std::path::Path;

use serde::{Deserialize, Serialize};
use tonic::Status;

use crate::server::service::RuntimeServiceImpl;
use crate::server::state_model::StoredPodState;
use crate::defaults::{
    INTERNAL_ANNOTATION_PREFIX, CRIO_SANDBOX_ID_ANNOTATION,
    CRIO_SANDBOX_NAME_ANNOTATION, CRIO_POD_NAME_ANNOTATION,
    CRIO_POD_NAMESPACE_ANNOTATION, CRIO_SECCOMP_NOTIFIER_ACTION_ANNOTATION,
    CRIO_RUNTIME_HANDLER_ANNOTATION, CONTAINERD_SANDBOX_ID_ANNOTATION,
    CONTAINERD_SANDBOX_NAME_ANNOTATION, CONTAINERD_SANDBOX_NAMESPACE_ANNOTATION,
    CONTAINERD_SANDBOX_UID_ANNOTATION, CONTAINERD_RUNTIME_HANDLER_ANNOTATION,
    CRIO_CONTAINER_ID_ANNOTATION, CRIO_CONTAINER_NAME_ANNOTATION,
    CRIO_CONTAINER_TYPE_ANNOTATION, CRIO_USER_REQUESTED_IMAGE_ANNOTATION,
    CRIO_IMAGE_NAME_ANNOTATION, CRIO_LOG_PATH_ANNOTATION,
    CONTAINERD_CONTAINER_TYPE_ANNOTATION, CONTAINERD_IMAGE_NAME_ANNOTATION,
    CONTAINERD_CONTAINER_NAME_ANNOTATION, KUBERNETES_CONTAINER_NAME_ANNOTATION,
    CONTAINER_TYPE_CONTAINER,
};

#[derive(Clone, Copy)]
pub(super) enum AnnotationScope {
    Pod,
    Container,
}

pub(super) struct ContainerAnnotationContext<'a> {
    pub(super) annotations: &'a mut HashMap<String, String>,
    pub(super) container_id: &'a str,
    pub(super) pod_sandbox_id: &'a str,
    pub(super) metadata_name: Option<&'a str>,
    pub(super) requested_image: Option<&'a str>,
    pub(super) resolved_image_name: Option<&'a str>,
    pub(super) log_path: Option<&'a Path>,
    pub(super) pod_state: Option<&'a StoredPodState>,
    pub(super) default_runtime: &'a str,
}

impl RuntimeServiceImpl {
    pub(super) fn read_internal_state<T: for<'de> Deserialize<'de>>(
        annotations: &HashMap<String, String>,
        key: &str,
    ) -> Option<T> {
        annotations
            .get(key)
            .and_then(|value| serde_json::from_str(value).ok())
    }

    pub(super) fn is_internal_annotation_key(key: &str) -> bool {
        key.starts_with(INTERNAL_ANNOTATION_PREFIX)
    }

    fn is_reserved_annotation_namespace(key: &str) -> bool {
        key.starts_with("io.kubernetes.cri-o.")
            || key.starts_with("io.kubernetes.cri.")
            || key.starts_with("io.containerd.cri.")
            || key.starts_with("io.kubernetes.container.")
    }

    fn scope_allows_reserved_annotation(scope: AnnotationScope, key: &str) -> bool {
        match scope {
            AnnotationScope::Pod => matches!(
                key,
                CRIO_SANDBOX_ID_ANNOTATION
                    | CRIO_SANDBOX_NAME_ANNOTATION
                    | CRIO_POD_NAME_ANNOTATION
                    | CRIO_POD_NAMESPACE_ANNOTATION
                    | CRIO_SECCOMP_NOTIFIER_ACTION_ANNOTATION
                    | CRIO_RUNTIME_HANDLER_ANNOTATION
                    | CONTAINERD_SANDBOX_ID_ANNOTATION
                    | CONTAINERD_SANDBOX_NAME_ANNOTATION
                    | CONTAINERD_SANDBOX_NAMESPACE_ANNOTATION
                    | CONTAINERD_SANDBOX_UID_ANNOTATION
                    | CONTAINERD_RUNTIME_HANDLER_ANNOTATION
            ),
            AnnotationScope::Container if key.starts_with("io.kubernetes.container.") => true,
            AnnotationScope::Container => matches!(
                key,
                CRIO_CONTAINER_ID_ANNOTATION
                    | CRIO_CONTAINER_NAME_ANNOTATION
                    | CRIO_CONTAINER_TYPE_ANNOTATION
                    | CRIO_USER_REQUESTED_IMAGE_ANNOTATION
                    | CRIO_IMAGE_NAME_ANNOTATION
                    | CRIO_LOG_PATH_ANNOTATION
                    | CRIO_SECCOMP_NOTIFIER_ACTION_ANNOTATION
                    | CRIO_RUNTIME_HANDLER_ANNOTATION
                    | CRIO_SANDBOX_ID_ANNOTATION
                    | CONTAINERD_CONTAINER_TYPE_ANNOTATION
                    | CONTAINERD_IMAGE_NAME_ANNOTATION
                    | CONTAINERD_SANDBOX_ID_ANNOTATION
                    | CONTAINERD_CONTAINER_NAME_ANNOTATION
                    | CONTAINERD_RUNTIME_HANDLER_ANNOTATION
                    | KUBERNETES_CONTAINER_NAME_ANNOTATION
            ),
        }
    }

    fn external_annotations_for_scope(
        annotations: &HashMap<String, String>,
        scope: AnnotationScope,
    ) -> HashMap<String, String> {
        annotations
            .iter()
            .filter(|(key, _)| !Self::is_internal_annotation_key(key))
            .filter(|(key, _)| {
                !Self::is_reserved_annotation_namespace(key)
                    || Self::scope_allows_reserved_annotation(scope, key)
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    pub(super) fn external_pod_annotations(
        annotations: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        Self::external_annotations_for_scope(annotations, AnnotationScope::Pod)
    }

    fn resolved_runtime_handler_name<'a>(&'a self, runtime_handler: &'a str) -> &'a str {
        let runtime_handler = runtime_handler.trim();
        if runtime_handler.is_empty() {
            self.config.runtime.as_str()
        } else {
            runtime_handler
        }
    }

    pub(super) fn apply_runtime_handler_default_annotations(
        &self,
        annotations: &mut HashMap<String, String>,
        runtime_handler: &str,
    ) {
        let Some(config) = self
            .config
            .runtime_configs
            .get(self.resolved_runtime_handler_name(runtime_handler))
        else {
            return;
        };

        for (key, value) in &config.default_annotations {
            annotations
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    pub(super) fn enrich_container_annotations(context: ContainerAnnotationContext<'_>) {
        context.annotations.insert(
            CRIO_CONTAINER_ID_ANNOTATION.to_string(),
            context.container_id.to_string(),
        );
        context.annotations.insert(
            CRIO_SANDBOX_ID_ANNOTATION.to_string(),
            context.pod_sandbox_id.to_string(),
        );
        context.annotations.insert(
            CRIO_CONTAINER_NAME_ANNOTATION.to_string(),
            context
                .metadata_name
                .unwrap_or(context.container_id)
                .to_string(),
        );
        context.annotations.insert(
            CRIO_CONTAINER_TYPE_ANNOTATION.to_string(),
            CONTAINER_TYPE_CONTAINER.to_string(),
        );
        context.annotations.insert(
            CONTAINERD_SANDBOX_ID_ANNOTATION.to_string(),
            context.pod_sandbox_id.to_string(),
        );
        context.annotations.insert(
            CONTAINERD_CONTAINER_TYPE_ANNOTATION.to_string(),
            CONTAINER_TYPE_CONTAINER.to_string(),
        );
        if let Some(name) = context.metadata_name {
            context.annotations.insert(
                CONTAINERD_CONTAINER_NAME_ANNOTATION.to_string(),
                name.to_string(),
            );
            context.annotations.insert(
                KUBERNETES_CONTAINER_NAME_ANNOTATION.to_string(),
                name.to_string(),
            );
        }
        if let Some(image) = context.requested_image.filter(|image| !image.is_empty()) {
            context.annotations.insert(
                CRIO_USER_REQUESTED_IMAGE_ANNOTATION.to_string(),
                image.to_string(),
            );
        }
        if let Some(image_name) = context
            .resolved_image_name
            .filter(|image| !image.is_empty())
        {
            context.annotations.insert(
                CRIO_IMAGE_NAME_ANNOTATION.to_string(),
                image_name.to_string(),
            );
            context.annotations.insert(
                CONTAINERD_IMAGE_NAME_ANNOTATION.to_string(),
                image_name.to_string(),
            );
        }
        let runtime_handler = context
            .pod_state
            .map(|state| state.runtime_handler.clone())
            .filter(|handler| !handler.is_empty())
            .unwrap_or_else(|| context.default_runtime.to_string());
        context.annotations.insert(
            CRIO_RUNTIME_HANDLER_ANNOTATION.to_string(),
            runtime_handler.clone(),
        );
        context.annotations.insert(
            CONTAINERD_RUNTIME_HANDLER_ANNOTATION.to_string(),
            runtime_handler,
        );
        if let Some(path) = context.log_path {
            context.annotations.insert(
                CRIO_LOG_PATH_ANNOTATION.to_string(),
                path.to_string_lossy().to_string(),
            );
        }
    }

    pub(super) fn insert_internal_state<T: Serialize>(
        annotations: &mut HashMap<String, String>,
        key: &str,
        state: &T,
    ) -> Result<(), Status> {
        let encoded = serde_json::to_string(state)
            .map_err(|e| Status::internal(format!("Failed to encode internal state: {}", e)))?;
        annotations.insert(key.to_string(), encoded);
        Ok(())
    }

    pub(super) fn external_container_annotations(
        annotations: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        Self::external_annotations_for_scope(annotations, AnnotationScope::Container)
    }
}