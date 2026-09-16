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

use serde::Deserialize;

use crate::server::service::RuntimeServiceImpl;
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
};

#[derive(Clone, Copy)]
pub(super) enum AnnotationScope {
    Pod,
    Container,
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
}