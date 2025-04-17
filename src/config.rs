use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub organization: String,
    pub project: String,
    pub personal_access_token: String,
    pub watched_users: Vec<String>,
}

impl AppConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config_str = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        let config: AppConfig = toml::from_str(&config_str)
            .with_context(|| format!("Failed to parse config file: {:?}", path.as_ref()))?;
        
        // Validate configuration
        if config.organization.is_empty() {
            return Err(anyhow::anyhow!("Organization name cannot be empty"));
        }
        
        if config.project.is_empty() {
            return Err(anyhow::anyhow!("Project name cannot be empty"));
        }
        
        if config.personal_access_token.is_empty() {
            return Err(anyhow::anyhow!("Personal access token cannot be empty"));
        }
        
        if config.watched_users.is_empty() {
            return Err(anyhow::anyhow!("Watched users list cannot be empty"));
        }
        
        Ok(config)
    }
}