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


use std::unimplemented;
use std::collections::HashMap;

use crate::oci::spec::LinuxIntelRdt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceClassRequest {
    pub blockio_class: Option<String>,
    pub rdt_class: Option<String>,
}

 pub fn requested_classes_from_annotations(
    container_name: &str,
    container_annotations: &HashMap<String, String>,
    pod_annotations: &HashMap<String, String>,
) -> ResourceClassRequest {
    unimplemented!()
}

pub fn resolve_rdt_class(class_name: &str) -> Option<LinuxIntelRdt> {
    unimplemented!()
}