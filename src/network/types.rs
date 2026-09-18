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


use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Pod 主 IP 选择策略。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MainIpPreference {
    Ipv4,
    Ipv6,
    #[default]
    Cni,
}

/// 网络接口信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// 接口名称
    pub name: String,

    /// IP 地址
    pub ip: Option<IpAddr>,

    /// MAC 地址
    pub mac: Option<String>,

    /// 子网掩码
    pub netmask: Option<String>,

    /// 网关
    pub gateway: Option<IpAddr>,
}

/// 网络状态
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkStatus {
    /// 网络名称
    pub name: String,

    /// IP 地址
    pub ip: Option<IpAddr>,

    /// MAC 地址
    pub mac: Option<String>,

    /// 网络接口列表
    pub interfaces: Vec<NetworkInterface>,

    /// 原始 CNI 结果，用于恢复 additional IP 顺序和调试
    pub raw_result: Option<Value>,
}