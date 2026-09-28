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


use std::unimplemented;
use std::collections::HashSet;
use std::process::Command;
use std::collections::HashMap;

use tonic::{Request, Response, Status};
use serde_json::json;

use crate::server::service::RuntimeServiceImpl;
use crate::proto::runtime::v1::{
    CgroupDriver, PodSandboxNetworkStatus,
    ListContainersRequest, ListContainersResponse,
    ContainerStatusRequest, ContainerStatusResponse,
    StatusRequest, StatusResponse, ContainerState,
    NamespaceMode, PodIp, LinuxPodSandboxStatus,
    NamespaceOption, Namespace, RuntimeStatus,
    Container, ContainerMetadata, ImageSpec,

};
use crate::runtime::ContainerStatus;
use crate::server::state_model::
{
    StoredPodState, StoredNamespaceOptions,
    StoredContainerState,
};
use crate::defaults::{
    STATUS_RECENT_NETWORK_EVENT_LIMIT,
    INTERNAL_CONTAINER_STATE_KEY,
};

impl RuntimeServiceImpl {

    pub(super) async fn list_containers(
        &self,
        request: Request<ListContainersRequest>,
    ) -> Result<Response<ListContainersResponse>, Status> {
        let req = request.into_inner();
        let filter = if let Some(mut filter) = req.filter {
            if !filter.id.is_empty() {
                let Some(resolved_id) = self.resolve_container_id_for_filter(&filter.id).await
                else {
                    return Ok(Response::new(ListContainersResponse {
                        containers: Vec::new(),
                    }));
                };
                filter.id = resolved_id;
            }

            if !filter.pod_sandbox_id.is_empty() {
                let Some(resolved_pod_id) = self
                    .resolve_pod_sandbox_id_for_filter(&filter.pod_sandbox_id)
                    .await
                else {
                    return Ok(Response::new(ListContainersResponse {
                        containers: Vec::new(),
                    }));
                };
                filter.pod_sandbox_id = resolved_pod_id;
            }

            Some(filter)
        } else {
            None
        };
        let pod_meta_by_id: HashMap<String, (String, String, String, Option<String>)> = {
            let pod_sandboxes = self.pod_sandboxes.lock().await;
            pod_sandboxes
                .iter()
                .map(|(id, pod)| {
                    let (name, namespace, uid) = pod
                        .metadata
                        .as_ref()
                        .map(|m| (m.name.clone(), m.namespace.clone(), m.uid.clone()))
                        .unwrap_or_else(|| {
                            ("unknown".to_string(), "default".to_string(), id.clone())
                        });
                    (
                        id.clone(),
                        (name, namespace, uid, pod.labels.get("component").cloned()),
                    )
                })
                .collect()
        };
        let container_snapshots: Vec<Container> = {
            let containers = self.containers.lock().await;
            containers.values().cloned().collect()
        };

        let mut containers_list = Vec::with_capacity(container_snapshots.len());
        for mut c in container_snapshots {
            if c.metadata.is_none() {
                c.metadata = Some(ContainerMetadata {
                    name: c.id.clone(),
                    attempt: 1,
                });
            }
            if c.image.is_none() {
                c.image = Some(ImageSpec {
                    image: c.image_ref.clone(),
                    ..Default::default()
                });
            }
            if let Some((pod_name, pod_namespace, pod_uid, component)) =
                pod_meta_by_id.get(&c.pod_sandbox_id)
            {
                c.labels
                    .entry("io.kubernetes.pod.name".to_string())
                    .or_insert_with(|| pod_name.clone());
                c.labels
                    .entry("io.kubernetes.pod.namespace".to_string())
                    .or_insert_with(|| pod_namespace.clone());
                c.labels
                    .entry("io.kubernetes.pod.uid".to_string())
                    .or_insert_with(|| pod_uid.clone());
                if let Some(component) = component.as_ref() {
                    c.labels
                        .entry("component".to_string())
                        .or_insert_with(|| component.clone());
                }
            }
            c.created_at = Self::normalize_timestamp_nanos(c.created_at);
            let runtime_status = self.runtime_container_status_checked(&c.id).await;
            if matches!(runtime_status, crate::runtime::ContainerStatus::Stopped(_)) {
                if let Some(updated) = self
                    .finalize_container_stop_state(&c.id, runtime_status.clone())
                    .await?
                {
                    c = updated;
                }
            }
            c.state = Self::effective_runtime_state_for_container(&c, runtime_status);
            if let Some((pod_name, pod_namespace, pod_uid, component)) =
                pod_meta_by_id.get(&c.pod_sandbox_id)
            {
                c.labels
                    .entry("io.kubernetes.pod.name".to_string())
                    .or_insert_with(|| pod_name.clone());
                c.labels
                    .entry("io.kubernetes.pod.namespace".to_string())
                    .or_insert_with(|| pod_namespace.clone());
                c.labels
                    .entry("io.kubernetes.pod.uid".to_string())
                    .or_insert_with(|| pod_uid.clone());
                if let Some(component) = component.as_ref() {
                    c.labels
                        .entry("component".to_string())
                        .or_insert_with(|| component.clone());
                }
            }
            c.annotations = Self::external_container_annotations(&c.annotations);
            containers_list.push(c);
        }
        let mut containers_list: Vec<_> = containers_list
            .into_iter()
            .filter(|c| {
                if let Some(f) = &filter {
                    Self::container_matches_filter(c, f)
                } else {
                    true
                }
            })
            .collect();
        if filter.as_ref().is_none_or(|f| f.id.is_empty()) {
            containers_list = Self::retain_best_logical_containers(containers_list);
        }

        Ok(Response::new(ListContainersResponse {
            containers: containers_list,
        }))
    }

    pub(super) async fn container_status(
        &self,
        request: Request<ContainerStatusRequest>,
    ) -> Result<Response<ContainerStatusResponse>, Status> {
        unimplemented!()
    }

    pub(super) fn runtime_binary_version(&self) -> Option<String> {
        if !self.config.runtime_path.exists() {
            return None;
        }

        let output = Command::new(&self.config.runtime_path)
            .arg("--version")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        String::from_utf8(output.stdout)
            .ok()?
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
    }

    pub(super) async fn probe_cni_load_status(&self) -> crate::network::cni::CniLoadStatus {
        self.probe_cni_load_status_for_config(self.current_cni_config())
            .await
    }

    pub(super) async fn probe_cni_load_status_for_config(
        &self,
        cni_config: crate::network::CniConfig,
    ) -> crate::network::cni::CniLoadStatus {
        let mut cni = match crate::network::cni::CniManager::new(
            cni_config
                .plugin_dirs()
                .iter()
                .map(|dir| dir.display().to_string())
                .collect(),
            cni_config
                .config_dirs()
                .iter()
                .map(|dir| dir.display().to_string())
                .collect(),
            cni_config.cache_dir().display().to_string(),
        ) {
            Ok(cni) => cni,
            Err(err) => {
                return crate::network::cni::CniLoadStatus {
                    checked_at_unix_millis: chrono::Utc::now().timestamp_millis(),
                    ready: false,
                    reason: "CNIConfigLoadFailed".to_string(),
                    message: format!("failed to initialize CNI manager: {}", err),
                    discovered_files: Vec::new(),
                    invalid_files: Vec::new(),
                    loaded_networks: Vec::new(),
                    declared_plugins: Vec::new(),
                    missing_plugin_binaries: Vec::new(),
                    default_network_name: None,
                };
            }
        };
        cni.set_max_conf_num(cni_config.max_conf_num());
        cni.set_default_network_name(cni_config.default_network_name().map(ToOwned::to_owned));

        match cni.load_network_configs().await {
            Ok(status) => status,
            Err(err) => {
                cni.last_load_status()
                    .cloned()
                    .unwrap_or_else(|| crate::network::cni::CniLoadStatus {
                        checked_at_unix_millis: chrono::Utc::now().timestamp_millis(),
                        ready: false,
                        reason: "CNIConfigLoadFailed".to_string(),
                        message: format!("failed to load CNI network configs: {}", err),
                        discovered_files: Vec::new(),
                        invalid_files: Vec::new(),
                        loaded_networks: Vec::new(),
                        declared_plugins: Vec::new(),
                        missing_plugin_binaries: Vec::new(),
                        default_network_name: None,
                    })
            }
        }
    }

    async fn recent_internal_events_for_status(
        &self,
        subject_kind: &str,
        subject_id: &str,
        limit: usize,
    ) -> (Vec<crate::service::event::InternalEvent>, Option<String>) {
        match self
            .internal_services
            .events
            .recent_internal_events(subject_kind, subject_id, limit)
            .await
        {
            Ok(events) => (events, None),
            Err(err) => (Vec::new(), Some(err.to_string())),
        }
    }

    pub(super) fn runtime_feature_flags(&self) -> serde_json::Value {
        self.internal_services
            .introspection
            .runtime_feature_flags(&self.config)
    }

    fn cni_config_summary(config: &crate::network::CniConfig) -> serde_json::Value {
        json!({
            "configDirs": config
                .config_dirs()
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "pluginDirs": config
                .plugin_dirs()
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "cacheDir": config.cache_dir().display().to_string(),
            "maxConfNum": config.max_conf_num(),
            "defaultNetworkName": config.default_network_name(),
        })
    }

    pub(super) fn detected_cgroup_version() -> &'static str {
        if std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
            "v2"
        } else if std::path::Path::new("/sys/fs/cgroup/cpu").exists() {
            "v1"
        } else {
            "unknown"
        }
    }

    pub(super) async fn status(
        &self,
        request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();
        let runtime_condition = self
            .internal_services
            .health
            .runtime_condition_for_path(&self.config.runtime_path, self.runtime_binary_version());
        let cni_load_status = self.probe_cni_load_status().await;
        let reload_state = self.current_reload_state();
        let reloadable_config = self.current_reloadable_config();
        let network_condition = self.internal_services.health.network_condition(
            &cni_load_status,
            reload_state.last_reload_error.as_deref(),
            reload_state.last_cni_watch_error.as_deref(),
        );
        let info = if req.verbose {
            let runtime_network_config = self.runtime_network_config.lock().await.clone();
            // let resource_class_support = crate::security::resource_classes::feature_support(Some(
            //     &self.nri_config.blockio_config_path,
            // ));
            let pull_cgroup_effective = self.image_service.pull_cgroup_effective_config();
            let pull_cgroup_last_scope = self.image_service.last_pull_cgroup_scope();
            let image_transfer_status = self.image_service.content_transfer_status();
            let (content_gc_candidates, content_gc_error) = {
                let persistence = self.persistence.lock().await;
                match crate::state::StateLedger::new(&persistence).content_gc_candidates() {
                    Ok(candidates) => (candidates, None),
                    Err(err) => (Vec::new(), Some(err.to_string())),
                }
            };
            let local_network_config = self.pod_network_domain_cni_config(true);
            let cri_network_config = self.pod_network_domain_cni_config(false);
            let local_cni_load_status = self
                .probe_cni_load_status_for_config(local_network_config.clone())
                .await;
            let cri_cni_load_status = cni_load_status.clone();
            let (recent_network_runtime_events, recent_network_runtime_events_error) = self
                .recent_internal_events_for_status(
                    "network",
                    "runtime",
                    STATUS_RECENT_NETWORK_EVENT_LIMIT,
                )
                .await;
            let (recent_network_cni_events, recent_network_cni_events_error) = self
                .recent_internal_events_for_status(
                    "network",
                    "cni",
                    STATUS_RECENT_NETWORK_EVENT_LIMIT,
                )
                .await;
            let recent_network_events = json!({
                "limitPerSubject": STATUS_RECENT_NETWORK_EVENT_LIMIT,
                "runtime": recent_network_runtime_events,
                "cni": recent_network_cni_events,
                "runtimeError": recent_network_runtime_events_error,
                "cniError": recent_network_cni_events_error,
            });
            let runtime_feature_flags = self.runtime_feature_flags();
            let runtime_detected_features = self
                .config
                .runtime_configs
                .keys()
                .map(|handler| {
                    let features =
                        self.runtime
                            .runtime_for_handler(handler)
                            .map(|runtime| {
                                serde_json::to_value(runtime.probe_runtime_features())
                                    .unwrap_or_else(|_| {
                                        json!({
                                            "available": false,
                                            "error": "failed to encode runtime feature probe"
                                        })
                                    })
                            })
                            .unwrap_or_else(|err| {
                                json!({
                                    "available": false,
                                    "error": format!("failed to resolve runtime handler: {}", err),
                                })
                            });
                    (handler.clone(), features)
                })
                .collect::<serde_json::Map<String, serde_json::Value>>();
            let payload = json!({
                "runtimeName": self.cri_runtime_name(),
                "runtimeVersion": self.cri_runtime_version(),
                "runtimeApiVersion": "v1",
                "rootDir": self.config.root_dir.display().to_string(),
                "runtime": self.config.runtime.clone(),
                "defaultRuntimeHandler": self.config.runtime.clone(),
                "runtimePath": self.config.runtime_path.display().to_string(),
                "ociRuntimeVersion": self.runtime_binary_version(),
                "runtimeRoot": self.config.runtime_root.display().to_string(),
                "attachSocketDir": self.config.attach_socket_dir.display().to_string(),
                "containerExitsDir": self.config.container_exits_dir.display().to_string(),
                "containerStopTimeoutSeconds": self.config.container_stop_timeout,
                "cleanShutdownFile": self.clean_shutdown_file.display().to_string(),
                "internalWipe": self.config.internal_wipe,
                "internalRepair": self.config.internal_repair,
                "bindMountPrefix": self.config.bind_mount_prefix.display().to_string(),
                "uidMappings": self
                    .config
                    .uid_mappings
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mapping| json!({
                        "containerId": mapping.container_id,
                        "hostId": mapping.host_id,
                        "length": mapping.length,
                    }))
                    .collect::<Vec<_>>(),
                "gidMappings": self
                    .config
                    .gid_mappings
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mapping| json!({
                        "containerId": mapping.container_id,
                        "hostId": mapping.host_id,
                        "length": mapping.length,
                    }))
                    .collect::<Vec<_>>(),
                "minimumMappableUid": self.config.minimum_mappable_uid,
                "minimumMappableGid": self.config.minimum_mappable_gid,
                "ioUid": self.config.io_uid,
                "ioGid": self.config.io_gid,
                "disableCgroup": self.config.disable_cgroup,
                "tolerateMissingHugetlbController": self.config.tolerate_missing_hugetlb_controller,
                "separatePullCgroup": self.config.separate_pull_cgroup,
                "pullCgroup": self.internal_services.introspection.pull_cgroup(
                    &pull_cgroup_effective,
                    pull_cgroup_last_scope.as_ref(),
                ),
                "pidsLimit": self.config.pids_limit,
                "infraCtrCpuset": self.config.infra_ctr_cpuset.clone(),
                "sharedCpuset": self.config.shared_cpuset.clone(),
                "execCpuAffinity": self.config.exec_cpu_affinity,
                "irqbalanceConfigFile": self.config.irqbalance_config_file.display().to_string(),
                "irqbalanceConfigRestoreFile": self.config.irqbalance_config_restore_file.clone(),
                "readOnly": self.config.read_only,
                "noPivot": self.config.no_pivot,
                "noNewKeyring": self.config.no_new_keyring,
                "logDir": self.config.log_dir.display().to_string(),
                "imageRoot": self.config.image_root.display().to_string(),
                "imageDriver": self.config.image_driver.clone(),
                "imageGlobalAuthFile": self.config.image_global_auth_file.display().to_string(),
                "imageNamespacedAuthDir": self
                    .config
                    .image_namespaced_auth_dir
                    .display()
                    .to_string(),
                "imageDefaultTransport": self.config.image_default_transport.clone(),
                "imageShortNameMode": self.config.image_short_name_mode.clone(),
                "imagePullProgressTimeoutMillis": self
                    .config
                    .image_pull_progress_timeout
                    .as_millis(),
                "imageMaxConcurrentDownloads": self.config.image_max_concurrent_downloads,
                "imagePullRetryCount": self.config.image_pull_retry_count,
                "imageRegistryConfigDir": self
                    .config
                    .image_registry_config_dir
                    .display()
                    .to_string(),
                "imageDecryptionKeysPath": self
                    .config
                    .image_decryption_keys_path
                    .display()
                    .to_string(),
                "imageDecryptionDecoderPath": self.config.image_decryption_decoder_path.clone(),
                "imageDecryptionKeyproviderConfig": self
                    .config
                    .image_decryption_keyprovider_config
                    .display()
                    .to_string(),
                "imageAdditionalArtifactStores": self
                    .config
                    .image_additional_artifact_stores
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
                "imageSignaturePolicy": self
                    .config
                    .image_signature_policy
                    .display()
                    .to_string(),
                "imageSignaturePolicyDir": self
                    .config
                    .image_signature_policy_dir
                    .display()
                    .to_string(),
                "imageStorageOptions": self.config.image_storage_options.clone(),
                "imageVolumes": self.config.image_volumes.clone(),
                "pinnedImages": self.config.image_pinned_images.clone(),
                "imageBigFilesTemporaryDir": self
                    .config
                    .image_big_files_temporary_dir
                    .display()
                    .to_string(),
                "imageLayout": self.internal_services.introspection.image_layout(&self.config),
                "imageTransfers": self
                    .internal_services
                    .introspection
                    .image_transfers(&image_transfer_status),
                "contentGc": self.internal_services.introspection.content_gc(
                    &content_gc_candidates,
                    content_gc_error.as_deref(),
                ),
                "ociArtifactMountSupport": self.config.image_oci_artifact_mount_support,
                "imageDecryption": {
                    "enabled": !self.config.image_decryption_keys_path.as_os_str().is_empty(),
                    "keyModel": if self.config.image_decryption_keys_path.as_os_str().is_empty() {
                        ""
                    } else {
                        "node"
                    },
                },
                "maxContainerLogLineSize": self.config.max_container_log_line_size,
                "pauseImage": self.config.pause_image.clone(),
                "pauseCommand": self.config.pause_command.clone(),
                "dropInfraCtr": self.config.drop_infra_ctr,
                "pinnsPath": self
                    .config
                    .cni_config
                    .namespace_helper_path()
                    .map(|path| path.display().to_string()),
                "shimPidfilePattern": self
                    .shim_work_dir
                    .join("<containerID>")
                    .join("shim.pid")
                    .display()
                    .to_string(),
                "logToJournald": self.config.log_to_journald,
                "noSyncLog": self.config.no_sync_log,
                "enablePodEvents": self.config.enable_pod_events,
                "includedPodMetrics": self.config.included_pod_metrics,
                "statsCollectionPeriodSeconds": self.config.stats_collection_period,
                "podSandboxMetricsCollectionPeriodSeconds": self
                    .config
                    .pod_sandbox_metrics_collection_period,
                "restrictOomScoreAdj": self.config.restrict_oom_score_adj,
                "enableUnprivilegedPorts": self.config.enable_unprivileged_ports,
                "enableUnprivilegedIcmp": self.config.enable_unprivileged_icmp,
                "cniMaxConfNum": self.config.cni_config.max_conf_num(),
                "cniConfTemplate": self
                    .config
                    .cni_config
                    .conf_template()
                    .map(|path| path.to_string_lossy().to_string()),
                "cniIpPref": self.config.cni_config.ip_pref().as_str(),
                "netnsMountDir": self
                    .config
                    .cni_config
                    .netns_mount_dir()
                    .display()
                    .to_string(),
                "netnsMountsUnderStateDir": self
                    .config
                    .cni_config
                    .netns_mounts_under_state_dir(),
                "disableHostportMapping": self.config.cni_config.disable_hostport_mapping(),
                "defaultEnv": self
                    .config
                    .default_env
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>(),
                "defaultCapabilities": self.config.default_capabilities.clone(),
                "defaultSysctls": self.config.default_sysctls.clone(),
                "allowedDevices": self
                    .config
                    .allowed_devices
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
                "deviceOwnershipFromSecurityContext": self
                    .config
                    .device_ownership_from_security_context,
                "addInheritableCapabilities": self.config.add_inheritable_capabilities,
                "defaultMountsFile": self
                    .config
                    .default_mounts_file
                    .as_os_str()
                    .is_empty()
                    .then_some(serde_json::Value::Null)
                    .unwrap_or_else(|| json!(self.config.default_mounts_file.display().to_string())),
                "hooksDir": self
                    .config
                    .hooks_dir
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
                "absentMountSourcesToReject": self
                    .config
                    .absent_mount_sources_to_reject
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
                "disableProcMount": self.config.disable_proc_mount,
                "timezone": self.config.timezone.clone(),
                "grpcMaxSendMsgSize": self.config.grpc_max_send_msg_size,
                "grpcMaxRecvMsgSize": self.config.grpc_max_recv_msg_size,
                "privilegedSeccompProfile": self.config.privileged_seccomp_profile.clone(),
                "apparmorDefaultProfile": self.config.apparmor_default_profile.clone(),
                "disableApparmor": self.config.disable_apparmor,
                "enableSelinux": self.config.enable_selinux,
                "selinuxCategoryRange": self.config.selinux_category_range,
                "hostnetworkDisableSelinux": self.config.hostnetwork_disable_selinux,
                "runtimeHandlers": self.config.runtime_handlers.clone(),
                "runtimeHandlerConfigs": self
                    .internal_services
                    .introspection
                    .runtime_handler_configs(&self.config, runtime_detected_features),
                "workloads": self.internal_services.introspection.workloads(&self.config),
                "execSyncIoDrainTimeoutMillis": self.config.exec_sync_io_drain_timeout.as_millis(),
                "runtimeFeatures": runtime_feature_flags.clone(),
                "internalServices": {
                    "events": {
                        "subscriberCount": self.internal_services.events.subscriber_count(),
                    },
                    "introspection": {
                        "runtimeBackend": self.internal_services.introspection.runtime_backend(&self.config),
                        "imageTransfers": self.internal_services.introspection.image_transfers(
                            &image_transfer_status,
                        ),
                        "contentGc": self.internal_services.introspection.content_gc(
                            &content_gc_candidates,
                            content_gc_error.as_deref(),
                        ),
                    },
                    "health": {
                        "runtime": runtime_condition.clone(),
                        "network": network_condition.clone(),
                        "watchers": self.internal_services.health.watcher_status(
                            crate::service::health::WatcherStatusInput {
                                reload_watcher_active: reload_state.watcher_active,
                                watcher_status: serde_json::json!(reload_state.watcher_status),
                                watcher_backoff_count: reload_state.watcher_backoff_count,
                                watcher_next_retry_unix_millis: reload_state.watcher_next_retry_unix_millis,
                                watcher_last_error: reload_state.watcher_last_error.as_deref(),
                                reload_error: reload_state.last_reload_error.as_deref(),
                                cni_watch_error: reload_state.last_cni_watch_error.as_deref(),
                                shim_reconnect_supported: true,
                            }
                        ),
                    },
                },
                "networkDiagnostics": {
                    "ready": network_condition.ready,
                    "reason": network_condition.reason.clone(),
                    "message": network_condition.message.clone(),
                    "domains": {
                        "local": {
                            "purpose": "crs pod and CRS local explicit Pod networking; uses crius local CNI config and containernetworking-plugins such as bridge, host-local, loopback, and portmap",
                            "config": Self::cni_config_summary(&local_network_config),
                            "loadStatus": local_cni_load_status,
                        },
                        "cri": {
                            "purpose": "CRI consumers such as crictl/kubelet; uses standard CRI CNI config dirs and may select Kubernetes/Calico configs if configured there",
                            "config": Self::cni_config_summary(&cri_network_config),
                            "loadStatus": cri_cni_load_status,
                        },
                    },
                    "lastCniLoadStatus": cni_load_status.clone(),
                    "reload": self.internal_services.introspection.reload(
                        self.config.config_path.as_deref(),
                        &reloadable_config,
                        &reload_state,
                    ),
                    "recentEvents": recent_network_events.clone(),
                },
                "reload": self.internal_services.introspection.reload(
                    self.config.config_path.as_deref(),
                    &reloadable_config,
                    &reload_state,
                ),
                "runtimeNetworkConfig": runtime_network_config.as_ref().map(|cfg| {
                    json!({
                        "podCIDR": cfg.pod_cidr,
                    })
                }),
                "lastCniLoadStatus": cni_load_status,
                "networkReady": network_condition.ready,
                "networkReason": network_condition.reason.clone(),
                "cgroupDriver": self.cgroup_driver().as_str_name(),
                "cgroupSupport": self.internal_services.introspection.cgroup_support(
                    &self.config,
                    Self::detected_cgroup_version(),
                ),
            });
            let mut info = HashMap::new();
            info.insert(
                "config".to_string(),
                serde_json::to_string(&payload).map_err(|e| {
                    Status::internal(format!("Failed to encode runtime config info: {}", e))
                })?,
            );
            info
        } else {
            HashMap::new()
        };

        Ok(Response::new(StatusResponse {
            status: Some(RuntimeStatus {
                conditions: vec![
                    runtime_condition.runtime_condition(),
                    network_condition.network_condition(),
                ],
            }),
            info,
        }))
    }

    pub(super) fn cgroup_driver(&self) -> CgroupDriver {
        unimplemented!()
    }

    pub(super) fn runtime_state_name(runtime_state: i32) -> &'static str {
        match runtime_state {
            x if x == ContainerState::ContainerCreated as i32 => "created",
            x if x == ContainerState::ContainerRunning as i32 => "running",
            x if x == ContainerState::ContainerExited as i32 => "exited",
            _ => "unknown",
        }
    }

    pub(super) fn pod_network_status_from_state(
        state: Option<&StoredPodState>,
    ) -> Option<PodSandboxNetworkStatus> {
        let host_network = state
            .and_then(|pod| pod.namespace_options.as_ref())
            .map(|options| options.network == NamespaceMode::Node as i32)
            .unwrap_or(false);
        let primary_ip = state.and_then(|pod| pod.ip.clone()).unwrap_or_default();
        let mut seen = HashSet::new();
        let mut additional: Vec<PodIp> = state
            .map(|pod| {
                pod.additional_ips
                    .iter()
                    .filter(|ip| !ip.is_empty())
                    .filter(|ip| primary_ip.is_empty() || *ip != &primary_ip)
                    .filter(|ip| seen.insert((*ip).clone()))
                    .map(|ip| PodIp { ip: ip.clone() })
                    .collect()
            })
            .unwrap_or_default();

        if primary_ip.is_empty() {
            if additional.is_empty() {
                if host_network {
                    return Some(PodSandboxNetworkStatus {
                        ip: String::new(),
                        additional_ips: Vec::new(),
                    });
                }
                return None;
            }

            let primary = additional.remove(0);
            Some(PodSandboxNetworkStatus {
                ip: primary.ip,
                additional_ips: additional,
            })
        } else {
            Some(PodSandboxNetworkStatus {
                ip: primary_ip,
                additional_ips: additional,
            })
        }
    }

    pub(super) fn pod_linux_status_from_state(
        state: Option<&StoredPodState>,
    ) -> Option<LinuxPodSandboxStatus> {
        let options = state
            .and_then(|pod| pod.namespace_options.as_ref())
            .map(StoredNamespaceOptions::to_proto)
            .unwrap_or_else(|| NamespaceOption {
                network: crate::proto::runtime::v1::NamespaceMode::Pod as i32,
                pid: crate::proto::runtime::v1::NamespaceMode::Pod as i32,
                ipc: crate::proto::runtime::v1::NamespaceMode::Pod as i32,
                target_id: String::new(),
                userns_options: None,
            });

        Some(LinuxPodSandboxStatus {
            namespaces: Some(Namespace {
                options: Some(options),
            }),
        })
    }

    pub(super) fn cri_runtime_name(&self) -> &'static str {
        env!("CARGO_PKG_NAME")
    }

    pub(super) fn cri_runtime_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    async fn runtime_container_status_checked(&self, container_id: &str) -> ContainerStatus {
        let runtime = match self.runtime_for_container_request(container_id).await {
            Ok(runtime) => runtime,
            Err(_) => return ContainerStatus::Unknown,
        };
        let container_id = container_id.to_string();
        tokio::task::spawn_blocking(move || {
            runtime.task_controller().container_status(&container_id)
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(ContainerStatus::Unknown)
    }

    pub(super) fn effective_runtime_state_for_container(
        container: &Container,
        runtime_status: crate::runtime::ContainerStatus,
    ) -> i32 {
        let runtime_state = Self::map_runtime_container_state(runtime_status);
        if runtime_state != ContainerState::ContainerUnknown as i32 {
            return runtime_state;
        }

        let stored_state = container.state;
        if stored_state == ContainerState::ContainerExited as i32
            || stored_state == ContainerState::ContainerRunning as i32
            || stored_state == ContainerState::ContainerCreated as i32
        {
            return stored_state;
        }

        let container_state = Self::read_internal_state::<StoredContainerState>(
            &container.annotations,
            INTERNAL_CONTAINER_STATE_KEY,
        );
        if container_state
            .as_ref()
            .and_then(|state| state.finished_at)
            .is_some()
            || container_state
                .as_ref()
                .and_then(|state| state.exit_code)
                .is_some()
        {
            return ContainerState::ContainerExited as i32;
        }

        ContainerState::ContainerUnknown as i32
    }

    pub(super) fn container_matches_filter(
        container: &Container,
        filter: &crate::proto::runtime::v1::ContainerFilter,
    ) -> bool {
        if !(filter.id.is_empty()
            || container.id == filter.id
            || container.id.starts_with(&filter.id)
            || filter.id.starts_with(&container.id))
        {
            return false;
        }

        if let Some(state) = &filter.state {
            if container.state != state.state {
                return false;
            }
        }

        if !(filter.pod_sandbox_id.is_empty()
            || container.pod_sandbox_id == filter.pod_sandbox_id
            || container.pod_sandbox_id.starts_with(&filter.pod_sandbox_id)
            || filter.pod_sandbox_id.starts_with(&container.pod_sandbox_id))
        {
            return false;
        }

        for (k, v) in &filter.label_selector {
            if container.labels.get(k) != Some(v) {
                return false;
            }
        }

        true
    }

    pub(super) fn container_identity_key(container: &Container) -> Option<String> {
        let metadata = container.metadata.as_ref()?;
        let pod_uid = container
            .labels
            .get("io.kubernetes.pod.uid")
            .filter(|uid| !uid.is_empty())?;
        Some(format!("{}\u{1f}{}", pod_uid, metadata.name))
    }

    pub(super) fn container_rank(container: &Container) -> (u8, i64) {
        let live = matches!(
            container.state,
            x if x == ContainerState::ContainerRunning as i32
                || x == ContainerState::ContainerCreated as i32
        ) as u8;
        (live, container.created_at)
    }

    pub(super) fn retain_best_logical_containers(containers: Vec<Container>) -> Vec<Container> {
        let mut best: HashMap<String, Container> = HashMap::new();
        let mut passthrough = Vec::new();

        for container in containers {
            let Some(identity) = Self::container_identity_key(&container) else {
                passthrough.push(container);
                continue;
            };

            match best.entry(identity) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if Self::container_rank(&container) > Self::container_rank(entry.get()) {
                        entry.insert(container);
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(container);
                }
            }
        }

        let mut deduped: Vec<_> = best.into_values().collect();
        deduped.extend(passthrough);
        deduped
    }
}