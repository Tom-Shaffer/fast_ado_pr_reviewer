use anyhow::{Context, Result};
use clap::Parser;
use log::{error, info, warn};
use std::time::Duration;
use std::collections::HashSet;
use std::io::{self, Write};
use tokio::{time, signal, sync::oneshot};
use tokio::sync::Mutex;
use env_logger::Env;
use std::sync::Arc;
use lazy_static::lazy_static;
use chrono::{DateTime, Utc};

mod ado_client;
mod config;
mod models;

use ado_client::AzureDevOpsClient;
use config::AppConfig;

// Use lazy_static with a mutex to safely track previously seen PRs
lazy_static! {
    static ref SEEN_PRS: Arc<Mutex<HashSet<i32>>> = Arc::new(Mutex::new(HashSet::new()));
    static ref PROGRAM_START_TIME: DateTime<Utc> = Utc::now();
}

/// Fast PR Reviewer - Automatically approve PRs from specified users
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Path to config file
    #[clap(short, long, default_value = "config.toml")]
    config: String,

    /// Polling interval in seconds
    #[clap(short, long, default_value = "1")]
    interval: u64,
    
    /// Users to watch for PRs (overrides config file)
    #[clap(trailing_var_arg = true)]
    watched_users: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger with custom settings to always show info logs
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Load configuration
    let mut config = AppConfig::from_file(&args.config)
        .context("Failed to load configuration")?;
    
    // Override watched users with CLI arguments if provided
    if !args.watched_users.is_empty() {
        info!("Overriding watched users from config with CLI arguments");
        config.watched_users = args.watched_users;
    }
    
    // Create Azure DevOps client
    let ado_client = AzureDevOpsClient::new(
        &config.organization,
        &config.project,
        &config.personal_access_token,
    );
    
    // Check if reviewer ID is set, if not prompt the user to set it
    if config.reviewer_id.is_none() {
        info!("No reviewer ID configured. Let's set it up.");
        config.reviewer_id = setup_reviewer_id(&ado_client, &args.config).await?;
    }
    
    info!("Starting FastPRReviewer bot");
    info!("Organization: {}", config.organization);
    info!("Project: {}", config.project);
    info!("Polling interval: {} seconds", args.interval);
    if let Some(reviewer_id) = &config.reviewer_id {
        info!("Using reviewer ID: {}", reviewer_id);
    }
    
    // Log who we're watching for PRs
    if !config.watched_users.is_empty() {
        info!("👀 Watching PRs from {} users:", config.watched_users.len());
        for user in &config.watched_users {
            info!("  • {}", user);
        }
    } else {
        warn!("No users being watched! Add users to config.toml or specify them as command line arguments.");
    }
    
    // Create a channel to signal shutdown
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
    
    // Handle Ctrl+C signal
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Received Ctrl+C, initiating graceful shutdown...");
                let _ = shutdown_tx.send(());
            }
            Err(err) => {
                error!("Failed to listen for Ctrl+C signal: {}", err);
            }
        }
    });
    
    let polling_interval = Duration::from_secs(args.interval);
    
    // Main loop - Poll for new PRs and approve them until shutdown signal
    loop {
        // Check if shutdown was requested
        if shutdown_rx.try_recv().is_ok() {
            info!("Shutting down...");
            break;
        }
        
        match check_and_approve_prs(&ado_client, &config).await {
            Ok(_) => (),
            Err(e) => error!("Error checking PRs: {}", e),
        }
        
        // Wait before checking again, but also listen for shutdown signal
        tokio::select! {
            _ = time::sleep(polling_interval) => {}
            _ = &mut shutdown_rx => {
                info!("Shutting down...");
                break;
            }
        }
    }
    
    info!("FastPRReviewer bot has stopped");
    Ok(())
}

/// Function to set up the reviewer ID by looking up reviewers on a PR
async fn setup_reviewer_id(client: &AzureDevOpsClient, config_path: &str) -> Result<Option<String>> {
    println!("You need to set up your reviewer ID.");
    println!("To do this, please provide a pull request number where you are listed as a reviewer.");
    
    print!("Enter a PR number: ");
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    
    let pr_id = input.trim().parse::<i32>()
        .context("Invalid PR number. Please enter a valid integer.")?;
        
    // Fetch the PR
    let pr = match client.get_pull_request_by_id(pr_id).await {
        Ok(pr) => pr,
        Err(e) => {
            error!("Failed to fetch PR #{}: {}", pr_id, e);
            println!("Could not fetch that PR. Please check the number and try again.");
            return Ok(None);
        }
    };
    
    // Get reviewers from the PR
    let reviewers = match client.get_reviewers(&pr).await {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to fetch reviewers for PR #{}: {}", pr_id, e);
            println!("Could not fetch reviewers for that PR.");
            return Ok(None);
        }
    };
    
    if reviewers.is_empty() {
        println!("No reviewers found for this PR.");
        return Ok(None);
    }
    
    println!("\nAvailable reviewers:");
    for (i, reviewer) in reviewers.iter().enumerate() {
        println!("{}: {} (ID: {})", i + 1, reviewer.display_name, reviewer.id);
    }
    
    print!("\nSelect your reviewer number (or 0 to cancel): ");
    io::stdout().flush()?;
    
    let mut selection = String::new();
    io::stdin().read_line(&mut selection)?;
    
    let choice = selection.trim().parse::<usize>()
        .context("Invalid selection. Please enter a valid number.")?;
        
    if choice == 0 || choice > reviewers.len() {
        println!("Selection canceled or invalid.");
        return Ok(None);
    }
    
    let selected_reviewer = &reviewers[choice - 1];
    let reviewer_id = selected_reviewer.id.clone();
    
    println!("Selected reviewer: {} (ID: {})", selected_reviewer.display_name, reviewer_id);
    
    // Save the reviewer ID to the config file
    let mut config = AppConfig::from_file(config_path)?;
    config.reviewer_id = Some(reviewer_id.clone());
    config.save_to_file(config_path)?;
    
    println!("Reviewer ID saved to config file.");
    
    Ok(Some(reviewer_id))
}

async fn check_and_approve_prs(client: &AzureDevOpsClient, config: &AppConfig) -> Result<()> {
    // Check if reviewer ID is configured
    let reviewer_id = match &config.reviewer_id {
        Some(id) => id,
        None => {
            error!("No reviewer ID configured. Cannot approve PRs.");
            return Ok(());
        }
    };

    // Get active pull requests
    let prs = client.get_active_pull_requests().await?;
    
    if prs.is_empty() {
        info!("No active pull requests found");
        return Ok(());
    }
    
    let mut new_prs = Vec::new();
    
    // Lock the mutex to safely access the HashSet of seen PRs
    let mut seen_prs = SEEN_PRS.lock().await;
    for pr in &prs {
        if !seen_prs.contains(&pr.pull_request_id) {
            new_prs.push(pr);
            seen_prs.insert(pr.pull_request_id);
        }
    }
    // Mutex is automatically unlocked when seen_prs goes out of scope
    
    if new_prs.is_empty() {
        info!("No new pull requests found");
        return Ok(());
    }
    
    info!("Found {} new pull requests", new_prs.len());
    
    let watched_prs: Vec<_> = new_prs.iter()
        .filter(|&&pr| {
            // Check if user is in watched list
            let is_watched_user = config.watched_users.contains(&pr.created_by.display_name);
            
            // Parse the PR creation date
            if let Ok(pr_creation_date) = DateTime::parse_from_rfc3339(&pr.creation_date) {
                let pr_creation_utc = pr_creation_date.with_timezone(&Utc);
                
                // Only include PRs created after the program started
                if pr_creation_utc < *PROGRAM_START_TIME {
                    info!("Skipping PR #{} from {} - created before program start", 
                          pr.pull_request_id, pr.created_by.display_name);
                    return false;
                }
                
                return is_watched_user;
            } else {
                // If we can't parse the date, log a warning but still include the PR if it's from a watched user
                warn!("Could not parse creation date for PR #{}", pr.pull_request_id);
                return is_watched_user;
            }
        })
        .collect();
    
    if !watched_prs.is_empty() {
        info!("Found {} PRs from watched users created after program start", watched_prs.len());
    } else {
        info!("No PRs from watched users found in this poll that were created after program start");
        return Ok(());
    }
    
    // Process PRs from watched users
    for &pr in &watched_prs {
        info!("🔍 Processing PR #{} from watched user {} - '{}'", 
            pr.pull_request_id, pr.created_by.display_name, pr.title);
        
        // Check if we've already approved this PR using our reviewer ID
        match client.check_approval_status(pr, reviewer_id).await {
            Ok(already_approved) => {
                if already_approved {
                    info!("✓ PR #{} is already approved", pr.pull_request_id);
                    continue;
                } else {
                    info!("PR #{} needs approval, will approve now...", pr.pull_request_id);
                }
            },
            Err(e) => {
                warn!("⚠ Failed to check approval status for PR #{}: {}", pr.pull_request_id, e);
                info!("Will attempt to approve PR #{} anyway", pr.pull_request_id);
            }
        }
        
        // Try to approve the PR using our reviewer ID
        match client.approve_pull_request(pr, reviewer_id).await {
            Ok(_) => {
                info!("✅ Successfully approved PR #{} from {}", 
                    pr.pull_request_id, pr.created_by.display_name);
                info!("Approval timestamp: {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));
            }
            Err(e) => {
                error!("❌ Failed to approve PR #{}: {}", pr.pull_request_id, e);
            }
        }
    }
    
    Ok(())
}
