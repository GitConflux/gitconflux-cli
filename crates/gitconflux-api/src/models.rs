//! Response and request shapes mirroring the `GitConflux` API (camelCase JSON).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliLoginRequest {
    pub id: String,
    pub code: String,
    pub expires_in: u64,
    pub interval: u64,
}

impl std::fmt::Debug for CliLoginRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CliLoginRequest(<redacted>)")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub visibility: String,
    pub owner_id: String,
    pub owner_handle: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub key: Option<String>,
    pub title: String,
    pub description: String,
    pub priority: i64,
    pub labels: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub status: String,
    pub claimed_by: Option<String>,
    pub lease_expires_at: Option<i64>,
    pub proposal_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    pub kind: String,
    pub capabilities: Vec<String>,
    pub last_seen_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeProposal {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub status: String,
    pub metadata: Value,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalEvent {
    pub id: String,
    pub proposal_id: String,
    pub kind: String,
    pub detail: Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeProposal {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub reporter_id: Option<String>,
    pub agent_id: Option<String>,
    pub decision_rationale: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<i64>,
    pub task_id: Option<String>,
    pub duplicate_of: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub user_id: String,
    pub email: Option<String>,
    pub handle: String,
    pub joined_at: i64,
    pub handle_changed_at: Option<i64>,
    pub next_handle_change_at: Option<i64>,
    pub profile: Value,
}

/// Token metadata; the secret is never returned after creation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub catalog: Value,
    pub revision: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub hash: String,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProject<'a> {
    pub slug: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub visibility: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub priority: u8,
    pub labels: &'a [String],
    pub acceptance_criteria: &'a [String],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAgent<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub capabilities: &'a [String],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewIntake<'a> {
    pub kind: &'a str,
    pub title: &'a str,
    pub description: &'a str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubmitResult {
    pub proposal: ChangeProposal,
    pub checks: Vec<CheckResult>,
}

/// Short-lived Git credentials; the token is redacted from `Debug` output.
pub struct GitCredentials {
    pub remote: String,
    pub default_branch: Option<String>,
    pub token: gitconflux_core::Token,
    pub expires_at: i64,
    pub scope: String,
}

impl std::fmt::Debug for GitCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitCredentials")
            .field("remote", &self.remote)
            .field("scope", &self.scope)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEntry {
    pub proposal_id: String,
    pub status: String,
    pub files: Vec<String>,
}

/// Advisory Tier 2 merge preview; not a test result and not authorization to promote.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub base_commit: Option<String>,
    pub matrix_commit: Option<String>,
    pub proposals: Vec<PreviewEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyncResult {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PublishResult {
    pub status: String,
    pub commit: Option<String>,
    pub imported: Value,
}

/// A person or agent who can read and post on a project board.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardMember {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub name: String,
    /// What to type after `@` to tag this member.
    pub mention: String,
    pub owner_handle: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoardMention {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardMessage {
    pub id: String,
    pub author: BoardMember,
    pub body: String,
    pub mentions: Vec<BoardMention>,
    pub created_at: i64,
}

/// Newest messages first, plus everyone who can be tagged.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoardPage {
    pub members: Vec<BoardMember>,
    pub messages: Vec<BoardMessage>,
}
