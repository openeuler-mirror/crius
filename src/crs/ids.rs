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


pub(crate) fn truncate_field(value: &str, no_trunc: bool) -> String {
    const MAX_FIELD_WIDTH: usize = 96;

    if no_trunc || value.chars().count() <= MAX_FIELD_WIDTH {
        return value.to_string();
    }

    let mut truncated = value.chars().take(MAX_FIELD_WIDTH - 3).collect::<String>();
    truncated.push_str("...");
    truncated
}


pub(crate) fn short_id(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

pub(crate) fn short_image_id(id: &str) -> String {
    let trimmed = id.strip_prefix("sha256:").unwrap_or(id);
    short_id(trimmed).to_string()
}
