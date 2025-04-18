use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PullRequest {
    #[serde(rename = "pullRequestId")]
    pub pull_request_id: i32,
    pub title: String,
    #[serde(rename = "createdBy")]
    pub created_by: IdentityRef,
    #[serde(rename = "creationDate")]
    pub creation_date: String,
    #[serde(rename = "targetRefName")]
    #[allow(dead_code)]
    pub target_branch: Option<String>,
    // Add repository information
    pub repository: Repository,
}

// Add Repository struct to store repository information
#[derive(Debug, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct IdentityRef {
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestList {
    pub value: Vec<PullRequest>,
}

#[derive(Debug, Serialize)]
pub struct ReviewRequest {
    pub vote: i32,
    pub comment: String,
}

#[derive(Debug, Deserialize)]
pub struct Reviewer {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct ReviewerList {
    pub value: Vec<Reviewer>,
}