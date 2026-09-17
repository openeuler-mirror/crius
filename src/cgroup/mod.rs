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


use std::{path::PathBuf};

use log::info;
use anyhow::{Result, Context};

/// 资源限制配置
#[derive(Debug, Clone, Default)]
pub struct ResourceLimits {
    /// CPU限制
    pub cpu: Option<CpuLimit>,
    /// 内存限制
    pub memory: Option<MemoryLimit>,
    /// 块设备IO限制
    pub blkio: Option<BlkioLimit>,
    /// 网络IO限制
    pub network: Option<NetworkLimit>,
    /// PID限制
    pub pids: Option<PidsLimit>,
}

/// CPU限制
#[derive(Debug, Clone)]
pub struct CpuLimit {
    /// CPU份额（相对权重）
    pub shares: Option<u64>,
    /// CPU配额（微秒）
    pub quota: Option<i64>,
    /// CPU周期（微秒）
    pub period: Option<u64>,
    /// 实时运行时间（微秒）
    pub realtime_runtime: Option<i64>,
    /// 实时周期（微秒）
    pub realtime_period: Option<u64>,
    /// CPU集（如"0-3,5"）
    pub cpus: Option<String>,
    /// MEM集
    pub mems: Option<String>,
}

/// 内存限制
#[derive(Debug, Clone)]
pub struct MemoryLimit {
    /// 内存限制（字节）
    pub limit: Option<i64>,
    /// 内存预留（软限制）
    pub reservation: Option<i64>,
    /// 内存+交换限制
    pub swap: Option<i64>,
    /// 内核内存限制
    pub kernel: Option<i64>,
    /// 内核TCP内存限制
    pub kernel_tcp: Option<i64>,
    /// 内存页回收阈值
    pub swappiness: Option<u64>,
    /// 禁用OOM killer
    pub disable_oom_killer: Option<bool>,
    /// 使用层级内存
    pub use_hierarchy: Option<bool>,
}

/// 块设备IO限制
#[derive(Debug, Clone)]
pub struct BlkioLimit {
    /// 权重
    pub weight: Option<u16>,
    /// 设备权重
    pub leaf_weight: Option<u16>,
    /// 设备特定限制
    pub device_weights: Vec<DeviceWeight>,
    /// 设备读取bps限制
    pub device_read_bps: Vec<DeviceThrottle>,
    /// 设备写入bps限制
    pub device_write_bps: Vec<DeviceThrottle>,
    /// 设备读取iops限制
    pub device_read_iops: Vec<DeviceThrottle>,
    /// 设备写入iops限制
    pub device_write_iops: Vec<DeviceThrottle>,
}

/// 设备权重
#[derive(Debug, Clone)]
pub struct DeviceWeight {
    pub major: i64,
    pub minor: i64,
    pub weight: Option<u16>,
    pub leaf_weight: Option<u16>,
}

/// 设备限速
#[derive(Debug, Clone)]
pub struct DeviceThrottle {
    pub major: i64,
    pub minor: i64,
    pub rate: u64,
}

/// 网络限制
#[derive(Debug, Clone)]
pub struct NetworkLimit {
    /// 网络类ID
    pub class_id: Option<u32>,
    /// 优先级
    pub priority: Option<u32>,
}

/// PID限制
#[derive(Debug, Clone)]
pub struct PidsLimit {
    /// 最大PID数量
    pub max: Option<i64>,
}

/// Cgroups管理器
pub struct CgroupManager {
    /// cgroups挂载点
    mount_point: PathBuf,
    /// cgroups版本
    version: CgroupVersion,
    /// 容器ID
    container_id: String,
}

/// Cgroups版本
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CgroupVersion {
    V1,
    V2,
}

impl CgroupManager {
    /// 创建新的cgroups管理器
    pub fn new(container_id: String) -> Result<Self> {
        let (mount_point, version) = Self::detect_cgroup_version()?;

        info!("Detected cgroups {:?} at {:?}", version, mount_point);

        Ok(Self {
            mount_point,
            version,
            container_id,
        })
    }

    /// 检测cgroups版本
    fn detect_cgroup_version() -> Result<(PathBuf, CgroupVersion)> {
        // 检查cgroup v2
        let v2_mount = PathBuf::from("/sys/fs/cgroup");
        if v2_mount.join("cgroup.controllers").exists() {
            return Ok((v2_mount, CgroupVersion::V2));
        }

        // 检查cgroup v1
        let v1_mount = PathBuf::from("/sys/fs/cgroup");
        if v1_mount.join("cpu").exists() {
            return Ok((v1_mount, CgroupVersion::V1));
        }

        Err(anyhow::anyhow!("No cgroups mount found"))
    }

    /// 设置资源限制
    pub fn set_resources(&self, limits: &ResourceLimits) -> Result<()> {
        match self.version {
            CgroupVersion::V1 => self.set_resources_v1(limits),
            CgroupVersion::V2 => self.set_resources_v2(limits),
        }
    }

    /// 写入cgroup文件
    fn write_file(&self, path: &PathBuf, content: impl AsRef<[u8]>) -> Result<()> {
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write to cgroup file: {:?}", path))?;
        Ok(())
    }

    /// 设置资源限制v1
    fn set_resources_v1(&self, limits: &ResourceLimits) -> Result<()> {
        // 设置CPU限制
        if let Some(cpu) = &limits.cpu {
            let cpu_path = self
                .mount_point
                .join("cpu")
                .join("crius")
                .join(&self.container_id);

            if let Some(shares) = cpu.shares {
                self.write_file(&cpu_path.join("cpu.shares"), shares.to_string())?;
            }
            if let Some(quota) = cpu.quota {
                self.write_file(&cpu_path.join("cpu.cfs_quota_us"), quota.to_string())?;
            }
            if let Some(period) = cpu.period {
                self.write_file(&cpu_path.join("cpu.cfs_period_us"), period.to_string())?;
            }
            if let Some(cpus) = &cpu.cpus {
                self.write_file(&cpu_path.join("cpuset.cpus"), cpus.clone())?;
            }
        }

        // 设置内存限制
        if let Some(memory) = &limits.memory {
            let mem_path = self
                .mount_point
                .join("memory")
                .join("crius")
                .join(&self.container_id);

            if let Some(limit) = memory.limit {
                self.write_file(&mem_path.join("memory.limit_in_bytes"), limit.to_string())?;
            }
            if let Some(swap) = memory.swap {
                self.write_file(
                    &mem_path.join("memory.memsw.limit_in_bytes"),
                    swap.to_string(),
                )?;
            }
            if let Some(reservation) = memory.reservation {
                self.write_file(
                    &mem_path.join("memory.soft_limit_in_bytes"),
                    reservation.to_string(),
                )?;
            }
            if let Some(swappiness) = memory.swappiness {
                self.write_file(&mem_path.join("memory.swappiness"), swappiness.to_string())?;
            }
            if let Some(true) = memory.disable_oom_killer {
                self.write_file(&mem_path.join("memory.oom_control"), "1")?;
            }
        }

        // 设置PID限制
        if let Some(pids) = &limits.pids {
            let pids_path = self
                .mount_point
                .join("pids")
                .join("crius")
                .join(&self.container_id);

            if let Some(max) = pids.max {
                self.write_file(&pids_path.join("pids.max"), max.to_string())?;
            }
        }

        info!("Set resource limits for container {}", self.container_id);
        Ok(())
    }

    /// 设置资源限制v2
    fn set_resources_v2(&self, limits: &ResourceLimits) -> Result<()> {
        let cgroup_path = self.mount_point.join("crius").join(&self.container_id);

        // 设置CPU限制
        if let Some(cpu) = &limits.cpu {
            let mut cpu_max = String::new();

            if let Some(quota) = cpu.quota {
                cpu_max.push_str(&quota.to_string());
            } else {
                cpu_max.push_str("max");
            }

            cpu_max.push(' ');

            if let Some(period) = cpu.period {
                cpu_max.push_str(&period.to_string());
            } else {
                cpu_max.push_str("100000");
            }

            self.write_file(&cgroup_path.join("cpu.max"), cpu_max)?;

            if let Some(shares) = cpu.shares {
                // v2使用cpu.weight，范围1-10000
                let weight = ((shares as f64 / 1024.0) * 100.0) as u64;
                self.write_file(&cgroup_path.join("cpu.weight"), weight.to_string())?;
            }

            if let Some(cpus) = &cpu.cpus {
                self.write_file(&cgroup_path.join("cpuset.cpus"), cpus.clone())?;
            }
        }

        // 设置内存限制
        if let Some(memory) = &limits.memory {
            if let Some(limit) = memory.limit {
                self.write_file(&cgroup_path.join("memory.max"), limit.to_string())?;
            }
            if let Some(swap) = memory.swap {
                self.write_file(&cgroup_path.join("memory.swap.max"), swap.to_string())?;
            }
            if let Some(reservation) = memory.reservation {
                self.write_file(&cgroup_path.join("memory.high"), reservation.to_string())?;
            }
        }

        // 设置PID限制
        if let Some(pids) = &limits.pids {
            if let Some(max) = pids.max {
                self.write_file(&cgroup_path.join("pids.max"), max.to_string())?;
            }
        }

        info!(
            "Set resource limits (v2) for container {}",
            self.container_id
        );
        Ok(())
    }

}