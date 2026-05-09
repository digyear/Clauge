use sqlx::SqlitePool;
use tauri::State;

use crate::modes::workspace::models::{
    ProjectIssue, ProjectScanResult, Workspace, WorkspaceBoard, WorkspaceBoardCard,
    WorkspaceBoardColumn, WorkspaceNote,
};
use crate::shared::repos::workspaces as repo;

fn project_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

fn is_git_repo(path: &str) -> bool {
    std::path::Path::new(path).join(".git").exists()
}

/// Discovered subproject — used when the workspace folder isn't a git
/// repo itself but contains git repos as immediate children. The
/// caller creates one board per entry. Single-layer scan only — no
/// recursion.
struct SubProject {
    name: String,
    path: String,
}

fn scan_immediate_subprojects(parent: &str) -> Vec<SubProject> {
    let mut out: Vec<SubProject> = Vec::new();
    let entries = match std::fs::read_dir(parent) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Skip dotfiles + common noise (.clauge-worktrees, node_modules, target, …).
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if file_name.starts_with('.') || file_name == "node_modules" || file_name == "target" {
            continue;
        }
        if !path.join(".git").exists() {
            continue;
        }
        out.push(SubProject {
            name: file_name,
            path: path.to_string_lossy().to_string(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Default board column shape applied to every board on creation.
/// Five columns; "Review" is the safety gate — agents move work here,
/// the user (or a future review-agent) clears it to Done. Names + colors
/// match the workspace prototype's palette.
const DEFAULT_COLUMNS: &[(&str, &str)] = &[
    ("Backlog", "#5b6776"),
    ("Todo", "#6aa9ff"),
    ("Doing", "#f4c150"),
    ("Review", "#a78bfa"),
    ("Done", "#2ee08a"),
];

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Insert a board with the standard 5-column layout. Used by
/// `workspace_create` for both single-board and per-subproject cases,
/// and by `workspace_board_create` for user-driven board creation.
async fn create_default_board(
    pool: &sqlx::SqlitePool,
    workspace_id: &str,
    name: &str,
    source_config: Option<&str>,
    position: i32,
    now: &str,
) -> Result<String, String> {
    let board_id = new_id();
    repo::insert_board(
        pool,
        &board_id,
        workspace_id,
        name,
        "manual",
        source_config,
        position,
        now,
    )
    .await
    .map_err(|e| e.to_string())?;
    for (idx, (col_name, col_color)) in DEFAULT_COLUMNS.iter().enumerate() {
        repo::insert_column(
            pool,
            &new_id(),
            &board_id,
            col_name,
            Some(col_color),
            idx as i32,
            now,
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(board_id)
}

// ---------------------------------------------------------------------------
// workspaces
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn workspace_list(pool: State<'_, SqlitePool>) -> Result<Vec<Workspace>, String> {
    repo::list_workspaces(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_get(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<Workspace, String> {
    repo::get_workspace_by_id(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_create(
    pool: State<'_, SqlitePool>,
    name: String,
    project_path: Option<String>,
    color: Option<String>,
    actor: String,
) -> Result<Workspace, String> {
    let id = new_id();
    let now = now_rfc3339();
    let project_name = project_path.as_deref().map(project_name_from_path);

    repo::insert_workspace(
        pool.inner(),
        &id,
        &name,
        project_path.as_deref(),
        project_name.as_deref(),
        color.as_deref(),
        &actor,
        &now,
    )
    .await
    .map_err(|e| e.to_string())?;

    // Project-bound workspaces auto-get one or more default boards so
    // the user has something to look at on first open. Three cases:
    //   1. project_path is itself a git repo  → 1 board ("Tasks") with
    //      no source override; sync uses the workspace project_path.
    //   2. project_path is a folder containing nested git repos as
    //      immediate children → 1 board per subproject, named after
    //      the subfolder, with source_config={project_path:<sub>} so
    //      each board syncs against its own remote.
    //   3. project_path isn't a git repo and has no nested ones → 1
    //      board ("Tasks") with no source override (user can set a
    //      project URL later from the board's overflow menu).
    // Standalone workspaces (no project_path) skip auto-board entirely.
    if let Some(path) = project_path.as_deref() {
        let subprojects = if is_git_repo(path) {
            Vec::new()
        } else {
            scan_immediate_subprojects(path)
        };

        if subprojects.is_empty() {
            create_default_board(pool.inner(), &id, "Tasks", None, 0, &now).await?;
        } else {
            for (idx, sub) in subprojects.iter().enumerate() {
                // Per-board override: store the subproject's absolute
                // path inside source_config JSON so the sync banner
                // and the editor's "Set project" UI can read it back.
                let cfg = serde_json::json!({ "project_path": sub.path }).to_string();
                create_default_board(
                    pool.inner(),
                    &id,
                    &sub.name,
                    Some(cfg.as_str()),
                    idx as i32,
                    &now,
                )
                .await?;
            }
        }
    }

    repo::get_workspace_by_id(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_update(
    pool: State<'_, SqlitePool>,
    id: String,
    name: String,
    project_path: Option<String>,
    color: Option<String>,
    actor: String,
) -> Result<(), String> {
    let now = now_rfc3339();
    let project_name = project_path.as_deref().map(project_name_from_path);
    repo::update_workspace(
        pool.inner(),
        &id,
        &name,
        project_path.as_deref(),
        project_name.as_deref(),
        color.as_deref(),
        &actor,
        &now,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_delete(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    repo::delete_workspace(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// notes
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn workspace_note_list(
    pool: State<'_, SqlitePool>,
    workspace_id: String,
) -> Result<Vec<WorkspaceNote>, String> {
    repo::list_notes_in_workspace(pool.inner(), &workspace_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_note_get(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<WorkspaceNote, String> {
    repo::get_note_by_id(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_note_create(
    pool: State<'_, SqlitePool>,
    workspace_id: String,
    title: String,
    content: Option<String>,
    tags: Option<Vec<String>>,
    linked_session_id: Option<String>,
    actor: String,
) -> Result<WorkspaceNote, String> {
    let id = new_id();
    let now = now_rfc3339();
    let tags_json = serde_json::to_string(&tags.unwrap_or_default()).unwrap_or_else(|_| "[]".to_string());
    repo::insert_note(
        pool.inner(),
        &id,
        &workspace_id,
        &title,
        content.as_deref().unwrap_or(""),
        &tags_json,
        linked_session_id.as_deref(),
        &actor,
        &now,
    )
    .await
    .map_err(|e| e.to_string())?;
    repo::get_note_by_id(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_note_update(
    pool: State<'_, SqlitePool>,
    id: String,
    title: String,
    content: String,
    tags: Vec<String>,
    linked_session_id: Option<String>,
    actor: String,
) -> Result<(), String> {
    let now = now_rfc3339();
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
    repo::update_note(
        pool.inner(),
        &id,
        &title,
        &content,
        &tags_json,
        linked_session_id.as_deref(),
        &actor,
        &now,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_note_delete(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    repo::delete_note(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// boards
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn workspace_board_list(
    pool: State<'_, SqlitePool>,
    workspace_id: String,
) -> Result<Vec<WorkspaceBoard>, String> {
    repo::list_boards_in_workspace(pool.inner(), &workspace_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_board_get(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<WorkspaceBoard, String> {
    repo::get_board_by_id(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_board_create(
    pool: State<'_, SqlitePool>,
    workspace_id: String,
    name: String,
) -> Result<WorkspaceBoard, String> {
    let now = now_rfc3339();
    let existing = repo::list_boards_in_workspace(pool.inner(), &workspace_id)
        .await
        .map_err(|e| e.to_string())?;
    let position = existing.len() as i32;
    let board_id =
        create_default_board(pool.inner(), &workspace_id, &name, None, position, &now).await?;
    repo::get_board_by_id(pool.inner(), &board_id)
        .await
        .map_err(|e| e.to_string())
}

/// Set or clear the per-board project override. Either or both of
/// `project_path` and `project_url` may be set:
///   • path  — local clone; sync runs from cwd, picks up the remote.
///   • url   — direct GitHub/GitLab URL; sync uses `--repo owner/repo`.
/// Both empty → override cleared, board falls back to the parent
/// workspace's project_path.
#[tauri::command]
pub async fn workspace_board_set_project(
    pool: State<'_, SqlitePool>,
    id: String,
    project_path: Option<String>,
    project_url: Option<String>,
) -> Result<(), String> {
    let now = now_rfc3339();
    let path = project_path.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let url = project_url.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let cfg = match (path, url) {
        (None, None) => None,
        _ => {
            let mut obj = serde_json::Map::new();
            if let Some(p) = path { obj.insert("project_path".into(), serde_json::Value::String(p.into())); }
            if let Some(u) = url  { obj.insert("project_url".into(),  serde_json::Value::String(u.into())); }
            Some(serde_json::Value::Object(obj).to_string())
        }
    };
    repo::update_board_source_config(pool.inner(), &id, cfg.as_deref(), &now)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_board_rename(
    pool: State<'_, SqlitePool>,
    id: String,
    name: String,
) -> Result<(), String> {
    let now = now_rfc3339();
    repo::update_board_name(pool.inner(), &id, &name, &now)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_board_delete(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    repo::delete_board(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_column_list(
    pool: State<'_, SqlitePool>,
    board_id: String,
) -> Result<Vec<WorkspaceBoardColumn>, String> {
    repo::list_columns_in_board(pool.inner(), &board_id)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// cards
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn workspace_card_list(
    pool: State<'_, SqlitePool>,
    board_id: String,
) -> Result<Vec<WorkspaceBoardCard>, String> {
    repo::list_cards_in_board(pool.inner(), &board_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn workspace_card_create(
    pool: State<'_, SqlitePool>,
    column_id: String,
    title: String,
    description: Option<String>,
    priority: Option<String>,
    tags: Option<Vec<String>>,
    position: Option<i32>,
    external_id: Option<String>,
    external_url: Option<String>,
    linked_session_id: Option<String>,
    actor: String,
) -> Result<WorkspaceBoardCard, String> {
    let id = new_id();
    let now = now_rfc3339();
    let tags_json = serde_json::to_string(&tags.unwrap_or_default()).unwrap_or_else(|_| "[]".to_string());

    repo::insert_card(
        pool.inner(),
        &id,
        &column_id,
        &title,
        description.as_deref().unwrap_or(""),
        priority.as_deref(),
        &tags_json,
        position.unwrap_or(0),
        external_id.as_deref(),
        external_url.as_deref(),
        linked_session_id.as_deref(),
        &actor,
        &now,
    )
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query_as::<_, WorkspaceBoardCard>("SELECT * FROM workspace_board_cards WHERE id = ?")
        .bind(&id)
        .fetch_one(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn workspace_card_update(
    pool: State<'_, SqlitePool>,
    id: String,
    title: String,
    description: String,
    priority: Option<String>,
    tags: Vec<String>,
    review_checklist: Option<String>,
    actor: String,
) -> Result<(), String> {
    let now = now_rfc3339();
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
    repo::update_card(
        pool.inner(),
        &id,
        &title,
        &description,
        priority.as_deref(),
        &tags_json,
        review_checklist.as_deref(),
        &actor,
        &now,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Move a card to a column + position. The actor decides whether the
/// move triggers a "Pending review" flag: when an agent (anything other
/// than `user` / `user:*`) moves to a column whose name matches one of
/// the review-class names below, we set `review_pending = 1`. User moves
/// always clear the flag. Keeping this rule in Rust means the same
/// behaviour applies whether the move comes from the UI or from an MCP
/// tool call later.
#[tauri::command]
pub async fn workspace_card_move(
    pool: State<'_, SqlitePool>,
    id: String,
    column_id: String,
    position: i32,
    actor: String,
) -> Result<(), String> {
    let now = now_rfc3339();
    let is_user = actor == "user" || actor.starts_with("user:");

    let review_pending = if is_user {
        0
    } else {
        // Look up the destination column's name and decide if it counts
        // as a review column. "Review" is the canonical name for the
        // safety gate but the user can rename a column; matching by
        // case-insensitive substring keeps the behaviour intuitive.
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM workspace_board_columns WHERE id = ?",
        )
        .bind(&column_id)
        .fetch_optional(pool.inner())
        .await
        .map_err(|e| e.to_string())?;
        match row {
            Some((name,)) if name.to_lowercase().contains("review") => 1,
            _ => 0,
        }
    };

    repo::move_card(
        pool.inner(),
        &id,
        &column_id,
        position,
        review_pending,
        &actor,
        &now,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_card_clear_review(
    pool: State<'_, SqlitePool>,
    id: String,
    actor: String,
) -> Result<(), String> {
    let now = now_rfc3339();
    repo::clear_review_pending(pool.inner(), &id, &actor, &now)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_card_delete(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    repo::delete_card(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

/// Set or clear the agent session linked to a card. Drives the
/// "Send to @session" mention flow — only cards with a non-null
/// `linked_session_id` can be mentioned.
#[tauri::command]
pub async fn workspace_card_set_linked_session(
    pool: State<'_, SqlitePool>,
    id: String,
    session_id: Option<String>,
    actor: String,
) -> Result<(), String> {
    let now = now_rfc3339();
    repo::update_card_linked_session(pool.inner(), &id, session_id.as_deref(), &actor, &now)
        .await
        .map_err(|e| e.to_string())
}

/// Trigger the agent session linked to this card. Body becomes a user
/// comment on the card; the agent's response is appended as a follow-up
/// comment from the provider's slug ("claude", "codex", …).
#[tauri::command]
pub async fn workspace_card_mention_session(
    pool: State<'_, SqlitePool>,
    id: String,
    body: String,
    actor: String,
) -> Result<serde_json::Value, String> {
    super::mention::mention_card_session(pool.inner(), &id, &body, &actor).await
}

/// Push a local card to its workspace's bound GitHub/GitLab repo as a
/// real issue. Always user-initiated (button in the card editor) —
/// never automatic. Same code path as the `cards_push_to_repo` MCP
/// tool so behaviour stays uniform across UI + agent triggers.
#[tauri::command]
pub async fn workspace_card_push_to_repo(
    pool: State<'_, SqlitePool>,
    id: String,
    actor: String,
) -> Result<serde_json::Value, String> {
    super::push::push_card_to_repo(pool.inner(), &id, &actor).await
}

/// Add a comment to a card. Each comment is its own row in
/// `workspace_card_comments`; the helper also bumps `card.updated_at`
/// + `updated_by` so the inbox and per-card unread tracking continue
/// to work without query changes.
#[tauri::command]
pub async fn workspace_card_add_comment(
    pool: State<'_, SqlitePool>,
    id: String,
    body: String,
    actor: String,
) -> Result<crate::modes::workspace::models::WorkspaceCardComment, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("Comment body is empty".into());
    }
    let now = now_rfc3339();
    let comment_id = uuid::Uuid::new_v4().to_string();
    repo::insert_card_comment(
        pool.inner(),
        &comment_id,
        &id,
        &actor,
        trimmed,
        None,
        &now,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(crate::modes::workspace::models::WorkspaceCardComment {
        id: comment_id,
        card_id: id,
        actor,
        body: trimmed.to_string(),
        parent_id: None,
        created_at: now,
    })
}

/// List all comments on a card, oldest first. Drives the Thread tab
/// in CardEditorDrawer.
#[tauri::command]
pub async fn workspace_card_comment_list(
    pool: State<'_, SqlitePool>,
    card_id: String,
) -> Result<Vec<crate::modes::workspace::models::WorkspaceCardComment>, String> {
    repo::list_card_comments(pool.inner(), &card_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_card_comment_delete(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    repo::delete_card_comment(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_inbox_list(
    pool: State<'_, SqlitePool>,
    limit: Option<i32>,
) -> Result<Vec<crate::shared::repos::workspaces::InboxItem>, String> {
    repo::list_inbox(pool.inner(), limit.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// MCP server lifecycle. Backed by a global Tokio Mutex<Option<McpHandle>>
// in app state — simple, single-instance. start() returns the port +
// token; stop() drops the handle (which fires the oneshot shutdown).
// ---------------------------------------------------------------------------

use tokio::sync::Mutex as AsyncMutex;
use crate::modes::workspace::mcp::{self, McpHandle};

pub struct McpServerState(pub AsyncMutex<Option<McpHandle>>);

impl Default for McpServerState {
    fn default() -> Self { Self(AsyncMutex::new(None)) }
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub running: bool,
    pub port: Option<u16>,
}

#[tauri::command]
pub async fn workspace_mcp_status(
    server: State<'_, McpServerState>,
) -> Result<McpStatus, String> {
    let g = server.0.lock().await;
    Ok(match &*g {
        Some(h) => McpStatus { running: true, port: Some(h.port) },
        None => McpStatus { running: false, port: None },
    })
}

#[tauri::command]
pub async fn workspace_mcp_start(
    pool: State<'_, SqlitePool>,
    server: State<'_, McpServerState>,
    port: u16,
    token: String,
) -> Result<McpStatus, String> {
    let mut g = server.0.lock().await;
    if g.is_some() {
        // Already running — caller should stop first if they want to re-bind.
        return Ok(McpStatus { running: true, port: g.as_ref().map(|h| h.port) });
    }
    let handle = mcp::start(pool.inner().clone(), port, token).await?;
    let p = handle.port;
    *g = Some(handle);
    Ok(McpStatus { running: true, port: Some(p) })
}

#[tauri::command]
pub async fn workspace_mcp_stop(
    server: State<'_, McpServerState>,
) -> Result<McpStatus, String> {
    let mut g = server.0.lock().await;
    if let Some(mut h) = g.take() {
        if let Some(tx) = h.shutdown.take() {
            let _ = tx.send(());
        }
    }
    Ok(McpStatus { running: false, port: None })
}

// ---------------------------------------------------------------------------
// Per-agent registration of the MCP server. Each supported agent has
// its own config-file location and JSON shape; we dispatch on an
// `agent` slug so the API stays stable as new agents (codex, gemini,
// opencode, …) get MCP support. The MCP server itself is shared —
// every agent connects to the same `localhost:<port>/mcp` endpoint;
// only the on-disk registration differs.
// ---------------------------------------------------------------------------

const AGENT_CLAUDE_CODE: &str = "claude-code";
// Future agents — uncomment + implement when their MCP config format is known.
// const AGENT_CODEX: &str = "codex";
// const AGENT_GEMINI: &str = "gemini";
// const AGENT_OPENCODE: &str = "opencode";

#[tauri::command]
pub fn workspace_mcp_register(
    agent: Option<String>,
    port: u16,
    token: String,
) -> Result<(), String> {
    let agent = agent.unwrap_or_else(|| AGENT_CLAUDE_CODE.to_string());
    match agent.as_str() {
        AGENT_CLAUDE_CODE => register_claude_code(port, &token),
        // AGENT_CODEX => register_codex(port, &token),
        // AGENT_GEMINI => register_gemini(port, &token),
        // AGENT_OPENCODE => register_opencode(port, &token),
        other => Err(format!(
            "Unknown agent '{}'. Supported today: {}",
            other, AGENT_CLAUDE_CODE
        )),
    }
}

#[tauri::command]
pub fn workspace_mcp_unregister(agent: Option<String>) -> Result<(), String> {
    let agent = agent.unwrap_or_else(|| AGENT_CLAUDE_CODE.to_string());
    match agent.as_str() {
        AGENT_CLAUDE_CODE => unregister_claude_code(),
        // AGENT_CODEX => unregister_codex(),
        // AGENT_GEMINI => unregister_gemini(),
        // AGENT_OPENCODE => unregister_opencode(),
        other => Err(format!(
            "Unknown agent '{}'. Supported today: {}",
            other, AGENT_CLAUDE_CODE
        )),
    }
}

// ─── claude-code (~/.claude.json) ────────────────────────────────────
//
// Important: Claude Code reads `mcpServers` from `~/.claude.json` —
// the same file `claude mcp add --scope user` writes to. The
// adjacent `~/.claude/settings.json` is a policy/hook file and does
// NOT load MCP servers from it. Earlier versions of this code wrote
// to the wrong file, so Claude Code silently never saw the workspace
// server. Keep this comment as the trail in case anyone moves it.

fn claude_settings_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude.json"))
}

fn register_claude_code(port: u16, token: &str) -> Result<(), String> {
    let path = claude_settings_path().ok_or("home directory not found")?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut root: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    let map = root.as_object_mut().unwrap();
    let servers = map
        .entry("mcpServers".to_string())
        .or_insert(serde_json::json!({}));
    if !servers.is_object() {
        *servers = serde_json::json!({});
    }
    let s = servers.as_object_mut().unwrap();
    s.insert(
        "clauge-workspace".to_string(),
        serde_json::json!({
            "type": "http",
            "url": format!("http://localhost:{}/mcp", port),
            "headers": { "Authorization": format!("Bearer {}", token) }
        }),
    );
    let pretty = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    Ok(())
}

fn unregister_claude_code() -> Result<(), String> {
    let path = match claude_settings_path() {
        Some(p) => p,
        None => return Ok(()),
    };
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut root: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    if let Some(map) = root.as_object_mut() {
        if let Some(servers) = map.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
            servers.remove("clauge-workspace");
        }
    }
    let pretty = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())?;
    Ok(())
}

/// Generate a fresh random token. Caller is expected to persist it
/// via the settings store and pass it to start + register.
#[tauri::command]
pub fn workspace_mcp_new_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ---------------------------------------------------------------------------
// Project issue scan — supports GitHub via `gh` and GitLab via `glab`.
// Each failure mode maps to its own UI state; we never throw a generic
// error string when there's a structured one available.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn workspace_scan_project_issues(
    project_path: String,
) -> Result<ProjectScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || scan_project_issues_sync(&project_path))
        .await
        .map_err(|e| format!("scan thread error: {}", e))?
}

fn scan_project_issues_sync(project_path: &str) -> Result<ProjectScanResult, String> {
    use std::process::Command;

    // 1. Is it a git repo at all?
    let inside = Command::new("git")
        .args(["-C", project_path, "rev-parse", "--is-inside-work-tree"])
        .output();
    let inside_ok = matches!(&inside, Ok(o) if o.status.success());
    if !inside_ok {
        return Ok(ProjectScanResult::NotGitRepo);
    }

    // 2. Read the origin remote URL.
    let remote_out = Command::new("git")
        .args(["-C", project_path, "remote", "get-url", "origin"])
        .output()
        .map_err(|e| e.to_string())?;
    if !remote_out.status.success() {
        return Ok(ProjectScanResult::NoRemote);
    }
    let remote_url = String::from_utf8_lossy(&remote_out.stdout).trim().to_string();
    if remote_url.is_empty() {
        return Ok(ProjectScanResult::NoRemote);
    }

    // 3. Decide which CLI to use. We support github.com and any host
    //    whose URL contains "gitlab" (covers gitlab.com + self-hosted).
    let lower = remote_url.to_lowercase();
    let (tool, source, install_url, login_cmd, args): (&str, &str, &str, &str, Vec<&str>) =
        if lower.contains("github.com") {
            (
                "gh",
                "github",
                "https://cli.github.com/",
                "gh auth login",
                vec![
                    "issue",
                    "list",
                    "--state",
                    "open",
                    "--limit",
                    "100",
                    "--json",
                    "number,title,body,url,labels",
                ],
            )
        } else if lower.contains("gitlab") {
            (
                "glab",
                "gitlab",
                "https://gitlab.com/gitlab-org/cli",
                "glab auth login",
                vec!["issue", "list", "-F", "json", "--per-page", "100"],
            )
        } else {
            return Ok(ProjectScanResult::UnsupportedRemote { url: remote_url });
        };

    // 4. Is the CLI on PATH?
    let which = if cfg!(target_os = "windows") { "where" } else { "which" };
    let which_ok = Command::new(which)
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !which_ok {
        return Ok(ProjectScanResult::ToolNotInstalled {
            tool: tool.to_string(),
            install_url: install_url.to_string(),
        });
    }

    // 5. Run the issue list. cwd matters — both CLIs read repo context
    //    from the working directory.
    let out = Command::new(tool)
        .current_dir(project_path)
        .args(&args)
        .output()
        .map_err(|e| format!("{} failed to spawn: {}", tool, e))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
        // Heuristic: every CLI worth its salt prints "auth" or
        // "logged in" wording when the issue is credentials.
        if stderr.contains("auth") || stderr.contains("logged in") || stderr.contains("token") {
            return Ok(ProjectScanResult::NotAuthenticated {
                tool: tool.to_string(),
                login_command: login_cmd.to_string(),
            });
        }
        return Ok(ProjectScanResult::ApiError {
            message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let issues = match source {
        "github" => parse_github_issues(&stdout),
        "gitlab" => parse_gitlab_issues(&stdout),
        _ => Vec::new(),
    };
    Ok(ProjectScanResult::Success {
        issues,
        remote: remote_url,
        source: source.to_string(),
    })
}

/// URL-based scan — used when the board has a `project_url` but no
/// `project_path`. We don't need a local clone; both `gh` and `glab`
/// support a `--repo`/`-R owner/repo` flag. Failure variants match
/// the path-based scan so the banner UI is identical.
#[tauri::command]
pub async fn workspace_scan_project_issues_by_url(
    project_url: String,
) -> Result<ProjectScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || scan_project_url_sync(&project_url))
        .await
        .map_err(|e| format!("scan thread error: {}", e))?
}

fn scan_project_url_sync(url: &str) -> Result<ProjectScanResult, String> {
    use std::process::Command;

    // 1. Pick CLI + parse owner/repo from the URL.
    let lower = url.to_lowercase();
    let (tool, source, install_url, login_cmd, repo_arg, args): (
        &str,
        &str,
        &str,
        &str,
        Vec<String>,
        Vec<&str>,
    ) = if lower.contains("github.com") {
        let or = match parse_owner_repo(url) {
            Some(s) => s,
            None => return Ok(ProjectScanResult::UnsupportedRemote { url: url.to_string() }),
        };
        (
            "gh",
            "github",
            "https://cli.github.com/",
            "gh auth login",
            vec!["--repo".to_string(), or],
            vec!["issue", "list", "--state", "open", "--limit", "100", "--json", "number,title,body,url,labels"],
        )
    } else if lower.contains("gitlab") {
        let or = match parse_owner_repo(url) {
            Some(s) => s,
            None => return Ok(ProjectScanResult::UnsupportedRemote { url: url.to_string() }),
        };
        (
            "glab",
            "gitlab",
            "https://gitlab.com/gitlab-org/cli",
            "glab auth login",
            vec!["-R".to_string(), or],
            vec!["issue", "list", "-F", "json", "--per-page", "100"],
        )
    } else {
        return Ok(ProjectScanResult::UnsupportedRemote { url: url.to_string() });
    };

    // 2. CLI on PATH?
    let which = if cfg!(target_os = "windows") { "where" } else { "which" };
    let which_ok = Command::new(which)
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !which_ok {
        return Ok(ProjectScanResult::ToolNotInstalled {
            tool: tool.to_string(),
            install_url: install_url.to_string(),
        });
    }

    // 3. Run with --repo / -R prepended so cwd doesn't matter.
    let mut cmd = Command::new(tool);
    for a in &repo_arg { cmd.arg(a); }
    for a in &args { cmd.arg(a); }
    let out = cmd.output().map_err(|e| format!("{} failed to spawn: {}", tool, e))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
        if stderr.contains("auth") || stderr.contains("logged in") || stderr.contains("token") {
            return Ok(ProjectScanResult::NotAuthenticated {
                tool: tool.to_string(),
                login_command: login_cmd.to_string(),
            });
        }
        return Ok(ProjectScanResult::ApiError {
            message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let issues = match source {
        "github" => parse_github_issues(&stdout),
        "gitlab" => parse_gitlab_issues(&stdout),
        _ => Vec::new(),
    };
    Ok(ProjectScanResult::Success {
        issues,
        remote: url.to_string(),
        source: source.to_string(),
    })
}

/// Extract `owner/repo` from a GitHub/GitLab URL. Tolerant — handles
/// trailing slashes, `.git` suffix, `https://`/`http://`/`git@` URLs,
/// and self-hosted GitLab paths with sub-groups (which `glab` accepts
/// as `group/subgroup/project`).
pub fn parse_owner_repo(url: &str) -> Option<String> {
    let mut s = url.trim();
    // Strip protocol.
    s = s.strip_prefix("https://").unwrap_or(s);
    s = s.strip_prefix("http://").unwrap_or(s);
    s = s.strip_prefix("ssh://").unwrap_or(s);
    s = s.strip_prefix("git://").unwrap_or(s);
    if let Some(rest) = s.strip_prefix("git@") {
        // git@host:owner/repo.git → host/owner/repo
        s = rest;
    }
    // Take everything after the first '/' or ':' (host separator).
    let after_host = s.find(|c: char| c == '/' || c == ':').map(|i| &s[i + 1..])?;
    let path = after_host.trim_end_matches('/').trim_end_matches(".git");
    // Need at least owner/repo.
    if !path.contains('/') || path.is_empty() { return None; }
    Some(path.to_string())
}

fn parse_github_issues(json: &str) -> Vec<ProjectIssue> {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|i| {
            let number = i.get("number")?.as_i64()?;
            let title = i.get("title")?.as_str()?.to_string();
            let body = i.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let url = i.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let labels = i
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| l.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(ProjectIssue {
                external_id: format!("#{}", number),
                title,
                body,
                url,
                source: "github".to_string(),
                labels,
            })
        })
        .collect()
}

fn parse_gitlab_issues(json: &str) -> Vec<ProjectIssue> {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|i| {
            let iid = i.get("iid").and_then(|v| v.as_i64())?;
            let title = i.get("title")?.as_str()?.to_string();
            let body = i.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let url = i.get("web_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let labels = i
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| {
                            l.as_str().map(|s| s.to_string()).or_else(|| {
                                l.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(ProjectIssue {
                external_id: format!("!{}", iid),
                title,
                body,
                url,
                source: "gitlab".to_string(),
                labels,
            })
        })
        .collect()
}
