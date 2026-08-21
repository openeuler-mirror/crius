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


pub mod service;

use std::collections::HashMap;
use std::path::PathBuf;
use std::vec;

use crate::config::Config;
use crate::server::service::RuntimeServiceConfig;


impl RuntimeServiceConfig {
    pub fn new(config: Config) -> Self {
        let runtime_name = config.runtime.runtime_type.clone();
        Self {
            root_dir: PathBuf::from(&config.root),
            runtime: runtime_name,
            runtime_handlers: vec![],
            runtime_root: PathBuf::from(&config.runtime.root),
            log_dir: PathBuf::from(&config.logging.dir),
            runtime_path: PathBuf::from(&config.runtime.runtime_path),
            runtime_config_path: PathBuf::from(&config.runtime.runtime_config_path),
            image_root: PathBuf::from(&config.image.root),
            image_driver: config.image.driver.clone(),
            image_global_auth_file: PathBuf::from(&config.image.global_auth_file),
            image_namespaced_auth_dir: PathBuf::from(&config.image.namespaced_auth_dir),
            image_default_transport: config.image.default_transport.clone(),
            image_short_name_mode: config.image.short_name_mode.clone(),
            image_pull_progress_timeout: config.image.pull_progress_timeout,
            image_max_concurrent_downloads: config.image.max_concurrent_downloads,
            image_pull_retry_count: config.image.pull_retry_count,
            image_registry_config_dir: PathBuf::from(&config.image.registry_config_dir),
            image_decryption_keys_path: PathBuf::from(&config.image.decryption_keys_path),
            image_decryption_decoder_path: config.image.decryption_decoder_path.clone(),
            image_decryption_keyprovider_config: PathBuf::from(
                &config.image.decryption_keyprovider_config,
            ),
            image_additional_artifact_stores: config
                .image
                .additional_artifact_stores
                .iter()
                .map(PathBuf::from)
                .collect(),
            image_signature_policy: PathBuf::from(&config.image.signature_policy),
            image_signature_policy_dir: PathBuf::from(&config.image.signature_policy_dir),
            image_storage_options: config.image.storage_options.clone(),
            image_volumes: config.image.image_volumes.clone(),
            image_pinned_images: {
                let mut pinned = config.image.pinned_images.clone();
                if !config.runtime.pause_image.trim().is_empty() {
                    pinned.push(config.runtime.pause_image.clone());
                }
                pinned.sort();
                pinned.dedup();
                pinned
            },
            image_big_files_temporary_dir: PathBuf::from(&config.image.big_files_temporary_dir),
            image_oci_artifact_mount_support: config.image.oci_artifact_mount_support,
            workloads: config.runtime.workloads.clone(),
            enable_pod_events: config.api.enable_pod_events,
            included_pod_metrics: config.api.included_pod_metrics.clone(),
            stats_collection_period: config.api.stats_collection_period,
            pod_sandbox_metrics_collection_period: config.api.pod_sandbox_metrics_collection_period,
            grpc_max_send_msg_size: config.api.grpc_max_send_msg_size,
            grpc_max_recv_msg_size: config.api.grpc_max_recv_msg_size,
            default_env: vec![],
            default_capabilities: config
                .runtime
                .default_capabilities
                .iter()
                .map(|capability| {
                    let upper = capability.trim().to_ascii_uppercase();
                    if upper.starts_with("CAP_") {
                        upper
                    } else {
                        format!("CAP_{upper}")
                    }
                })
                .collect(),
            default_sysctls: HashMap::new(),
            allowed_devices: vec![],
            device_ownership_from_security_context: config
                .runtime
                .device_ownership_from_security_context,
            add_inheritable_capabilities: config.runtime.add_inheritable_capabilities,
            default_mounts_file: PathBuf::from(&config.runtime.default_mounts_file),
            hooks_dir: config.runtime.hooks_dir.iter().map(PathBuf::from).collect(),
            absent_mount_sources_to_reject: config
                .runtime
                .absent_mount_sources_to_reject
                .iter()
                .map(PathBuf::from)
                .collect(),
            disable_proc_mount: config.runtime.disable_proc_mount,
            timezone: config.runtime.timezone.clone(),
            attach_socket_dir: PathBuf::from(&config.runtime.attach_socket_dir),
            container_exits_dir: PathBuf::from(&config.runtime.container_exits_dir),
            clean_shutdown_file: PathBuf::from(&config.runtime.clean_shutdown_file),
            container_stop_timeout: config.runtime.container_stop_timeout,
            version_file: PathBuf::from(&config.runtime.version_file),
            version_file_persist: PathBuf::from(&config.runtime.version_file_persist),
            internal_wipe: config.runtime.internal_wipe,
            internal_repair: config.runtime.internal_repair,
            bind_mount_prefix: PathBuf::from(&config.runtime.bind_mount_prefix),
            disable_cgroup: config.runtime.disable_cgroup,
            tolerate_missing_hugetlb_controller: false,
            separate_pull_cgroup: config.runtime.separate_pull_cgroup.clone(),
            uid_mappings: None,
            gid_mappings: None,
            minimum_mappable_uid: config.runtime.minimum_mappable_uid,
            minimum_mappable_gid: config.runtime.minimum_mappable_gid,
            io_uid: config.runtime.io_uid,
            io_gid: config.runtime.io_gid,
            pids_limit: config.runtime.pids_limit,
            infra_ctr_cpuset: config.runtime.infra_ctr_cpuset.clone(),
            shared_cpuset: config.runtime.shared_cpuset.clone(),
            exec_cpu_affinity: config.runtime.exec_cpu_affinity.clone(),
            irqbalance_config_file: PathBuf::from(&config.runtime.irqbalance_config_file),
            irqbalance_config_restore_file: config.runtime.irqbalance_config_restore_file.clone(),
            read_only: config.runtime.read_only,
            no_pivot: config.runtime.no_pivot,
            no_new_keyring: config.runtime.no_new_keyring,
            pause_image: config.runtime.pause_image.clone(),
            pause_command: config.runtime.pause_command.clone(),
            drop_infra_ctr: config.runtime.drop_infra_ctr,
            cgroup_driver: config.runtime.cgroup_driver.map(|driver| driver.as_proto()),
            exec_sync_io_drain_timeout: config.api.exec_sync_io_drain_timeout,
            max_container_log_line_size: config.logging.max_container_log_line_size,
            log_to_journald: config.runtime.log_to_journald,
            no_sync_log: config.runtime.no_sync_log,
            restrict_oom_score_adj: config.runtime.restrict_oom_score_adj,
            enable_unprivileged_ports: config.runtime.enable_unprivileged_ports,
            enable_unprivileged_icmp: config.runtime.enable_unprivileged_icmp,
        }
    }
}