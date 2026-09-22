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

pub mod runtime {
    #[allow(clippy::doc_lazy_continuation)]
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/runtime.v1.rs"));
    }
}

pub mod local {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/local.v1.rs"));
    }
}

pub mod diagnostics {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/diagnostics.v1.rs"));
    }
}