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

pub mod content_store;
pub mod metadata_store;
pub mod pull_cgroup;

use std::path::PathBuf;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::Notify;
use serde::{Serialize, Deserialize};

use crate::proto::runtime::v1::{Image};
use crate::error::Error;

use content_store::{FsContentStore, ContentTransferTracker};
use metadata_store::FilesystemImageMetadataStore;
use pull_cgroup::PullCgroupExecutor;

/// 镜像服务实现
#[derive(Clone)]
pub struct ImageServiceImpl {
    // 存储镜像信息的线程安全HashMap
    images: Arc<tokio::sync::Mutex<HashMap<String, Image>>>,
    storage_path: PathBuf,
    storage_driver: String,
    storage_options: Vec<String>,
    parsed_storage_options: OverlayImageStorageOptions,
    content_store: Arc<FsContentStore>,
    metadata_store: Arc<FilesystemImageMetadataStore>,
    default_transport: String,
    short_name_mode: String,
    pull_progress_timeout: std::time::Duration,
    max_concurrent_downloads: usize,
    pull_retry_count: u32,
    additional_artifact_stores: Vec<PathBuf>,
    _big_files_temporary_dir: Option<PathBuf>,
    in_progress_pulls: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    transfer_tracker: ContentTransferTracker,
    reloadable_config: Arc<RwLock<ReloadableImageConfig>>,
    pull_cgroup: PullCgroupExecutor,
    ledger_db_path: Option<PathBuf>,
}

impl ImageServiceImpl {
    pub fn new_with_options(options: ImageServiceOptions) -> Result<Self, Error> {
        let ImageServiceOptions {
            storage_path,
            ledger_db_path,
            storage_driver,
            storage_options,
            global_auth_file,
            namespaced_auth_dir,
            default_transport,
            short_name_mode,
            pull_progress_timeout,
            max_concurrent_downloads,
            pull_retry_count,
            registry_config_dir,
            decryption_keys_path,
            decryption_decoder_path,
            decryption_keyprovider_config,
            additional_artifact_stores,
            pinned_image_patterns,
            signature_policy,
            signature_policy_dir,
            big_files_temporary_dir,
            separate_pull_cgroup,
            cgroup_driver,
            disable_cgroup,
        } = options;
        let pull_cgroup = PullCgroupExecutor::new(
            separate_pull_cgroup,
            cgroup_driver,
            disable_cgroup,
        );
        let reloadable_config = ReloadableImageConfig {
            global_auth_file,
            namespaced_auth_dir,
            registry_config_dir,
            decryption_keys_path,
            decryption_decoder_path,
            decryption_keyprovider_config,
            pinned_image_patterns,
            signature_policy,
            signature_policy_dir,
        };
        let storage_driver = storage_driver.trim().to_string();

        if storage_driver != "overlay" {
            return Err(Error::Config(format!(
                "image.driver must be \"overlay\", got {}",
                storage_driver
            )));
        }
        let parsed_storage_options =
            OverlayImageStorageOptions::parse(&storage_driver, &storage_options)?;

        if !storage_path.exists() {
            std::fs::create_dir_all(&storage_path)?;
        }

        let images = std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let content_store = Arc::new(FsContentStore::new_with_ledger(
            &storage_path,
            ledger_db_path.clone(),
        )?);
        let transfer_tracker = ContentTransferTracker::new_with_ledger(ledger_db_path.clone())?;
        let metadata_store = Arc::new(FilesystemImageMetadataStore::new(
            &storage_path,
            additional_artifact_stores.clone(),
            ledger_db_path.clone(),
        ));

        Ok(Self {
            images,
            storage_path,
            storage_driver,
            storage_options,
            parsed_storage_options,
            content_store,
            metadata_store,
            default_transport,
            short_name_mode,
            pull_progress_timeout,
            max_concurrent_downloads,
            pull_retry_count,
            additional_artifact_stores,
            _big_files_temporary_dir: big_files_temporary_dir,
            in_progress_pulls: Arc::new(Mutex::new(HashMap::new())),
            transfer_tracker,
            reloadable_config: Arc::new(RwLock::new(reloadable_config)),
            pull_cgroup,
            ledger_db_path,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayImageStorageOptions {
    pub mount_program: Option<PathBuf>,
    pub ignore_chown_errors: bool,
}

impl OverlayImageStorageOptions {
    fn parse(storage_driver: &str, options: &[String]) -> Result<Self, Error> {
        if storage_driver != "overlay" && !options.is_empty() {
            return Err(Error::Config(format!(
                "image.storage_options is not supported for image.driver {storage_driver}"
            )));
        }

        let parsed = Self::default();

        Ok(parsed)
    }
}

#[derive(Debug, Clone)]
pub struct ReloadableImageConfig {
    pub global_auth_file: Option<PathBuf>,
    pub namespaced_auth_dir: Option<PathBuf>,
    pub registry_config_dir: Option<PathBuf>,
    pub decryption_keys_path: Option<PathBuf>,
    pub decryption_decoder_path: String,
    pub decryption_keyprovider_config: Option<PathBuf>,
    pub pinned_image_patterns: Vec<String>,
    pub signature_policy: Option<PathBuf>,
    pub signature_policy_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ImageServiceOptions {
    pub storage_path: PathBuf,
    pub ledger_db_path: Option<PathBuf>,
    pub storage_driver: String,
    pub storage_options: Vec<String>,
    pub global_auth_file: Option<PathBuf>,
    pub namespaced_auth_dir: Option<PathBuf>,
    pub default_transport: String,
    pub short_name_mode: String,
    pub pull_progress_timeout: std::time::Duration,
    pub max_concurrent_downloads: usize,
    pub pull_retry_count: u32,
    pub registry_config_dir: Option<PathBuf>,
    pub decryption_keys_path: Option<PathBuf>,
    pub decryption_decoder_path: String,
    pub decryption_keyprovider_config: Option<PathBuf>,
    pub additional_artifact_stores: Vec<PathBuf>,
    pub pinned_image_patterns: Vec<String>,
    pub signature_policy: Option<PathBuf>,
    pub signature_policy_dir: Option<PathBuf>,
    pub big_files_temporary_dir: Option<PathBuf>,
    pub separate_pull_cgroup: String,
    pub cgroup_driver: crate::config::CgroupDriverConfig,
    pub disable_cgroup: bool,
}

