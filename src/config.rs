use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::env;

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfig {
    pub organization: String,
    pub project: String,
    pub personal_access_token: String,
    pub watched_users: Vec<String>,
    #[serde(default)]
    pub reviewer_id: Option<String>,
}

impl AppConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config_str = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        let mut config: AppConfig = toml::from_str(&config_str)
            .with_context(|| format!("Failed to parse config file: {:?}", path.as_ref()))?;
        
        // Process environment variables in the PAT value
        if config.personal_access_token.starts_with("${") && config.personal_access_token.ends_with("}") {
            // Extract the environment variable name
            let env_var_name = &config.personal_access_token[2..config.personal_access_token.len()-1];
            
            // Get the value from environment variable
            config.personal_access_token = env::var(env_var_name)
                .with_context(|| format!("Environment variable {} not set", env_var_name))?;
        }
        
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
    
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let config_str = toml::to_string(self)
            .context("Failed to serialize config")?;
            
        fs::write(&path, config_str)
            .with_context(|| format!("Failed to write config file: {:?}", path.as_ref()))?;
            
        Ok(())
    }
}