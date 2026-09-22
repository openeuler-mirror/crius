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


pub mod devices;
pub mod resource_classes;
pub mod spec_patch;

use std::unimplemented;

use serde::{Deserialize, Serialize};

/// 安全配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    /// SELinux配置
    pub selinux: Option<SelinuxConfig>,
    /// AppArmor配置
    pub apparmor: Option<ApparmorConfig>,
    /// Seccomp配置
    pub seccomp: Option<SeccompConfig>,
    /// Capabilities配置
    pub capabilities: Option<CapabilitiesConfig>,
    /// NoNewPrivileges配置
    pub no_new_privileges: bool,
    /// ReadOnlyRootFilesystem
    pub read_only_root_filesystem: bool,
}

/// 安全管理器
pub struct SecurityManager {
    /// SELinux是否可用
    selinux_available: bool,
    /// AppArmor是否可用
    apparmor_available: bool,
    /// Seccomp是否可用
    seccomp_available: bool,
    /// 默认安全配置
    _default_config: SecurityConfig,
}

impl SecurityManager {
    /// 创建新的安全管理器
    pub fn new() -> Self {
        unimplemented!()
    }
}

/// SELinux配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelinuxConfig {
    /// SELinux模式 (enforcing, permissive, disabled)
    pub mode: SelinuxMode,
    /// 用户
    pub user: String,
    /// 角色
    pub role: String,
    /// 类型
    pub selinux_type: String,
    /// 级别
    pub level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SelinuxMode {
    Enforcing,
    Permissive,
    Disabled,
}

/// AppArmor配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApparmorConfig {
    /// AppArmor配置文件名称
    pub profile: String,
    /// 自定义配置文件内容
    pub custom_profile: Option<String>,
}

/// Seccomp配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompConfig {
    /// Seccomp模式 (default, unconfined, custom)
    pub mode: SeccompMode,
    /// 自定义seccomp配置文件路径
    pub profile_path: Option<String>,
    /// 自定义seccomp配置内容
    pub profile_content: Option<String>,
    /// 允许的系统调用列表
    pub syscalls: Vec<SyscallRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SeccompMode {
    Default,
    Unconfined,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallRule {
    pub names: Vec<String>,
    pub action: SeccompAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SeccompAction {
    Allow,
    Errno,
    Kill,
    Trap,
    Trace,
    Log,
}

/// Capabilities配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesConfig {
    /// 添加的能力
    pub add: Vec<String>,
    /// 删除的能力
    pub drop: Vec<String>,
}