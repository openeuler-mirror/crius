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


use std::path::Path;

use serde_json::{json, Value};

use crate::image::content_store::ContentTransferStatus;
use crate::image::pull_cgroup::{PullCgroupEffectiveConfig, PullCgroupScopeRecord};
use crate::server::service::{RuntimeServiceConfig, RuntimeReloadState, RuntimeReloadableConfig};
use crate::storage::{ContentGcBlocker, ContentGcCandidate};

#[derive(Debug, Clone, Default)]
pub struct IntrospectionService;

impl IntrospectionService {
    pub fn runtime_backend(&self, config: &RuntimeServiceConfig) -> Value {
        json!({
            "defaultHandler": config.runtime,
            "runtimePath": config.runtime_path.display().to_string(),
            "runtimeRoot": config.runtime_root.display().to_string(),
            "handlers": config.runtime_handlers,
            "handlerBackends": config
                .runtime_configs
                .iter()
                .map(|(handler, handler_config)| {
                    (
                        handler.clone(),
                        json!({
                            "backend": handler_config.backend,
                            "backendOptions": handler_config.backend_options,
                            "runtimePath": handler_config.runtime_path,
                            "runtimeRoot": handler_config.runtime_root,
                            "snapshotter": handler_config.snapshotter,
                        }),
                    )
                })
                .collect::<serde_json::Map<String, Value>>(),
        })
    }

    pub fn image_layout(&self, config: &RuntimeServiceConfig) -> Value {
        json!({
            "mode": "single-store-root",
            "root": config.image_root.display().to_string(),
            "imageRecordPathPattern": config
                .image_root
                .join("images")
                .join("<imageID>")
                .display()
                .to_string(),
            "separateImageStoreSupported": false
        })
    }

    pub fn image_transfers(&self, status: &ContentTransferStatus) -> Value {
        json!({
            "providerModel": {
                "localBlobStore": true,
                "remoteProviders": [
                    "registry",
                    "test"
                ],
                "transferLifecycle": [
                    "running",
                    "succeeded",
                    "failed",
                    "interrupted"
                ]
            },
            "active": status.active,
            "recent": status.recent,
        })
    }

    pub fn content_gc(&self, candidates: &[ContentGcCandidate], error: Option<&str>) -> Value {
        let candidate_values = candidates
            .iter()
            .map(|candidate| {
                let blockers = candidate
                    .blockers
                    .iter()
                    .map(|blocker| match blocker {
                        ContentGcBlocker::ContentRef {
                            owner_kind,
                            owner_id,
                            ref_kind,
                        } => json!({
                            "reason": "referenced",
                            "ownerKind": owner_kind,
                            "ownerId": owner_id,
                            "refKind": ref_kind,
                        }),
                        ContentGcBlocker::ActiveTransfer {
                            transfer_id,
                            source,
                        } => json!({
                            "reason": "activeTransfer",
                            "transferId": transfer_id,
                            "source": source,
                        }),
                    })
                    .collect::<Vec<_>>();
                json!({
                    "digest": candidate.blob.digest,
                    "mediaType": candidate.blob.media_type,
                    "size": candidate.blob.size,
                    "relativePath": candidate.blob.relative_path,
                    "createdAt": candidate.blob.created_at,
                    "lastUsedAt": candidate.blob.last_used_at,
                    "blocked": !candidate.blockers.is_empty(),
                    "blockers": blockers,
                })
            })
            .collect::<Vec<_>>();
        let reclaimable_bytes: u64 = candidates
            .iter()
            .filter(|candidate| candidate.blockers.is_empty())
            .map(|candidate| candidate.blob.size)
            .sum();
        json!({
            "dryRunSupported": true,
            "deleteSupported": false,
            "candidateCount": candidates.len(),
            "reclaimableCount": candidates
                .iter()
                .filter(|candidate| candidate.blockers.is_empty())
                .count(),
            "blockedCount": candidates
                .iter()
                .filter(|candidate| !candidate.blockers.is_empty())
                .count(),
            "reclaimableBytes": reclaimable_bytes,
            "candidates": candidate_values,
            "recentResult": serde_json::Value::Null,
            "error": error,
        })
    }

    pub fn snapshot_stats_collection(&self, config: &RuntimeServiceConfig) -> Value {
        json!({
            "strategy": "on-demand-rootfs-walk",
            "backgroundCollector": false,
            "containerStatsPeriodSeconds": config.stats_collection_period,
            "podSandboxMetricsPeriodSeconds": config.pod_sandbox_metrics_collection_period
        })
    }

    pub fn runtime_feature_flags(&self, config: &RuntimeServiceConfig) -> Value {
        json!({
            "exec": true,
            "execSync": true,
            "attach": true,
            "portForward": true,
            "containerStats": true,
            "podSandboxStats": true,
            "podSandboxMetrics": true,
            "containerEvents": true,
            "podLifecycleEvents": config.enable_pod_events,
            "reopenContainerLog": true,
            "updateContainerResources": !config.disable_cgroup,
            "checkpointContainer": config.enable_criu_support,
        })
    }

    pub fn pull_cgroup(
        &self,
        effective: &PullCgroupEffectiveConfig,
        last_scope: Option<&PullCgroupScopeRecord>,
    ) -> Value {
        json!({
            "requestedValue": effective.configured,
            "effectiveMode": effective.mode,
            "enabled": effective.enabled,
            "disableCgroupDegraded": effective.disable_cgroup_degraded,
            "cgroupDriver": effective.cgroup_driver,
            "lastError": last_scope.and_then(|scope| scope.error.as_deref()),
            "effective": effective,
            "lastScope": last_scope,
        })
    }

    pub fn runtime_handler_configs(
        &self,
        config: &RuntimeServiceConfig,
        runtime_detected_features: serde_json::Map<String, Value>,
    ) -> Value {
        config
            .runtime_configs
            .iter()
            .map(|(handler, handler_config)| {
                (
                    handler.clone(),
                    json!({
                        "backend": handler_config.backend,
                        "backendOptions": handler_config.backend_options,
                        "runtimePath": handler_config.runtime_path,
                        "runtimeConfigPath": handler_config.runtime_config_path,
                        "runtimeRoot": handler_config.runtime_root,
                        "platformRuntimePaths": handler_config.platform_runtime_paths,
                        "monitorPath": handler_config.monitor_path,
                        "monitorCgroup": handler_config.monitor_cgroup,
                        "monitorEnv": handler_config.monitor_env,
                        "streamWebsockets": handler_config.stream_websockets,
                        "runtimeDetectedFeatures": runtime_detected_features
                            .get(handler)
                            .cloned()
                            .unwrap_or_else(|| json!({
                                "available": false,
                                "error": "runtime feature probe was not collected",
                            })),
                        "allowedAnnotations": handler_config.allowed_annotations,
                        "defaultAnnotations": handler_config.default_annotations,
                        "privilegedWithoutHostDevices": handler_config.privileged_without_host_devices,
                        "privilegedWithoutHostDevicesAllDevicesAllowed": handler_config
                            .privileged_without_host_devices_all_devices_allowed,
                        "containerCreateTimeoutSeconds": handler_config.container_create_timeout,
                        "snapshotter": handler_config.snapshotter,
                        "cniConfDir": config
                            .cni_config
                            .handler_config_dirs(handler)
                            .and_then(|dirs| dirs.first())
                            .map(|dir| dir.to_string_lossy().to_string()),
                        "cniMaxConfNum": config
                            .cni_config
                            .handler_max_conf_num(handler)
                            .unwrap_or(config.cni_config.max_conf_num()),
                    }),
                )
            })
            .collect::<serde_json::Map<String, Value>>()
            .into()
    }

    pub fn workloads(&self, config: &RuntimeServiceConfig) -> Value {
        config
            .workloads
            .iter()
            .map(|(name, workload)| {
                (
                    name.clone(),
                    json!({
                        "activationAnnotation": workload.activation_annotation,
                        "annotationPrefix": workload.annotation_prefix,
                        "allowedAnnotations": workload.allowed_annotations,
                        "resources": {
                            "cpuShares": workload.resources.cpu_shares,
                            "cpuQuota": workload.resources.cpu_quota,
                            "cpuPeriod": workload.resources.cpu_period,
                            "cpusetCpus": workload.resources.cpuset_cpus,
                            "cpuLimit": workload.resources.cpu_limit,
                        },
                    }),
                )
            })
            .collect::<serde_json::Map<String, Value>>()
            .into()
    }

    pub fn cgroup_support(
        &self,
        config: &RuntimeServiceConfig,
        active_version: &str,
    ) -> Value {
        json!({
            "activeVersion": active_version,
            "resourceUpdateStrategy": "runtime-update-resources",
            "disableCgroup": config.disable_cgroup,
            "tolerateMissingHugetlbController": config.tolerate_missing_hugetlb_controller,
            "resourceClasses": {
                "rdt": {
                    "softFailure": "drop-class-when-resctrl-missing",
                },
            },
            "drivers": {
                "systemd": {
                    "supported": true,
                    "monitorCgroup": {
                        "default": "system.slice",
                        "acceptedValues": ["", "pod", "*.slice"],
                    },
                    "resourceUpdatePath": "runtime update --resources",
                },
                "cgroupfs": {
                    "supported": true,
                    "monitorCgroup": {
                        "default": "",
                        "acceptedValues": ["", "pod"],
                    },
                    "resourceUpdatePath": "runtime update --resources",
                },
            },
            "versions": {
                "v1": {
                    "supported": true,
                    "hierarchyMode": "legacy",
                    "hugetlbBehavior": if config.tolerate_missing_hugetlb_controller {
                        "best-effort"
                    } else {
                        "required"
                    },
                },
                "v2": {
                    "supported": true,
                    "hierarchyMode": "unified",
                    "hugetlbBehavior": if config.tolerate_missing_hugetlb_controller {
                        "best-effort"
                    } else {
                        "required"
                    },
                },
            },
        })
    }

    pub fn reload(
        &self,
        config_path: Option<&Path>,
        reloadable_config: &RuntimeReloadableConfig,
        reload_state: &RuntimeReloadState,
    ) -> Value {
        json!({
            "strategy": "config-file-watch-and-cni-watch",
            "signalReload": false,
            "configFileWatch": reload_state.config_file_watch,
            "configFilePath": config_path.map(|path| path.display().to_string()),
            "watcherActive": reload_state.watcher_active,
            "watcherStatus": reload_state.watcher_status,
            "watcherBackoffCount": reload_state.watcher_backoff_count,
            "watcherNextRetryUnixMillis": reload_state.watcher_next_retry_unix_millis,
            "watcherLastError": reload_state.watcher_last_error,
            "cniWatchDirs": reload_state.cni_watch_dirs,
            "reloadableFields": [
                "runtime.pause_image",
                "image.pinned_images",
                "image.registry_config_dir",
                "image.global_auth_file",
                "image.namespaced_auth_dir",
                "image.signature_policy",
                "image.signature_policy_dir",
                "image.decryption_keys_path",
                "image.decryption_decoder_path",
                "image.decryption_keyprovider_config",
                "security.seccomp_profile",
                "security.apparmor_default_profile",
                "network.config_dirs",
                "network.conf_template",
                "network.max_conf_num",
                "network.default_network_name",
            ],
            "current": {
                "pauseImage": reloadable_config.pause_image,
                "pinnedImages": reloadable_config.pinned_images,
                "registryConfigDir": reloadable_config.registry_config_dir.display().to_string(),
                "globalAuthFile": reloadable_config.global_auth_file.display().to_string(),
                "namespacedAuthDir": reloadable_config.namespaced_auth_dir.display().to_string(),
                "signaturePolicy": reloadable_config.signature_policy.display().to_string(),
                "signaturePolicyDir": reloadable_config.signature_policy_dir.display().to_string(),
                "decryptionKeysPath": reloadable_config.decryption_keys_path.display().to_string(),
                "decryptionDecoderPath": reloadable_config.decryption_decoder_path,
                "decryptionKeyproviderConfig": reloadable_config
                    .decryption_keyprovider_config
                    .display()
                    .to_string(),
                "seccompProfile": reloadable_config.seccomp_profile.display().to_string(),
                "apparmorDefaultProfile": reloadable_config.apparmor_default_profile,
                "cniConfigDirs": reloadable_config
                    .cni_config_dirs
                    .iter()
                    .map(|dir| dir.display().to_string())
                    .collect::<Vec<_>>(),
                "cniConfTemplate": reloadable_config
                    .cni_conf_template
                    .as_ref()
                    .map(|path| path.display().to_string()),
                "cniMaxConfNum": reloadable_config.cni_max_conf_num,
                "cniDefaultNetworkName": reloadable_config.cni_default_network_name,
            },
            "lastReloadAtUnixMillis": reload_state.last_reload_at_unix_millis,
            "lastReloadSource": reload_state.last_reload_source,
            "lastReloadFields": reload_state.last_reload_fields,
            "lastReloadError": reload_state.last_reload_error,
            "lastCniWatchAtUnixMillis": reload_state.last_cni_watch_at_unix_millis,
            "lastCniWatchError": reload_state.last_cni_watch_error,
            "runtimeConfigApiOnly": [
                "UpdateRuntimeConfig.network_config.pod_cidr"
            ],
        })
    }

}