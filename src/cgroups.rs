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
// Minimal cgroup manager stub for crius-shim.
//
// `shim/daemon.rs` only uses `CgroupManager` inside `update_resources`, a
// kept non-core RPC. The minimal shim does not actually enforce cgroup
// resource limits (the core create/start/exec/stop paths do not depend on
// it), so `set_resources` is a no-op. If real enforcement is needed later,
// port the github cgroups implementation (which pulls in `crate::oci`).

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct CgroupManager {
    #[allow(dead_code)]
    container_id: String,
}

impl CgroupManager {
    pub fn new(container_id: String) -> Result<Self> {
        Ok(Self { container_id })
    }

    /// No-op in the minimal shim path.
    pub fn set_resources(&self, _limits: &crate::runtime::Limits) -> Result<()> {
        Ok(())
    }
}
