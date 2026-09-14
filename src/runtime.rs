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
// Minimal runtime stub for crius-shim.
//
// `shim/daemon.rs` invokes the container runtime (runc) directly via its own
// `runtime_command()` helper (`Command::new(&self.runtime)`), so it does NOT
// need the full github `runtime` module (~4059 lines, which also pulls in
// oci/security/rootless and the daemon-side `ShimManager`). Only two static
// helpers are referenced:
//   - `apply_exec_cpu_affinity_to_std_command` (ported from github, real nix
//     implementation, not a stub)
//   - `cri_to_limits` (used only by `update_resources`, a kept non-core RPC;
//     returns a minimal `Limits` type consumed by the cgroup stub below)

use std::os::unix::process::CommandExt;
use std::process::Command;

/// Placeholder runtime type. The shim drives runc directly; this exists only
/// to host the two static helpers below.
#[derive(Debug, Clone)]
pub struct RuncRuntime;

/// Minimal resource-limits type consumed by `cgroups::CgroupManager`.
/// `update_resources` is a kept non-core RPC; its cgroup application is a
/// no-op in the minimal shim, so this type carries no real fields.
#[derive(Debug, Clone, Default)]
pub struct Limits;

impl RuncRuntime {
    /// Pin the spawned process to a single CPU if requested. Ported verbatim
    /// from the github implementation (nix::sched), so CPU affinity is a real
    /// behavior, not a stub.
    pub fn apply_exec_cpu_affinity_to_std_command(command: &mut Command, cpu: Option<usize>) {
        let Some(cpu) = cpu else {
            return;
        };
        unsafe {
            command.pre_exec(move || {
                let mut set = nix::sched::CpuSet::new();
                set.set(cpu)
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
                nix::sched::sched_setaffinity(nix::unistd::Pid::from_raw(0), &set)
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
                Ok(())
            });
        }
    }

    /// Convert CRI `LinuxContainerResources` into the minimal `Limits` type.
    /// Real resource enforcement is delegated to the (no-op) cgroup stub in
    /// the minimal shim path.
    pub fn cri_to_limits(
        _resources: &crate::proto::runtime::v1::LinuxContainerResources,
    ) -> Limits {
        Limits
    }
}
