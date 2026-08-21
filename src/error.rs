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
use thiserror::Error;

/// 自定义错误类型
#[derive(Error, Debug)]
pub enum Error {
    /// I/O错误
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// 配置错误
    #[error("config error: {0}")]
    Config(String),
}

/// 本 crate 统一使用的 `Result` 类型别名。
pub type Result<T> = std::result::Result<T, Error>;

// 为其他错误类型实现From trait
impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Error::Config(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Config(err.to_string())
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Error::Config(err.to_string())
    }
}