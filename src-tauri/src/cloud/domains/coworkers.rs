// Workspace coworkers sync domain.
//
// Coworkers (personas) are user-curated and path-independent — name,
// role, system_prompt, provider, avatar config — so they're the one
// piece of the workspace_* family that makes sense to follow the user
// across devices. The rest of the workspace_* tables (workspaces,
// boards, cards, notes, comments, links) stay device-local because
// they're tied to absolute project_path values.
//
// FK note: workspace_cards and workspace_card_comments reference
// workspace_coworkers via FK ON DELETE SET NULL. We can NOT use the
// agent.rs DELETE-then-INSERT pattern here — that would null out
// `claimed_coworker_id`, `created_by_coworker_id`,
// `updated_by_coworker_id`, and `card_comments.coworker_id` on every
// import, silently losing card attribution on devices that already
// have workspace data. Instead: UPSERT incoming rows (true UPDATE,
// no DELETE) and only remove rows whose id is absent from the payload
// (so a coworker explicitly deleted on another device propagates).

use sqlx::SqlitePool;

use crate::cloud::domains::util::{bind_value_to_query, empty_payload, encode, select_rows_as_json, SyncPayload, TableSpec};

pub const KIND: &str = "coworkers";

/// Merge semantics: insert-missing-only, BY DESIGN. workspace_coworkers has
/// no updated_at column, so last-write-wins is impossible — on rows that
/// exist on both sides, local wins during a merge. This is not an oversight:
/// remote edits to existing coworkers still propagate via full pulls, where
/// import() applies the remote row through its UPSERT.
pub fn merge_specs() -> &'static [TableSpec] {
    &[TableSpec {
        table: "workspace_coworkers",
        pk: "id",
        updated_at: None,
        columns: COLUMNS,
    }]
}

const COLUMNS: &[&str] = &[
    "id",
    "name",
    "role",
    "system_prompt",
    "provider",
    "avatar_seed",
    "avatar_style",
    "created_at",
    "created_by",
    "disabled_at",
];

pub async fn build_payload(pool: &SqlitePool) -> Result<SyncPayload, String> {
    let mut payload = empty_payload(KIND);
    let select = format!(
        "SELECT {} FROM workspace_coworkers ORDER BY created_at ASC",
        COLUMNS.join(", "),
    );
    payload.tables.insert(
        "workspace_coworkers".into(),
        select_rows_as_json(pool, &select).await?,
    );
    Ok(payload)
}

pub async fn export(pool: &SqlitePool) -> Result<(String, String), String> {
    encode(&build_payload(pool).await?)
}

pub async fn import(pool: &SqlitePool, payload: &SyncPayload) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {}", e))?;

    // Collect incoming ids so we know which local rows are "gone from
    // the remote" and need deletion (vs. just absent because the user
    // hasn't created them on this device yet — but on a wholesale pull
    // the payload IS the truth, so "absent from payload" = "deleted").
    let incoming = payload
        .tables
        .get("workspace_coworkers")
        .map(|v| v.as_slice())
        .unwrap_or(&[]);

    let mut incoming_ids: Vec<String> = Vec::with_capacity(incoming.len());
    for row in incoming {
        if let Some(id) = row.get("id").and_then(|v| v.as_str()) {
            incoming_ids.push(id.to_string());
        }
    }

    // Delete rows that aren't in the payload. SET NULL on the FK
    // children only fires for actually-removed rows (not for rows
    // we're about to upsert) — preserves card attribution.
    if incoming_ids.is_empty() {
        sqlx::query("DELETE FROM workspace_coworkers")
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("clear coworkers: {}", e))?;
    } else {
        let placeholders = (0..incoming_ids.len())
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "DELETE FROM workspace_coworkers WHERE id NOT IN ({})",
            placeholders,
        );
        let mut q = sqlx::query(&sql);
        for id in &incoming_ids {
            q = q.bind(id.as_str());
        }
        q.execute(&mut *tx)
            .await
            .map_err(|e| format!("prune coworkers: {}", e))?;
    }

    // UPSERT each incoming row — true UPDATE on conflict, no DELETE,
    // so FK children referencing this id keep their references.
    let placeholders = COLUMNS
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let updates = COLUMNS
        .iter()
        .filter(|c| **c != "id")
        .map(|c| format!("{} = excluded.{}", c, c))
        .collect::<Vec<_>>()
        .join(", ");
    let upsert_sql = format!(
        "INSERT INTO workspace_coworkers ({}) VALUES ({}) \
         ON CONFLICT(id) DO UPDATE SET {}",
        COLUMNS.join(", "),
        placeholders,
        updates,
    );

    for row in incoming {
        let mut q = sqlx::query(&upsert_sql);
        for col in COLUMNS {
            let value = row.get(*col).unwrap_or(&serde_json::Value::Null);
            q = bind_value_to_query(q, value);
        }
        q.execute(&mut *tx)
            .await
            .map_err(|e| format!("upsert coworker: {}", e))?;
    }

    tx.commit().await.map_err(|e| format!("commit: {}", e))?;
    Ok(())
}
