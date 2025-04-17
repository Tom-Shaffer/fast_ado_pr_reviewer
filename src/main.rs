use anyhow::{Context, Result};
use clap::Parser;
use log::{error, info, warn};
use std::time::Duration;
use tokio::time;

mod ado_client;
mod config;
mod models;

use ado_client::AzureDevOpsClient;
use config::AppConfig;

/// Fast PR Reviewer - Automatically approve PRs from specified users
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Path to config file
    #[clap(short, long, default_value = "config.toml")]
    config: String,

    /// Polling interval in seconds
    #[clap(short, long, default_value = "5")]
    interval: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Load configuration
    let config = AppConfig::from_file(&args.config)
        .context("Failed to load configuration")?;
    
    info!("Starting FastPRReviewer bot");
    info!("Organization: {}", config.organization);
    info!("Project: {}", config.project);
    info!("Watching PRs from {} users", config.watched_users.len());
    info!("Polling interval: {} seconds", args.interval);
    
    // Create Azure DevOps client
    let ado_client = AzureDevOpsClient::new(
        &config.organization,
        &config.project,
        &config.personal_access_token,
    );
    
    let polling_interval = Duration::from_secs(args.interval);
    
    // Main loop - Poll for new PRs and approve them
    loop {
        match check_and_approve_prs(&ado_client, &config).await {
            Ok(_) => (),
            Err(e) => error!("Error checking PRs: {}", e),
        }
        
        // Wait before checking again
        time::sleep(polling_interval).await;
    }
}

async fn check_and_approve_prs(client: &AzureDevOpsClient, config: &AppConfig) -> Result<()> {
    // Get active pull requests
    let prs = client.get_active_pull_requests().await?;
    
    info!("Found {} active pull requests", prs.len());
    
    // Filter PRs by watched users and approve them
    for pr in prs {
        // Check if PR creator is in our watch list
        if config.watched_users.contains(&pr.created_by.display_name) {
            info!(
                "Found PR #{} from watched user {}: {}",
                pr.pull_request_id, pr.created_by.display_name, pr.title
            );
            
            // Check if we've already approved this PR
            match client.check_approval_status(&pr.pull_request_id.to_string()).await {
                Ok(already_approved) => {
                    if already_approved {
                        info!("PR #{} is already approved by us", pr.pull_request_id);
                        continue;
                    }
                },
                Err(e) => {
                    warn!("Failed to check approval status for PR #{}: {}", pr.pull_request_id, e);
                    // Continue to approval attempt
                }
            }
            
            // Try to approve the PR
            match client.approve_pull_request(&pr.pull_request_id.to_string()).await {
                Ok(_) => {
                    info!("Successfully approved PR #{} from {}", pr.pull_request_id, pr.created_by.display_name);
                }
                Err(e) => {
                    error!("Failed to approve PR #{}: {}", pr.pull_request_id, e);
                }
            }
        }
    }
    
    Ok(())
}
