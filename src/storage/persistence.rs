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


use crate::storage::StorageManager;

/// 状态持久化配置
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    /// 数据库文件路径
    pub db_path: std::path::PathBuf,
    /// 是否启用状态恢复
    pub enable_recovery: bool,
    /// 自动保存间隔（秒）
    pub auto_save_interval: u64,
}

/// 持久化管理器
#[derive(Debug)]
pub struct PersistenceManager {
    storage: StorageManager,
    _config: PersistenceConfig,
}