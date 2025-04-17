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
    pub status: String,
    #[serde(rename = "sourceRefName")]
    pub source_ref_name: String,
    #[serde(rename = "targetRefName")]
    pub target_ref_name: String,
}

#[derive(Debug, Deserialize)]
pub struct IdentityRef {
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub id: String,
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
pub struct ReviewResult {
    pub vote: i32,
    pub reviewer: IdentityRef,
}

#[derive(Debug, Deserialize)]
pub struct ReviewList {
    pub value: Vec<ReviewResult>,
}