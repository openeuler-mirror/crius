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


//! crius-shim 模块
//!
//! 提供OCI容器运行时shim功能

pub mod daemon;
pub mod io;
pub mod process;
pub mod subreaper;

pub use daemon::{Daemon, DaemonOptions};
pub use io::{IoConfig, IoManager};
