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
use std::collections::{HashMap};
use std::sync::{Arc, RwLock};
use std::path::Path;

use tokio::sync::{Mutex, Notify};
use serde::{Serialize, Deserialize};
use tonic::{Request, Response, Status};
use oci_distribution::{secrets::RegistryAuth, Reference};
use base64::Engine;
use sha2::{Digest, Sha256};
use log::info;

use crate::proto::runtime::v1::{Image, image_service_server::ImageService};
use crate::proto::runtime::v1::*;
use crate::error::Error;
use crate::image::content_store::{RemoteContentProviderKind, ContentTransferRecord};
use crate::storage::StorageManager;

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

    fn has_registry_component(component: &str) -> bool {
        component.contains('.') || component.contains(':') || component == "localhost"
    }

    pub fn resolve_pull_reference(&self, reference:&str) -> Result<String, Status> {
        let raw = reference.trim();
        if raw.is_empty() {
            return Err(Status::invalid_argument(
                "Image reference must not be empty",
            ));
        }

        let transport = self.default_transport.trim();
        let without_transport = if let Some(rest) = raw.strip_prefix("docker://") {
            rest
        } else if raw.contains("://") {
            return Err(Status::invalid_argument(format!(
                "unsupported image transport in reference {}",
                raw
            )));
        } else {
            raw
        };

        let first_component = without_transport.split('/').next().unwrap_or_default();
        let is_short_name =
            !without_transport.contains('/') || !Self::has_registry_component(first_component);
        if is_short_name && self.short_name_mode == "enforcing" {
            return Err(Status::invalid_argument(format!(
                "short image names are rejected when image.short_name_mode = enforcing: {}",
                raw
            )));
        }
        if !transport.is_empty() && transport != "docker://" {
            return Err(Status::invalid_argument(format!(
                "unsupported image.default_transport {}",
                transport
            )));
        }

        Ok(Self::canonicalize_image_reference(without_transport))
    }

    fn canonicalize_image_reference(reference: &str) -> String {
        let raw = reference.trim();
        if raw.is_empty() || raw.starts_with("sha256:") {
            return raw.to_string();
        }

        let mut normalized = if raw.split('/').count() == 1 {
            format!("docker.io/library/{}", raw)
        } else {
            let mut parts = raw.splitn(2, '/');
            let first = parts.next().unwrap_or_default();
            let remainder = parts.next().unwrap_or_default();
            if Self::has_registry_component(first) {
                raw.to_string()
            } else {
                format!("docker.io/{}/{}", first, remainder)
            }
        };

        let has_digest = normalized.contains('@');
        let last_segment = normalized.rsplit('/').next().unwrap_or_default();
        if !has_digest && !last_segment.contains(':') {
            normalized.push_str(":latest");
        }

        normalized.into()
    }

    fn provided_registry_bearer_token(auth: &AuthConfig) -> Option<String> {
        for candidate in [&auth.registry_token, &auth.identity_token] {
            let token = candidate.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }

        None
    }

    fn current_reloadable_config(&self) -> ReloadableImageConfig {
        self.reloadable_config
            .read()
            .expect("image reloadable config lock poisoned")
            .clone()
    }

    fn image_name_for_namespaced_auth(reference: &Reference) -> String {
        format!(
            "{}/{}",
            reference.resolve_registry(),
            reference.repository()
        )
    }

    fn decode_auth_field(auth_field: &str) -> Option<(String, String)> {
        let raw = auth_field.trim();
        if raw.is_empty() {
            return None;
        }
        let encoded = raw
            .strip_prefix("Basic ")
            .or_else(|| raw.strip_prefix("basic "))
            .unwrap_or(raw);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .ok()?;
        let decoded = String::from_utf8(decoded).ok()?;
        let (username, password) = decoded.split_once(':')?;
        Some((username.to_string(), password.to_string()))
    }

    fn registry_auth_from_auth_config(auth: AuthConfig) -> Result<RegistryAuth, Status> {
        if !auth.username.is_empty() || !auth.password.is_empty() {
            return Ok(RegistryAuth::Basic(auth.username, auth.password));
        }

        if !auth.auth.trim().is_empty() {
            let (username, password) = Self::decode_auth_field(&auth.auth).ok_or_else(|| {
                Status::invalid_argument(
                    "Invalid auth.auth field: expected base64(username:password)",
                )
            })?;
            return Ok(RegistryAuth::Basic(username, password));
        }

        Ok(RegistryAuth::Anonymous)
    }

    fn namespaced_auth_file_path(&self, namespace: &str, reference: &Reference) -> Option<PathBuf> {
        let reloadable = self.current_reloadable_config();
        let root = reloadable.namespaced_auth_dir.as_ref()?;
        let namespace = namespace.trim();
        if namespace.is_empty() {
            return None;
        }

        let image_name = Self::image_name_for_namespaced_auth(reference);
        let digest = format!("{:x}", Sha256::digest(image_name.as_bytes()));
        Some(root.join(format!("{namespace}-{digest}.json")))
    }

    fn registry_auth_aliases(registry: &str) -> Vec<String> {
        let normalized = Self::normalize_registry_key(registry);
        match normalized.as_str() {
            "docker.io" | "registry-1.docker.io" | "index.docker.io" => vec![
                "docker.io".to_string(),
                "registry-1.docker.io".to_string(),
                "index.docker.io".to_string(),
                "index.docker.io/v1".to_string(),
                "index.docker.io/v1/".to_string(),
                "https://index.docker.io/v1/".to_string(),
            ],
            _ => vec![normalized],
        }
    }

    fn normalize_registry_key(value: &str) -> String {
        value
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches("/v1/")
            .trim_end_matches("/v2")
            .trim_end_matches("/v2/")
            .trim_end_matches("/v1/_catalog")
            .trim_end_matches("/v2/_catalog")
            .to_ascii_lowercase()
    }

    fn registry_auth_from_docker_entry(entry: &DockerAuthEntry) -> Option<RegistryAuth> {
        if !entry.username.trim().is_empty() || !entry.password.trim().is_empty() {
            return Some(RegistryAuth::Basic(
                entry.username.clone(),
                entry.password.clone(),
            ));
        }

        Self::decode_auth_field(&entry.auth)
            .map(|(username, password)| RegistryAuth::Basic(username, password))
    }

    fn registry_auth_from_file(
        path: &Path,
        reference: &Reference,
    ) -> Result<Option<RegistryAuth>, Status> {
        let raw = std::fs::read(path).map_err(|err| {
            Status::failed_precondition(format!(
                "failed to read auth file {}: {}",
                path.display(),
                err
            ))
        })?;
        let config: DockerConfigFile = serde_json::from_slice(&raw).map_err(|err| {
            Status::failed_precondition(format!(
                "failed to parse auth file {}: {}",
                path.display(),
                err
            ))
        })?;

        let aliases = Self::registry_auth_aliases(reference.resolve_registry());
        for alias in aliases {
            for (registry, entry) in &config.auths {
                if Self::normalize_registry_key(registry) == alias {
                    return Ok(Self::registry_auth_from_docker_entry(entry));
                }
            }
        }

        Ok(None)
    }

    fn registry_auth_from_namespaced_auth_dir(
        &self,
        reference: &Reference,
        namespace: Option<&str>,
    ) -> Result<Option<RegistryAuth>, Status> {
        let Some(path) =
            namespace.and_then(|namespace| self.namespaced_auth_file_path(namespace, reference))
        else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }

        Self::registry_auth_from_file(&path, reference)
    }

    fn normalize_image_id(id: &str) -> &str {
        id.strip_prefix("sha256:").unwrap_or(id)
    }

    fn image_id_matches(image_id: &str, candidate: &str) -> bool {
        if image_id == candidate {
            return true;
        }

        let normalized_image_id = Self::normalize_image_id(image_id);
        let normalized_candidate = Self::normalize_image_id(candidate);

        normalized_image_id == normalized_candidate
            || normalized_image_id.starts_with(normalized_candidate)
            || normalized_candidate.starts_with(normalized_image_id)
    }

    fn image_matches_ref(image: &Image, requested_ref: &str) -> bool {
        let canonical_requested = Self::canonicalize_image_reference(requested_ref);
        Self::image_id_matches(&image.id, requested_ref)
            || image.repo_tags.iter().any(|tag| {
                let canonical_tag = Self::canonicalize_image_reference(tag);
                tag == requested_ref
                    || canonical_tag == canonical_requested
                    || tag.starts_with(requested_ref)
                    || requested_ref.starts_with(tag)
                    || canonical_tag.starts_with(&canonical_requested)
                    || canonical_requested.starts_with(&canonical_tag)
            })
            || image.repo_digests.iter().any(|digest| {
                digest == requested_ref
                    || digest.starts_with(requested_ref)
                    || requested_ref.starts_with(digest)
            })
    }

    fn registry_auth_from_global_auth_file(
        &self,
        reference: &Reference,
    ) -> Result<Option<RegistryAuth>, Status> {
        let reloadable = self.current_reloadable_config();
        let Some(path) = reloadable.global_auth_file.as_ref() else {
            return Ok(None);
        };

        Self::registry_auth_from_file(path, reference)
    }

    fn pinned_pattern_matches(pattern: &str, candidate: &str) -> bool {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return false;
        }
        if pattern.starts_with('*') && pattern.ends_with('*') && pattern.len() > 2 {
            return candidate.contains(&pattern[1..pattern.len() - 1]);
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            return candidate.starts_with(prefix);
        }
        candidate == pattern
    }

    fn image_is_pinned_by_patterns<'a>(
        patterns: &[String],
        refs: impl IntoIterator<Item = &'a str>,
    ) -> bool {
        let refs: Vec<&str> = refs.into_iter().collect();
        patterns.iter().any(|pattern| {
            refs.iter()
                .copied()
                .any(|candidate| Self::pinned_pattern_matches(pattern, candidate))
        })
    }

    fn image_is_pinned_meta(&self, meta: &ImageMeta) -> bool {
        let mut refs = meta
            .repo_tags
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        refs.extend(meta.repo_digests.iter().map(String::as_str));
        if let Some(source_reference) = meta.source_reference.as_deref() {
            refs.push(source_reference);
        }
        let reloadable = self.current_reloadable_config();
        Self::image_is_pinned_by_patterns(&reloadable.pinned_image_patterns, refs)
    }

    fn image_user_fields_from_config_user(
        config_user: Option<&str>,
    ) -> (Option<Int64Value>, String) {
        let Some(config_user) = config_user
            .map(str::trim)
            .filter(|config_user| !config_user.is_empty())
        else {
            return (None, String::new());
        };

        let user = config_user.split(':').next().unwrap_or(config_user).trim();
        if user.is_empty() {
            return (None, String::new());
        }

        match user.parse::<i64>() {
            Ok(uid) => (Some(Int64Value { value: uid }), String::new()),
            Err(_) => (None, user.to_string()),
        }
    }

    fn image_from_meta(meta: &ImageMeta) -> Image {
        let (uid, username) = Self::image_user_fields_from_config_user(meta.config_user.as_deref());
        let mut image = Image {
            id: meta.id.clone(),
            repo_tags: meta.repo_tags.clone(),
            repo_digests: meta.repo_digests.clone(),
            size: meta.size,
            uid,
            username,
            pinned: meta.pinned,
            spec: meta.repo_tags.first().map(|tag| ImageSpec {
                image: tag.clone(),
                user_specified_image: tag.clone(),
                annotations: meta.annotations.clone(),
                ..Default::default()
            }),
        };
        image.repo_tags.sort();
        image.repo_tags.dedup();
        image.repo_digests.sort();
        image.repo_digests.dedup();
        image
    }

    async fn find_local_image(&self, image_ref: &str) -> Option<Image> {
        let canonical_ref = Self::canonicalize_image_reference(image_ref);
        {
            let images = self.images.lock().await;
            if let Some(image) = images.get(image_ref) {
                return Some(image.clone());
            }
            if let Some(image) = images.get(&canonical_ref) {
                return Some(image.clone());
            }
        }

        if let Ok(Some(record)) = self
            .metadata_store
            .find_by_reference(image_ref, Self::image_matches_ref)
        {
            let mut meta = record.meta;
            meta.pinned = self.image_is_pinned_meta(&meta);
            let image = Self::image_from_meta(&meta);
            let mut images = self.images.lock().await;
            for tag in &meta.repo_tags {
                images.insert(tag.clone(), image.clone());
            }
            return Some(image);
        }

        None
    }

    fn load_image_metadata(&self, image_id: &str) -> Option<ImageMeta> {
        self.metadata_store
            .load_by_id(image_id)
            .map(|record| record.meta)
    }

    // 保存镜像元数据
    async fn save_image_metadata(&self, image: &CriusImage) -> Result<(), Error> {
        self.metadata_store.save(image)?;
        Ok(())
    }

    fn push_unique(target: &mut Vec<String>, value: &str) {
        if !value.is_empty() && !target.iter().any(|existing| existing == value) {
            target.push(value.to_string());
        }
    }

    async fn persist_local_image_alias(
        &self,
        image: &Image,
        requested_ref: &str,
        canonical_ref: &str,
    ) -> Result<(), Error> {
        let Some(existing) = self.load_image_metadata(&image.id) else {
            return Ok(());
        };
        let reloadable = self.current_reloadable_config();

        let mut repo_tags = existing.repo_tags.clone();
        Self::push_unique(&mut repo_tags, canonical_ref);
        if requested_ref != canonical_ref {
            Self::push_unique(&mut repo_tags, requested_ref);
        }
        repo_tags.sort();
        repo_tags.dedup();

        self.save_image_metadata(&CriusImage {
            id: image.id.clone(),
            repo_tags,
            repo_digests: image.repo_digests.clone(),
            size: image.size,
            pinned: Self::image_is_pinned_by_patterns(
                &reloadable.pinned_image_patterns,
                image
                    .repo_tags
                    .iter()
                    .map(String::as_str)
                    .chain([canonical_ref, requested_ref]),
            ),
            pulled_at: existing.pulled_at,
            source_reference: if requested_ref != canonical_ref {
                Some(requested_ref.to_string())
            } else {
                existing.source_reference
            },
            os: existing.os,
            architecture: existing.architecture,
            config_user: existing.config_user,
            config_env: existing.config_env,
            config_entrypoint: existing.config_entrypoint,
            config_cmd: existing.config_cmd,
            config_working_dir: existing.config_working_dir,
            annotations: existing.annotations,
            declared_volumes: existing.declared_volumes,
            manifest_media_type: existing.manifest_media_type,
            selected_manifest_digest: existing.selected_manifest_digest,
            selected_platform: existing.selected_platform,
            stored_layers: existing.stored_layers,
            artifact_type: existing.artifact_type,
            artifact_blobs: existing.artifact_blobs,
        })
        .await
    }

    fn transfer_provider_kind(&self) -> RemoteContentProviderKind {
        RemoteContentProviderKind::Registry
    }

    fn persist_content_transfer_by_id(&self, transfer_id: &str) -> Result<(), Error> {
        let Some(record) = self.transfer_tracker.record(transfer_id) else {
            return Ok(());
        };
        self.persist_content_transfer_record(&record)
    }

    fn persist_content_transfer_record(&self, record: &ContentTransferRecord) -> Result<(), Error> {
        let Some(db_path) = self.ledger_db_path.as_ref() else {
            return Ok(());
        };
        StorageManager::new(db_path)
            .and_then(|mut storage| storage.save_content_transfer(&record))
            .map_err(|err| Error::Storage(format!("failed to persist content transfer: {err}")))
    }
}

#[tonic::async_trait]
impl ImageService for ImageServiceImpl {
    async fn list_images(
        &self,
        _request: Request<ListImagesRequest>,
    ) -> Result<Response<ListImagesResponse>, Status> {
        Err(tonic::Status::unimplemented("list image: not implemented"))
    }

    async fn image_status(
        &self,
        _request: Request<ImageStatusRequest>,
    ) -> Result<Response<ImageStatusResponse>, Status> {
        Err(tonic::Status::unimplemented("image status: not implemented"))
    }

    async fn pull_image(
        &self,
        request: Request<PullImageRequest>,
    ) -> Result<Response<PullImageResponse>, Status> {
        let req = request.into_inner();
        let image_spec = req
            .image
            .ok_or_else(|| Status::invalid_argument("Image spec not specified"))?;
        let requested_ref = image_spec.image.clone();
        let canonical_ref = self.resolve_pull_reference(&requested_ref)?;

        // 解析镜像引用
        let reference: Reference = canonical_ref
            .parse()
            .map_err(|e| Status::invalid_argument(format!("Invalid image reference: {}", e)))?;
        let supplied_bearer_token = req
            .auth
            .as_ref()
            .and_then(Self::provided_registry_bearer_token);
        let pull_namespace = req
            .sandbox_config
            .as_ref()
            .and_then(|config| config.metadata.as_ref())
            .map(|metadata| metadata.namespace.clone());
        let pod_cgroup_parent = req
            .sandbox_config
            .as_ref()
            .and_then(|config| config.linux.as_ref())
            .map(|linux| linux.cgroup_parent.as_str());
        let pull_cgroup_target = self
            .pull_cgroup
            .target_for_pod(pod_cgroup_parent)
            .map_err(|err| Status::invalid_argument(err.to_string()))?;

        let auth = match req.auth.clone() {
            Some(auth) => Self::registry_auth_from_auth_config(auth)?,
            None => {
                if let Some(auth) = self
                    .registry_auth_from_namespaced_auth_dir(&reference, pull_namespace.as_deref())?
                {
                    auth
                } else if let Some(auth) = self.registry_auth_from_global_auth_file(&reference)? {
                    auth
                } else {
                    RegistryAuth::Anonymous
                }
            }
        };
        let pull_key = canonical_ref.clone();

        loop {
            let wait_for_existing = {
                let mut in_progress = self.in_progress_pulls.lock().await;
                if let Some(notify) = in_progress.get(&pull_key) {
                    Some(notify.clone())
                } else {
                    in_progress.insert(pull_key.clone(), Arc::new(Notify::new()));
                    None
                }
            };

            if let Some(notify) = wait_for_existing {
                notify.notified().await;
                if let Some(existing_image) = self.find_local_image(&pull_key).await {
                    return Ok(Response::new(PullImageResponse {
                        image_ref: existing_image.id,
                    }));
                }
                continue;
            }

            break;
        }

        info!("Pulling image: {}", canonical_ref);
        info!("Checking whether image exists locally: {}", canonical_ref);
        if let Some(existing_image) = self.find_local_image(&canonical_ref).await {
            self.persist_local_image_alias(&existing_image, &requested_ref, &canonical_ref)
                .await
                .map_err(|e| Status::internal(format!("Failed to persist image alias: {}", e)))?;
            if let Some(notify) = self.in_progress_pulls.lock().await.remove(&pull_key) {
                notify.notify_waiters();
            }
            info!(
                "Image already exists locally: {} -> {}",
                canonical_ref, existing_image.id
            );
            return Ok(Response::new(PullImageResponse {
                image_ref: existing_image.id,
            }));
        }

        info!(
            "Local image not found, start remote pull: {}",
            canonical_ref
        );
        let transfer = self.transfer_tracker.start(
            canonical_ref.clone(),
            self.transfer_provider_kind(),
            "resolving",
        );
        let transfer_id = transfer.id().to_string();
        self.persist_content_transfer_by_id(&transfer_id)
            .map_err(|err| Status::internal(err.to_string()))?;

        Err(tonic::Status::unimplemented("pull image: not implemented"))
    }

    async fn remove_image(
        &self,
        _request: Request<RemoveImageRequest>,
    ) -> Result<Response<RemoveImageResponse>, Status> {
        Err(tonic::Status::unimplemented("remove image: not implemented"))
    }

    async fn image_fs_info(
        &self,
        _request: Request<ImageFsInfoRequest>,
    ) -> Result<Response<ImageFsInfoResponse>, Status> {
        Err(tonic::Status::unimplemented("image fs info: not implemented"))
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

#[derive(Debug, Deserialize, Default)]
struct DockerAuthEntry {
    #[serde(default)]
    auth: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

#[derive(Debug, Deserialize)]
struct DockerConfigFile {
    #[serde(default)]
    auths: HashMap<String, DockerAuthEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StoredLayerMeta {
    #[serde(default)]
    pub digest: String,
    pub path: String,
    pub media_type: String,
    pub source_media_type: String,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ArtifactBlobMeta {
    pub digest: String,
    pub media_type: String,
    pub path: String,
    pub size: u64,
    pub annotations: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ImageMeta {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub size: u64,
    pub pinned: bool,
    pub pulled_at: i64,
    pub source_reference: Option<String>,
    pub os: Option<String>,
    pub architecture: Option<String>,
    pub config_user: Option<String>,
    pub config_env: Vec<String>,
    pub config_entrypoint: Vec<String>,
    pub config_cmd: Vec<String>,
    pub config_working_dir: Option<String>,
    pub annotations: HashMap<String, String>,
    pub declared_volumes: Vec<String>,
    pub manifest_media_type: Option<String>,
    pub selected_manifest_digest: Option<String>,
    pub selected_platform: Option<String>,
    pub stored_layers: Vec<StoredLayerMeta>,
    pub artifact_type: Option<String>,
    pub artifact_blobs: Vec<ArtifactBlobMeta>,
}

/// 增加crius镜像定义
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CriusImage {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub size: u64,
    pub pinned: bool,
    pub pulled_at: i64,
    pub source_reference: Option<String>,
    pub os: Option<String>,
    pub architecture: Option<String>,
    pub config_user: Option<String>,
    pub config_env: Vec<String>,
    pub config_entrypoint: Vec<String>,
    pub config_cmd: Vec<String>,
    pub config_working_dir: Option<String>,
    pub annotations: HashMap<String, String>,
    pub declared_volumes: Vec<String>,
    pub manifest_media_type: Option<String>,
    pub selected_manifest_digest: Option<String>,
    pub selected_platform: Option<String>,
    pub stored_layers: Vec<StoredLayerMeta>,
    pub artifact_type: Option<String>,
    pub artifact_blobs: Vec<ArtifactBlobMeta>,
}
