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


use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use thiserror::Error;

/// 网络模块错误类型
#[derive(Debug, Error)]
pub enum NetworkError {
    /// I/O 错误
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// JSON 序列化/反序列化错误
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// 命令执行错误
    #[error("Command '{command}' failed with status: {status}")]
    CommandExecutionError { command: String, status: ExitStatus },

    /// 不支持的配置
    #[error("Unsupported configuration: {0}")]
    UnsupportedConfig(String),

    /// 无效参数
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// 网络错误
    #[error("Network error: {0}")]
    Network(String),

    /// Rootless 网络 helper 缺失或不可执行
    #[error(
        "RootlessNetworkHelperMissing: rootless network helper {helper} at {path} is missing or not executable"
    )]
    RootlessNetworkHelperMissing { helper: String, path: PathBuf },

    /// 其他错误
    #[error("{0}")]
    Other(String),
}
