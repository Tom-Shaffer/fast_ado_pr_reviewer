use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::{debug, info, warn};
use reqwest::{Client, header, StatusCode};
use std::time::Duration;
use tokio::time::sleep;
use rand::Rng;

use crate::models::{PullRequest, PullRequestList, ReviewRequest, Reviewer, ReviewerList};

/// Azure DevOps API client
pub struct AzureDevOpsClient {
    client: Client,
    base_url: String,
    auth_header: String,
    max_retries: u32,
    initial_retry_delay_ms: u64,
}

const API_VERSION: &str = "7.1";

impl AzureDevOpsClient {
    pub fn new(organization: &str, project: &str, pat: &str) -> Self {
        // Modified to handle custom URL structures
        // The URL structure from the error logs suggests your organization might be using a
        // custom domain or on-premise Azure DevOps Server
        let base_url = if organization.contains(".") {
            // Custom domain approach
            format!("https://{}", organization)
        } else {
            // Standard Azure DevOps Services
            format!("https://dev.azure.com/{}/{}", organization, project)
        };

        // Log the base URL for debugging
        info!("Using ADO base URL: {}", base_url);
        info!("Organization: {}, Project: {}", organization, project);

        // Create auth header using PAT (Personal Access Token)
        let auth_token = general_purpose::STANDARD.encode(format!(":{}", pat));
        let auth_header = format!("Basic {}", auth_token);

        // Create HTTP client with default headers
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        // Explicitly request JSON responses
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            auth_header,
            max_retries: 5,  // Default max retries
            initial_retry_delay_ms: 1000,  // Start with 1 second delay
        }
    }

    /// Helper method to execute a request with automatic retry and exponential backoff
    async fn execute_with_retry<T, F, Fut>(&self, operation: &str, f: F) -> Result<T> 
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0;
        let mut delay = self.initial_retry_delay_ms;
        
        loop {
            attempt += 1;
            match f().await {
                Ok(response) => {
                    return Ok(response);
                }
                Err(e) => {
                    // Check if we've hit the max retries
                    if attempt > self.max_retries {
                        return Err(anyhow::anyhow!("Operation '{}' failed after {} attempts: {}", 
                            operation, self.max_retries, e));
                    }
                    
                    // Check if the error is retryable (rate limiting, server errors)
                    let should_retry = if let Some(reqwest_err) = e.downcast_ref::<reqwest::Error>() {
                        if let Some(status) = reqwest_err.status() {
                            match status {
                                // Rate limiting
                                StatusCode::TOO_MANY_REQUESTS => true,
                                // Server errors (5xx) are usually transient
                                s if s.is_server_error() => true,
                                // Other client errors (4xx) are usually not retryable (except 429)
                                _ => false,
                            }
                        } else {
                            // Network errors (timeout, connection reset) are retryable
                            reqwest_err.is_timeout() || reqwest_err.is_connect()
                        }
                    } else {
                        // For non-reqwest errors, we'll retry conservatively
                        false
                    };
                    
                    if !should_retry {
                        return Err(e);
                    }
                    
                    // Add jitter to prevent all clients retrying at the same time
                    let mut rng = rand::rng();
                    let jitter = rng.random_range(1..=100) as u64;
                    let backoff_delay = delay + jitter;
                    
                    warn!("{} failed (attempt {}/{}), retrying in {}ms", 
                        operation, attempt, self.max_retries, backoff_delay);
                    
                    // Wait before retrying
                    sleep(Duration::from_millis(backoff_delay)).await;
                    
                    // Exponential backoff - double the delay for next attempt
                    delay = delay.saturating_mul(2);
                }
            }
        }
    }

    /// Get all active pull requests
    pub async fn get_active_pull_requests(&self) -> Result<Vec<PullRequest>> {
        let url = format!(
            "{}/_apis/git/pullrequests?api-version={}&status=active&$top=10&$orderby=creationDate desc",
            self.base_url, API_VERSION
        );

        debug!("Fetching active pull requests");
        info!("Request URL: {}", url);

        self.execute_with_retry("Get active pull requests", || async {
            let response = self.client
                .get(&url)
                .header(header::AUTHORIZATION, &self.auth_header)
                .send()
                .await
                .context("Failed to send request to Azure DevOps API")?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_else(|_| String::from("Unable to read response body"));
                return Err(anyhow::anyhow!("API request failed with status {}: {}", status, text));
            }

            let pr_list: PullRequestList = response.json().await
                .context("Failed to parse pull request response")?;

            Ok(pr_list.value)
        }).await
    }

    /// Approve a pull request
    pub async fn approve_pull_request(&self, pull_request: &PullRequest, reviewer_id: &str) -> Result<()> {
        // Submit the vote using the provided reviewer ID
        let vote_url = format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/reviewers/{}?api-version={}",
            self.base_url, pull_request.repository.id, pull_request.pull_request_id, 
            reviewer_id, API_VERSION
        );

        debug!("Approving pull request #{} in repository {}", pull_request.pull_request_id, pull_request.repository.name);
        info!("Approval URL: {}", vote_url);

        // Vote values: 10 = approve, 5 = approve with suggestions, 0 = no vote, -5 = waiting for author, -10 = reject
        let review_request = ReviewRequest {
            vote: 10,  // Approve
            comment: "Auto-approved by FastPRReviewer".to_string(),
        };

        self.execute_with_retry(&format!("Approve pull request #{}", pull_request.pull_request_id), || async {
            let response = self.client
                .put(&vote_url)
                .header(header::AUTHORIZATION, &self.auth_header)
                .json(&review_request)
                .send()
                .await
                .context("Failed to send approval request")?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_else(|_| String::from("Unable to read response body"));
                return Err(anyhow::anyhow!("API request failed with status {}: {}", status, text));
            }

            info!("Successfully approved PR #{}", pull_request.pull_request_id);
            Ok(())
        }).await
    }

    /// Check if we've already approved this PR
    pub async fn check_approval_status(&self, pull_request: &PullRequest, reviewer_id: &str) -> Result<bool> {
        // Check if this reviewer ID has already approved the PR
        let url = format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/reviewers/{}?api-version={}",
            self.base_url, pull_request.repository.id, pull_request.pull_request_id, reviewer_id, API_VERSION
        );

        debug!("Checking approval status for PR #{} in repository {}", 
            pull_request.pull_request_id, pull_request.repository.name);
        info!("Check status URL: {}", url);

        self.execute_with_retry(&format!("Check approval status for PR #{}", pull_request.pull_request_id), || async {
            let response = self.client
                .get(&url)
                .header(header::AUTHORIZATION, &self.auth_header)
                .send()
                .await
                .context("Failed to send request to check approval status")?;
            
            if response.status() == StatusCode::NOT_FOUND {
                // If the reviewer doesn't exist, it means we haven't reviewed yet
                return Ok(false);
            } else if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_else(|_| String::from("Unable to read response body"));
                return Err(anyhow::anyhow!("API request failed with status {}: {}", status, text));
            }

            // Parse the individual reviewer response
            let reviewer: serde_json::Value = response.json().await
                .context("Failed to parse reviewer response")?;
            
            // Check if the vote is positive (approval)
            if let Some(vote) = reviewer["vote"].as_i64() {
                if vote > 0 {
                    debug!("PR #{} is already approved by the reviewer", pull_request.pull_request_id);
                    return Ok(true);
                }
            }

            debug!("PR #{} is not approved by the reviewer", pull_request.pull_request_id);
            Ok(false)
        }).await
    }

    /// Get all reviewers for a pull request
    pub async fn get_reviewers(&self, pull_request: &PullRequest) -> Result<Vec<Reviewer>> {
        let url = format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/reviewers?api-version={}",
            self.base_url, pull_request.repository.id, pull_request.pull_request_id, API_VERSION
        );

        debug!("Fetching reviewers for PR #{} in repository {}", pull_request.pull_request_id, pull_request.repository.name);
        info!("Reviewers URL: {}", url);

        self.execute_with_retry(&format!("Get reviewers for PR #{}", pull_request.pull_request_id), || async {
            let response = self.client
                .get(&url)
                .header(header::AUTHORIZATION, &self.auth_header)
                .send()
                .await
                .context("Failed to send request to get reviewers")?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_else(|_| String::from("Unable to read response body"));
                return Err(anyhow::anyhow!("API request failed with status {}: {}", status, text));
            }

            let reviewer_list: ReviewerList = response.json().await
                .context("Failed to parse reviewers response")?;

            Ok(reviewer_list.value)
        }).await
    }

    /// Get a specific pull request by ID
    pub async fn get_pull_request_by_id(&self, pull_request_id: i32) -> Result<PullRequest> {
        // Because we don't know the repository ID in advance, we need a URL that doesn't require it
        let url = format!(
            "{}/_apis/git/pullrequests/{}?api-version={}",
            self.base_url, pull_request_id, API_VERSION
        );

        debug!("Fetching pull request #{}", pull_request_id);
        info!("Request URL: {}", url);

        self.execute_with_retry(&format!("Get pull request #{}", pull_request_id), || async {
            let response = self.client
                .get(&url)
                .header(header::AUTHORIZATION, &self.auth_header)
                .send()
                .await
                .context("Failed to send request to get pull request")?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_else(|_| String::from("Unable to read response body"));
                return Err(anyhow::anyhow!("API request failed with status {}: {}", status, text));
            }

            let pull_request: PullRequest = response.json().await
                .context("Failed to parse pull request response")?;

            Ok(pull_request)
        }).await
    }
}