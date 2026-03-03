use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub matrix: MatrixConfig,
    pub auth: AuthConfig,
    pub rooms: RoomsConfig,
    #[serde(default)]
    pub inter_agent: InterAgentConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub user: String,
    pub password_file: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub default_allowed_users: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoomsConfig {
    pub control: ControlRoomConfig,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ControlRoomConfig {
    pub room_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    pub room_id: String,
    pub work_dir: String,
    pub store_dir: String,
    pub timeout_secs: Option<u64>,
    pub allowed_users: Option<Vec<String>>,
}

impl AgentConfig {
    pub fn timeout(&self) -> u64 {
        self.timeout_secs.unwrap_or(120)
    }

    pub fn effective_allowed_users<'a>(&'a self, defaults: &'a [String]) -> &'a [String] {
        self.allowed_users.as_deref().unwrap_or(defaults)
    }

    pub fn session_id(name: &str) -> String {
        name.to_string()
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct InterAgentConfig {
    pub timeout_secs: u64,
    pub max_depth: u8,
}

impl Default for InterAgentConfig {
    fn default() -> Self {
        Self { timeout_secs: 180, max_depth: 3 }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ObservabilityConfig {
    pub metrics_port: u16,
    pub skywalking_endpoint: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            metrics_port: 9090,
            skywalking_endpoint: "http://192.168.0.4:11800".to_string(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading config from {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("parsing config from {}", path.display()))
    }

    pub fn from_str(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-chat")
            .join("config.toml")
    }

    pub fn matrix_password(&self) -> Result<String> {
        std::fs::read_to_string(&self.matrix.password_file)
            .map(|s| s.trim().to_string())
            .with_context(|| format!("reading password from {}", self.matrix.password_file))
    }
}
