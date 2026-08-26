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


pub mod event;
pub mod health;
pub mod introspection;

use event::EventService;
use health::HealthService;
use introspection::IntrospectionService;

#[derive(Debug, Clone)]
pub struct InternalServices {
    pub events: EventService,
    pub health: HealthService,
    pub introspection: IntrospectionService,
}

impl InternalServices {
    pub fn new(events: EventService) -> Self {
        Self {
            events,
            health: HealthService,
            introspection: IntrospectionService,
        }
    }
}