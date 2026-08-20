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


pub const DEFAULT_CRI_SOCKET_URI :&str = "unix:///run/crius/crius.sock";
pub const DEFAULT_CONTAINER_STORAGE_DIR: &str = "/var/lib/containers/storage";
pub const DEFAULT_STORAGE_DRIVER: &str = "overlay";
pub const DEFAULT_GRPC_MAX_MESSAGE_SIZE_BYTES: u32 = 80 * 1024 * 1024;
