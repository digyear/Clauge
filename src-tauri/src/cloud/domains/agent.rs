use sqlx::SqlitePool;

use crate::cloud::domains::util::{empty_payload, encode, insert_row, select_rows_as_json, SyncPayload, TableSpec};

pub const KIND: &str = "agent";

pub fn merge_specs() -> &'static [TableSpec] {
    &[TableSpec {
        table: "agent_contexts",
        pk: "id",
        updated_at: Some("updated_at"),
        columns: &["id", "name", "content", "created_at", "updated_at"],
    }]
}

pub async fn build_payload(pool: &SqlitePool) -> Result<SyncPayload, String> {
    // Only agent_contexts travel — the named system-prompt snippets the
    // user manages under Settings → Agent → Contexts. agent_sessions
    // are intentionally machine-local: they're glued to claude_session_id,
    // worktree paths, project_path, and other state that doesn't make
    // sense to clone across devices.
    let mut payload = empty_payload(KIND);
    payload.tables.insert(
        "agent_contexts".into(),
        select_rows_as_json(pool, "SELECT id, name, content, created_at, updated_at FROM agent_contexts ORDER BY name").await?,
    );
    Ok(payload)
}

pub async fn export(pool: &SqlitePool) -> Result<(String, String), String> {
    encode(&build_payload(pool).await?)
}

pub async fn import(pool: &SqlitePool, payload: &SyncPayload) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {}", e))?;

    sqlx::query("DELETE FROM agent_contexts")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear contexts: {}", e))?;

    if let Some(rows) = payload.tables.get("agent_contexts") {
        for r in rows {
            insert_row(&mut tx, "agent_contexts", &[
                "id","name","content","created_at","updated_at",
            ], r).await?;
        }
    }

    tx.commit().await.map_err(|e| format!("commit: {}", e))?;
    Ok(())
}
