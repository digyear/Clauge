use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Project issue scan — used by `workspace_scan_project_issues` to fetch
// open issues from the workspace's bound project (GitHub via `gh`,
// GitLab via `glab`) so the kanban can pre-populate cards.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIssue {
    pub external_id: String,
    pub title: String,
    pub body: String,
    pub url: String,
    /// 'github' | 'gitlab' — drives the icon shown on the imported card.
    pub source: String,
    pub labels: Vec<String>,
}

/// Result of a project-issue scan. Each variant maps 1:1 to a UI banner
/// state with its own action button (install tool, run auth, retry, …).
/// Frontend matches on `kind` and renders accordingly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProjectScanResult {
    #[serde(rename_all = "camelCase")]
    Success {
        issues: Vec<ProjectIssue>,
        remote: String,
        source: String,
    },
    NotGitRepo,
    NoRemote,
    #[serde(rename_all = "camelCase")]
    UnsupportedRemote { url: String },
    #[serde(rename_all = "camelCase")]
    ToolNotInstalled { tool: String, install_url: String },
    #[serde(rename_all = "camelCase")]
    NotAuthenticated { tool: String, login_command: String },
    #[serde(rename_all = "camelCase")]
    ApiError { message: String },
}


#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub project_path: Option<String>,
    pub project_name: Option<String>,
    pub color: Option<String>,
    pub created_at: String,
    pub created_by: String,
    pub updated_at: String,
    pub updated_by: String,
    /// Workspace-level GitHub/GitLab URL. Used as the agent's default
    /// remote when no per-board override is set. Migration 12 added.
    pub repo_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceNote {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub content: String,
    /// JSON-encoded `string[]`. Kept as a string at the SQL boundary so
    /// FromRow stays trivial; frontends parse on receive.
    pub tags: String,
    pub linked_session_id: Option<String>,
    pub created_at: String,
    pub created_by: String,
    pub updated_at: String,
    pub updated_by: String,
    /// `1` = blocked from agent edits; UI is free to edit. Migration
    /// 12 added. Tools that mutate must check this and return an
    /// error explaining the row is frozen.
    pub frozen: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBoard {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    /// `'manual' | 'github_issues'` etc. Currently always `manual` —
    /// non-manual sources land with the v1.5 issue-sync work.
    pub source: String,
    pub source_config: Option<String>,
    pub position: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBoardColumn {
    pub id: String,
    pub board_id: String,
    pub name: String,
    pub color: Option<String>,
    pub position: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCardComment {
    pub id: String,
    pub card_id: String,
    pub actor: String,
    pub body: String,
    /// Reserved for threaded replies; always None in v1. Storage is
    /// `parent_id TEXT` with a self-referential FK in migration 13.
    pub parent_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBoardCard {
    pub id: String,
    pub column_id: String,
    pub title: String,
    pub description: String,
    pub priority: Option<String>,
    pub tags: String,
    pub position: i32,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub linked_session_id: Option<String>,
    /// `1` when an agent moved this card into a Review column. Surfaced
    /// as a "Pending review" badge in the UI; user clears by approving
    /// (move to Done) or requesting changes (move elsewhere).
    pub review_pending: i32,
    pub review_checklist: Option<String>,
    pub created_at: String,
    pub created_by: String,
    pub updated_at: String,
    pub updated_by: String,
    /// Same semantics as WorkspaceNote.frozen — agent mutations
    /// blocked, UI edits allowed. Migration 12 added.
    pub frozen: i32,
}
