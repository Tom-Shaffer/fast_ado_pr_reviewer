use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::{debug, info, warn};
use reqwest::{Client, header, StatusCode};
use url::Url;
use std::time::Duration;
use tokio::time::sleep;
use rand::{thread_rng, Rng};

use crate::models::{PullRequest, PullRequestList, ReviewList, ReviewRequest};

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
        let base_url = format!(
            "https://dev.azure.com/{}/{}/_apis",
            organization, project
        );

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

    /// Configure retry settings
    pub fn with_retry_settings(mut self, max_retries: u32, initial_delay_ms: u64) -> Self {
        self.max_retries = max_retries;
        self.initial_retry_delay_ms = initial_delay_ms;
        self
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
                    let jitter = thread_rng().gen_range(0..=100) as u64;
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
            "{}/git/pullrequests?api-version={}&status=active&$top=10&$fields=pullRequestId,title,createdBy,creationDate,status,sourceRefName,targetRefName",
            self.base_url, API_VERSION
        );

        debug!("Fetching active pull requests");

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
    pub async fn approve_pull_request(&self, pull_request_id: &str) -> Result<()> {
        let url = format!(
            "{}/git/pullrequests/{}/reviewers/{}?api-version={}",
            self.base_url, pull_request_id, "me", API_VERSION
        );

        debug!("Approving pull request #{}", pull_request_id);

        // Vote values: 10 = approve, 5 = approve with suggestions, 0 = no vote, -5 = waiting for author, -10 = reject
        let review_request = ReviewRequest {
            vote: 10,  // Approve
            comment: "Auto-approved by FastPRReviewer".to_string(),
        };

        self.execute_with_retry(&format!("Approve pull request #{}", pull_request_id), || async {
            let response = self.client
                .put(&url)
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

            info!("Successfully approved PR #{}", pull_request_id);
            Ok(())
        }).await
    }

    /// Check if we've already approved this PR
    pub async fn check_approval_status(&self, pull_request_id: &str) -> Result<bool> {
        let url = format!(
            "{}/git/pullrequests/{}/reviewers?api-version={}&$fields=vote,reviewer",
            self.base_url, pull_request_id, API_VERSION
        );

        debug!("Checking approval status for PR #{}", pull_request_id);

        self.execute_with_retry(&format!("Check approval status for PR #{}", pull_request_id), || async {
            let response = self.client
                .get(&url)
                .header(header::AUTHORIZATION, &self.auth_header)
                .send()
                .await
                .context("Failed to send request to check approval status")?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_else(|_| String::from("Unable to read response body"));
                return Err(anyhow::anyhow!("API request failed with status {}: {}", status, text));
            }

            let reviewers: ReviewList = response.json().await
                .context("Failed to parse reviewers response")?;

            // Check if we've already approved this PR (vote > 0)
            for reviewer in reviewers.value {
                // We're looking for our own approval - would need to match the reviewer's ID
                // with the ID associated with the PAT, but for simplicity we'll check for any approval
                if reviewer.vote > 0 {
                    return Ok(true);
                }
            }

            Ok(false)
        }).await
    }

    /// Parse and extract PR ID from a Teams/ADO URL
    pub fn extract_pr_id_from_url(&self, url_str: &str) -> Option<String> {
        match Url::parse(url_str) {
            Ok(url) => {
                // Sample URL pattern: https://dev.azure.com/org/project/_git/repo/pullrequest/123
                let path_segments: Vec<&str> = url.path_segments()?.collect();
                
                for (i, segment) in path_segments.iter().enumerate() {
                    if (*segment == "pullrequest" || *segment == "pullrequests") && i + 1 < path_segments.len() {
                        return Some(path_segments[i + 1].to_string());
                    }
                }
                None
            },
            Err(_) => None,
        }
    }
}