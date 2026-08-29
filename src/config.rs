use anyhow::{Context, Result};
use serde::Deserialize;

use crate::mqtt::MqttConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationsConfig {
    pub skill_id: String,
    pub user_id: String,
    pub oauth_token: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub notifications: Option<NotificationsConfig>,
    #[serde(default)]
    pub mqtt: Option<MqttConfig>,
    pub devices: toml::Table,
}

pub fn load(path: &str) -> Result<Config> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read config {path}"))?;
    toml::from_str(&text).with_context(|| format!("cannot parse config {path}"))
}
