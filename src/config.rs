// Copyright 2026 Mark Mandel
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::path::PathBuf;

use serde::Deserialize;

use crate::Result;

#[derive(Deserialize)]
pub struct Config {
    pub client_id: String,
    pub client_secret: String,
}

pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".into());
    Ok(PathBuf::from(home)
        .join(".config")
        .join("discord-notification-center"))
}

pub fn load_config() -> Result<Config> {
    let path = config_dir()?.join("config.toml");

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read config at {}: {e}", path.display()))?;

    let config: Config = toml::from_str(&raw)
        .map_err(|e| format!("invalid config at {}: {e}", path.display()))?;

    Ok(config)
}
