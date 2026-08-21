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


use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FilesystemImageMetadataStore {
    storage_root: PathBuf,
    additional_artifact_stores: Vec<PathBuf>,
    ledger_db_path: Option<PathBuf>,
}

impl FilesystemImageMetadataStore {
    pub fn new(
        storage_root: impl AsRef<Path>,
        additional_artifact_stores: Vec<PathBuf>,
        ledger_db_path: Option<PathBuf>,
    ) -> Self {
        Self {
            storage_root: storage_root.as_ref().to_path_buf(),
            additional_artifact_stores,
            ledger_db_path,
        }
    }
}