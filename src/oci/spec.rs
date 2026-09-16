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


use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// OCI运行时配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    /// 配置版本
    pub oci_version: String,
    /// 进程配置
    pub process: Option<Process>,
    /// 根文件系统配置
    pub root: Option<Root>,
    /// 主机名
    pub hostname: Option<String>,
    /// 挂载点
    pub mounts: Option<Vec<Mount>>,
    /// OCI hooks
    pub hooks: Option<Hooks>,
    /// Linux命名空间配置
    pub linux: Option<Linux>,
    /// 平台特定配置
    pub annotations: Option<HashMap<String, String>>,
}

/// 进程配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    /// 终端配置
    pub terminal: Option<bool>,
    /// 用户配置
    pub user: Option<User>,
    /// 执行参数
    pub args: Vec<String>,
    /// 环境变量
    pub env: Option<Vec<String>>,
    /// 工作目录
    pub cwd: String,
    /// Capabilities
    pub capabilities: Option<LinuxCapabilities>,
    /// RLimits
    pub rlimits: Option<Vec<Rlimit>>,
    /// OOM 分数调整
    pub oom_score_adj: Option<i32>,
    /// 调度策略
    pub scheduler: Option<Scheduler>,
    /// 进程属性
    pub no_new_privileges: Option<bool>,
    /// Apparmor配置
    pub apparmor_profile: Option<String>,
    /// SELinux标签
    pub selinux_label: Option<String>,
    /// IO 优先级
    pub io_priority: Option<LinuxIoPriority>,
}

/// 用户配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// 用户UID
    pub uid: u32,
    /// 组GID
    pub gid: u32,
    /// 附加组
    pub additional_gids: Option<Vec<u32>>,
    /// 用户名
    pub username: Option<String>,
}


/// Capabilities配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxCapabilities {
    /// Bounding capabilities
    pub bounding: Option<Vec<String>>,
    /// Effective capabilities
    pub effective: Option<Vec<String>>,
    /// Inheritable capabilities
    pub inheritable: Option<Vec<String>>,
    /// Permitted capabilities
    pub permitted: Option<Vec<String>>,
    /// Ambient capabilities
    pub ambient: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scheduler {
    pub policy: String,
    pub nice: Option<i32>,
    pub priority: Option<i32>,
    pub flags: Option<Vec<String>>,
    pub runtime: Option<u64>,
    pub deadline: Option<u64>,
    pub period: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxIoPriority {
    pub class: String,
    pub priority: i32,
}

/// Rlimit配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rlimit {
    /// 资源类型
    #[serde(rename = "type")]
    pub rtype: String,
    /// 硬限制
    pub hard: u64,
    /// 软限制
    pub soft: u64,
}

/// 根文件系统配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    /// 根目录路径
    pub path: String,
    /// 是否只读
    pub readonly: Option<bool>,
}


/// 挂载点配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mount {
    /// 目标路径
    pub destination: String,
    /// 源路径
    pub source: Option<String>,
    /// 文件系统类型
    #[serde(rename = "type")]
    pub mount_type: Option<String>,
    /// 挂载选项
    pub options: Option<Vec<String>>,
}

/// OCI hooks 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hooks {
    pub prestart: Option<Vec<Hook>>,
    pub create_runtime: Option<Vec<Hook>>,
    pub create_container: Option<Vec<Hook>>,
    pub start_container: Option<Vec<Hook>>,
    pub poststart: Option<Vec<Hook>>,
    pub poststop: Option<Vec<Hook>>,
}

/// 单个 OCI hook
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub path: String,
    pub args: Option<Vec<String>>,
    pub env: Option<Vec<String>>,
    pub timeout: Option<i32>,
}

/// Linux配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Linux {
    /// 命名空间配置
    pub namespaces: Option<Vec<Namespace>>,
    /// UID映射
    pub uid_mappings: Option<Vec<IdMapping>>,
    /// GID映射
    pub gid_mappings: Option<Vec<IdMapping>>,
    /// 设备
    pub devices: Option<Vec<Device>>,
    /// 网络设备
    pub net_devices: Option<HashMap<String, LinuxNetDevice>>,
    /// Cgroups配置
    pub cgroups_path: Option<String>,
    /// 资源限制
    pub resources: Option<LinuxResources>,
    /// 根文件系统挂载传播
    pub rootfs_propagation: Option<String>,
    /// Seccomp配置
    pub seccomp: Option<Seccomp>,
    /// 系统控制
    pub sysctl: Option<HashMap<String, String>>,
    /// 挂载标签
    pub mount_label: Option<String>,
    /// 需要在容器内屏蔽的路径。
    pub masked_paths: Option<Vec<String>>,
    /// 需要在容器内设为只读的路径。
    pub readonly_paths: Option<Vec<String>>,
    /// Intel RDT资源控制
    pub intel_rdt: Option<LinuxIntelRdt>,
}

/// 命名空间配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    /// 命名空间类型
    #[serde(rename = "type")]
    pub ns_type: String,
    /// 命名空间路径（用于加入现有命名空间）
    pub path: Option<String>,
}

/// ID映射配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdMapping {
    /// 容器内起始ID
    pub container_id: u32,
    /// 宿主机起始ID
    pub host_id: u32,
    /// 映射大小
    pub size: u32,
}

/// 设备配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// 设备类型
    #[serde(rename = "type")]
    pub device_type: String,
    /// 设备路径
    pub path: String,
    /// 主设备号
    pub major: Option<i64>,
    /// 次设备号
    pub minor: Option<i64>,
    /// 文件权限
    pub file_mode: Option<u32>,
    /// UID
    pub uid: Option<u32>,
    /// GID
    pub gid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxNetDevice {
    pub name: String,
}

/// Linux资源限制
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxResources {
    /// 网络限制
    pub network: Option<LinuxNetwork>,
    /// PID限制
    pub pids: Option<LinuxPids>,
    /// 内存限制
    pub memory: Option<LinuxMemory>,
    /// CPU限制
    pub cpu: Option<LinuxCpu>,
    /// 块IO限制
    pub block_io: Option<LinuxBlockIo>,
    /// 巨页限制
    pub hugepage_limits: Option<Vec<LinuxHugepageLimit>>,
    /// 设备限制
    pub devices: Option<Vec<LinuxDeviceCgroup>>,
    /// RDT资源控制
    pub intel_rdt: Option<LinuxIntelRdt>,
    /// 统一资源限制
    pub unified: Option<HashMap<String, String>>,
}

/// 网络限制
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxNetwork {
    /// 接口优先级
    pub class_id: Option<u32>,
    /// 优先级策略
    pub priorities: Option<Vec<LinuxInterfacePriority>>,
}

/// 接口优先级
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxInterfacePriority {
    /// 接口名称
    pub name: String,
    /// 优先级
    pub priority: u32,
}

/// PID限制
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxPids {
    /// 最大PID数
    pub limit: i64,
}

/// 内存限制
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxMemory {
    /// 内存限制
    pub limit: Option<i64>,
    /// 交换限制
    pub swap: Option<i64>,
    /// 内核内存限制
    pub kernel: Option<i64>,
    /// TCP内存限制
    pub kernel_tcp: Option<i64>,
    /// 内存软限制
    pub reservation: Option<i64>,
    /// Swappiness
    pub swappiness: Option<u64>,
    /// 禁用OOM killer
    pub disable_oom_killer: Option<bool>,
    /// 使用层级内存
    pub use_hierarchy: Option<bool>,
}

/// CPU限制
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxCpu {
    /// CPU shares
    pub shares: Option<u64>,
    /// CPU配额
    pub quota: Option<i64>,
    /// CPU周期
    pub period: Option<u64>,
    /// 实时运行时间
    pub realtime_runtime: Option<i64>,
    /// 实时周期
    pub realtime_period: Option<u64>,
    /// CPU亲和性
    pub cpus: Option<String>,
    /// MEM亲和性
    pub mems: Option<String>,
}

/// 块IO限制
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxBlockIo {
    /// 权重
    pub weight: Option<u16>,
    /// 叶节点权重
    pub leaf_weight: Option<u16>,
    /// 设备权重
    pub weight_device: Option<Vec<LinuxWeightDevice>>,
    /// 读取速率限制
    pub throttle_read_bps_device: Option<Vec<LinuxThrottleDevice>>,
    /// 写入速率限制
    pub throttle_write_bps_device: Option<Vec<LinuxThrottleDevice>>,
    /// 读取IOPS限制
    pub throttle_read_iops_device: Option<Vec<LinuxThrottleDevice>>,
    /// 写入IOPS限制
    pub throttle_write_iops_device: Option<Vec<LinuxThrottleDevice>>,
}

/// 设备权重
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxWeightDevice {
    /// 主设备号
    pub major: i64,
    /// 次设备号
    pub minor: i64,
    /// 权重
    pub weight: Option<u16>,
    /// 叶节点权重
    pub leaf_weight: Option<u16>,
}

/// 速率限制设备
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxThrottleDevice {
    /// 主设备号
    pub major: i64,
    /// 次设备号
    pub minor: i64,
    /// 速率
    pub rate: u64,
}

/// 巨页限制
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxHugepageLimit {
    /// 页大小
    pub page_size: String,
    /// 限制
    pub limit: u64,
}

/// 设备Cgroup配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxDeviceCgroup {
    /// 是否允许
    pub allow: bool,
    /// 设备类型
    #[serde(rename = "type")]
    pub device_type: Option<String>,
    /// 主设备号
    pub major: Option<i64>,
    /// 次设备号
    pub minor: Option<i64>,
    /// 访问权限
    pub access: Option<String>,
}

/// RDT资源控制
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxIntelRdt {
    /// 关闭内存带宽分配
    pub clos_id: Option<String>,
    /// L3缓存模式
    pub l3_cache_schema: Option<String>,
    /// 内存带宽模式
    pub mem_bw_schema: Option<String>,
    /// 是否启用内存带宽分配
    pub enable_cmt: Option<bool>,
    /// 是否启用内存带宽监控
    pub enable_mbm: Option<bool>,
}

/// Seccomp配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seccomp {
    /// Seccomp模式
    pub default_action: String,
    /// 默认 errno 返回值
    pub default_errno_ret: Option<u32>,
    /// 架构
    pub architectures: Option<Vec<String>>,
    /// seccomp flags
    pub flags: Option<Vec<String>>,
    /// listener socket path
    pub listener_path: Option<String>,
    /// listener metadata
    pub listener_metadata: Option<String>,
    /// 系统调用
    pub syscalls: Option<Vec<SeccompSyscall>>,
}

/// Seccomp系统调用
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeccompSyscall {
    /// 动作
    pub action: String,
    /// 名称
    pub names: Vec<String>,
    /// 条件
    pub args: Option<Vec<SeccompArg>>,
    /// errno 返回值
    pub errno_ret: Option<u32>,
}

/// Seccomp参数条件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeccompArg {
    /// 索引
    pub index: u32,
    /// 值
    pub value: u64,
    /// 值高32位
    pub value_two: Option<u64>,
    /// 操作符
    pub op: String,
}

/// Intel RDT
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelRdt {
    /// 是否启用内存带宽分配
    pub l3_cache: Option<bool>,
    /// 是否启用内存带宽监控
    pub mem_bw: Option<bool>,
}
