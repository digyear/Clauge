// Triggers an agent CLI from a workspace card comment ("@claude"
// mention). Fire-and-forget: shell out, capture stdout, write the
// agent's reply back as a card comment, return.
//
// Multi-agent design
// ──────────────────
// Every provider has its own non-interactive ("print mode") invocation:
//   • Claude Code → `claude -p <prompt> [--resume <id>]`
//   • Codex CLI   → TBD (slot in `oneshot_argv`)
//   • Gemini CLI  → TBD
//   • OpenCode    → TBD
// The provider-specific knowledge lives in ONE function — `oneshot_argv`
// — so adding a new agent is a single match arm, no other plumbing.
//
// Today the agent_sessions table only carries `claude_session_id`, so
// `provider_id_for_session` returns `"claude"` unless we explicitly add
// other provider columns later. Architecture is ready; data model isn't
// yet, by design (single-provider today).

use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::shared::repos::{sessions, workspaces as repo};

/// How long we wait for a one-shot agent invocation before giving up.
/// 5 minutes is generous — code-edit prompts on slow networks routinely
/// take 60–120s; anything past 5 minutes is almost certainly hung.
const ONESHOT_TIMEOUT_SECS: u64 = 300;

/// Maximum prompt size we'll feed to the CLI as a single argv. Past
/// this we truncate to avoid blowing past argv-length limits on macOS
/// (~256 KB) and Linux (~2 MB). The card description is the input, so
/// in practice this rarely matters.
const PROMPT_MAX_BYTES: usize = 64 * 1024;

/// Trigger the linked session of `card_id` with `body`. Returns:
/// `{ ok: true, response: <text>, sessionId, provider }` on success.
/// On failure, `Err(message)` — the caller surfaces it as a toast or
/// JSON-RPC error.
pub async fn mention_card_session(
    pool: &SqlitePool,
    card_id: &str,
    body: &str,
    actor: &str,
) -> Result<Value, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("Comment body is empty".into());
    }

    // 1. Card → linked_session_id. We pull the live row directly so we
    //    can also grab title + description for the agent's context.
    let card_row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT title, description, linked_session_id \
         FROM workspace_board_cards WHERE id = ?",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB error reading card: {e}"))?;
    let (title, description, linked) =
        card_row.ok_or_else(|| "Card not found".to_string())?;

    let session_id = linked
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "No session linked to this card".to_string())?;

    // 2. Session row must exist. fetch_one would surface as "RowNotFound";
    //    convert that to a friendlier message.
    let session = sessions::get_session_by_id(pool, session_id)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                "Linked session no longer exists. Re-link a session to use this.".to_string()
            }
            other => format!("DB error reading session: {other}"),
        })?;

    let provider = provider_id_for_session(&session);

    // 3. Dirty-tree gate. The agent will modify files in `project_path`;
    //    if there's already uncommitted work there from another card,
    //    those changes get tangled. Refuse the mention with the actual
    //    `git status` output so the user knows exactly what to commit
    //    or stash. If `git` itself isn't on PATH or the directory isn't
    //    a repo, we silently skip — non-git projects aren't blocked.
    if let Some(report) = check_working_tree(&session.project_path) {
        return Err(report);
    }

    // 4. Persist the user's comment as a real row first — even if the
    //    agent fails downstream, the comment is preserved in the thread.
    let now = chrono::Utc::now().to_rfc3339();
    let user_comment_id = uuid::Uuid::new_v4().to_string();
    repo::insert_card_comment(pool, &user_comment_id, card_id, actor, trimmed, None, &now)
        .await
        .map_err(|e| format!("DB error inserting user comment: {e}"))?;

    // 5. Build the prompt — card identity + thread so far + user's
    //    new message. The full thread (loaded from the comments table)
    //    keeps the agent oriented even if the linked session has drifted.
    let prior_comments = repo::list_card_comments(pool, card_id)
        .await
        .map_err(|e| format!("DB error reading thread: {e}"))?;
    let prompt = build_prompt(&title, &description, &prior_comments, trimmed);
    let truncated = truncate_to_bytes(&prompt, PROMPT_MAX_BYTES);

    // 5. Resolve argv per-provider. Errors here mean "we don't know how
    //    to drive this provider yet" — useful guard rail when codex/
    //    gemini sessions land before their argv arms.
    let resume_id = match provider {
        "claude" => session.claude_session_id.clone(),
        _ => None,
    };
    let argv = oneshot_argv(provider, &truncated, resume_id.as_deref())?;
    if argv.is_empty() {
        return Err(format!(
            "Provider '{provider}' is not yet supported for mention triggers"
        ));
    }

    // 6. Spawn with cwd = project_path so the CLI sees the right repo.
    let project_path = session.project_path.clone();
    let provider_owned = provider.to_string();
    let argv_owned = argv.clone();

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(ONESHOT_TIMEOUT_SECS),
        tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(&argv_owned[0]);
            cmd.args(&argv_owned[1..]).current_dir(&project_path);
            cmd.output()
        }),
    )
    .await
    .map_err(|_| {
        format!(
            "{} timed out after {}s — agent may be stuck",
            provider_owned, ONESHOT_TIMEOUT_SECS
        )
    })?
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
    .map_err(|e| format!("{} failed to spawn: {}", provider, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let lower = stderr.to_lowercase();
        let friendly = if lower.contains("auth") || lower.contains("logged in") || lower.contains("token") {
            format!("{provider} is not authenticated. Run `{provider} /login` and retry.")
        } else if stderr.is_empty() {
            format!("{provider} exited with non-zero status (no stderr)")
        } else {
            stderr
        };
        return Err(friendly);
    }

    let response = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if response.is_empty() {
        return Err(format!("{provider} returned an empty response"));
    }

    // 7. Persist the agent's reply as a comment row attributed to the
    //    provider slug. Same pipeline as user comments — bumps the
    //    card's updated_at/by so the thread stays the conversation locus.
    let reply_now = chrono::Utc::now().to_rfc3339();
    let reply_id = uuid::Uuid::new_v4().to_string();
    repo::insert_card_comment(pool, &reply_id, card_id, provider, &response, None, &reply_now)
        .await
        .map_err(|e| format!("DB error inserting agent reply: {e}"))?;

    Ok(json!({
        "ok": true,
        "sessionId": session_id,
        "provider": provider,
        "response": response,
        "userCommentId": user_comment_id,
        "replyCommentId": reply_id,
    }))
}

/// Inspect the working tree at `project_path`. Returns:
///   • `None` — clean, OR not a git repo, OR `git` isn't on PATH (we
///     silently skip rather than block legitimate non-git projects).
///   • `Some(message)` — dirty; the message is a user-facing error
///     ready to surface as a toast / JSON-RPC error. It includes the
///     branch name and the abridged porcelain output so the user can
///     act on it without opening a terminal.
fn check_working_tree(project_path: &str) -> Option<String> {
    use std::process::Command;

    // 1. Is git even installed?
    let which_bin = if cfg!(target_os = "windows") { "where" } else { "which" };
    let git_present = Command::new(which_bin)
        .arg("git")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !git_present {
        return None;
    }

    // 2. Are we inside a work tree?
    let inside = Command::new("git")
        .args(["-C", project_path, "rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()?;
    if !inside.status.success() {
        return None;
    }

    // 3. Porcelain status. Empty output = clean.
    let status = Command::new("git")
        .args(["-C", project_path, "status", "--porcelain"])
        .output()
        .ok()?;
    if !status.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&status.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 4. Branch name — purely for the error message.
    let branch_out = Command::new("git")
        .args(["-C", project_path, "branch", "--show-current"])
        .output()
        .ok();
    let branch = branch_out
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "(detached)".to_string());

    // 5. Build a tidy report — first 12 lines so we don't drown the
    //    toast on huge diffs.
    let lines: Vec<&str> = trimmed.lines().take(12).collect();
    let suffix = if trimmed.lines().count() > lines.len() {
        format!("\n  … and {} more", trimmed.lines().count() - lines.len())
    } else {
        String::new()
    };
    let listing = lines.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n");
    Some(format!(
        "Working tree on '{branch}' has uncommitted changes — agents \
         won't run while it's dirty. Commit or stash first, then retry.\n\n{listing}{suffix}"
    ))
}

/// Provider id for an `agent_sessions` row. The schema doesn't carry
/// an explicit provider column today (single-provider product), so we
/// hard-code `"claude"`. When codex / gemini sessions land they should
/// add a column (e.g. `provider TEXT NOT NULL DEFAULT 'claude'`) and
/// this function flips to reading it.
fn provider_id_for_session(_session: &crate::modes::agent::models::AgentSession) -> &'static str {
    "claude"
}

/// Per-provider argv for a non-interactive ("print mode") invocation.
/// Adding a new agent is one new arm. Returning an empty Vec signals
/// "not supported yet" so callers can give a clear error instead of
/// silently spawning the wrong binary.
fn oneshot_argv(
    provider: &str,
    prompt: &str,
    resume_id: Option<&str>,
) -> Result<Vec<String>, String> {
    match provider {
        "claude" => {
            let mut argv = vec!["claude".to_string(), "-p".to_string(), prompt.to_string()];
            if let Some(sid) = resume_id {
                argv.push("--resume".to_string());
                argv.push(sid.to_string());
            }
            Ok(argv)
        }
        // ── Future agents ─────────────────────────────────────────
        // Codex CLI — print-mode flag TBD. When wiring this, ensure
        //   `codex --help` confirms the right flag (likely `-p` or
        //   `--prompt`) and append `--resume <id>` if supported.
        // Gemini CLI — `gemini -p <prompt>` once their print mode lands.
        // OpenCode    — `opencode <prompt>` is the current shape.
        // Aider       — non-interactive aider is uncommon; consider
        //   exposing it as a worktree-mode alternative instead.
        "codex" | "gemini" | "opencode" | "aider" => Ok(Vec::new()),
        _ => Err(format!("Unknown provider '{provider}'")),
    }
}

fn build_prompt(
    card_title: &str,
    card_desc: &str,
    prior: &[crate::modes::workspace::models::WorkspaceCardComment],
    user_msg: &str,
) -> String {
    // Keep the framing minimal so the agent doesn't get a giant boilerplate
    // injection on every short comment. We render the prior thread as
    // `<actor>: <body>` lines — agents handle this format well without
    // any further markup.
    let mut prompt = format!("Card: {card_title}\n");
    if !card_desc.trim().is_empty() {
        prompt.push_str("\nDescription:\n");
        prompt.push_str(card_desc.trim());
        prompt.push('\n');
    }
    if !prior.is_empty() {
        prompt.push_str("\nPrior thread (oldest first):\n");
        for c in prior {
            prompt.push_str(&format!("{}: {}\n", c.actor, c.body));
        }
    }
    prompt.push_str("\nNew user comment: ");
    prompt.push_str(user_msg);
    prompt.push_str(
        "\n\nRespond conversationally; if you need to make code changes, do them and summarise.",
    );
    prompt
}

fn truncate_to_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Cut at a char boundary so we never split a multi-byte sequence.
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push_str("\n…[truncated]");
    out
}
