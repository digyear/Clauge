// Workspace notes sync. Machine-local columns are EXCLUDED from the wire:
//   workspaces.project_path / project_name — absolute paths differ per machine
//   workspace_notes.linked_session_id      — agent sessions are device-local
// Import is merge-only (UPSERT-union, newer updated_at wins) — notes are
// long-form text; wholesale DELETE+INSERT would eat edits. Excluded columns
// stay NULL on new rows and untouched on existing rows.

use sqlx::SqlitePool;

use crate::cloud::domains::util::{
    self, empty_payload, encode, select_rows_as_json, SyncPayload, TableSpec,
};

pub const KIND: &str = "workspace_notes";

// `workspaces` is exported by both workspace kinds intentionally — merge
// semantics make the duplicate import idempotent, and each kind stays
// self-sufficient if the other has no remote blob yet.
const WORKSPACE_COLS: &[&str] = &[
    "id", "name", "color", "repo_url", "created_at", "created_by", "updated_at", "updated_by",
];
const NOTE_COLS: &[&str] = &[
    "id", "workspace_id", "title", "content", "tags", "frozen",
    "created_at", "created_by", "updated_at", "updated_by",
];

fn specs() -> &'static [TableSpec] {
    &[
        TableSpec {
            table: "workspaces",
            pk: "id",
            updated_at: Some("updated_at"),
            columns: WORKSPACE_COLS,
        },
        TableSpec {
            table: "workspace_notes",
            pk: "id",
            updated_at: Some("updated_at"),
            columns: NOTE_COLS,
        },
    ]
}

pub async fn build_payload(pool: &SqlitePool) -> Result<SyncPayload, String> {
    let mut payload = empty_payload(KIND);
    payload.tables.insert(
        "workspaces".into(),
        select_rows_as_json(
            pool,
            &format!(
                "SELECT {} FROM workspaces ORDER BY created_at ASC",
                WORKSPACE_COLS.join(", "),
            ),
        )
        .await?,
    );
    payload.tables.insert(
        "workspace_notes".into(),
        select_rows_as_json(
            pool,
            &format!(
                "SELECT {} FROM workspace_notes ORDER BY created_at ASC",
                NOTE_COLS.join(", "),
            ),
        )
        .await?,
    );
    Ok(payload)
}

pub async fn export(pool: &SqlitePool) -> Result<(String, String), String> {
    encode(&build_payload(pool).await?)
}

/// Both plain import and merge use merge semantics — never wipe notes.
pub async fn import(pool: &SqlitePool, payload: &SyncPayload) -> Result<(), String> {
    util::merge_import(pool, payload, specs()).await
}

pub fn merge_specs() -> &'static [TableSpec] {
    specs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn merge_import_keeps_fts_in_sync() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::migrator::MIGRATOR.run(&pool).await.unwrap();

        let mut p = empty_payload(KIND);
        p.tables.insert(
            "workspaces".into(),
            vec![serde_json::json!({
                "id": "w1", "name": "W", "color": null, "repo_url": null,
                "created_at": "2026-01-01", "created_by": "user",
                "updated_at": "2026-01-01", "updated_by": "user",
            })
            .as_object()
            .unwrap()
            .clone()],
        );
        p.tables.insert(
            "workspace_notes".into(),
            vec![serde_json::json!({
                "id": "n1", "workspace_id": "w1", "title": "findme",
                "content": "hello body", "tags": "[]", "frozen": 0,
                "created_at": "2026-01-01", "created_by": "user",
                "updated_at": "2026-01-01", "updated_by": "user",
            })
            .as_object()
            .unwrap()
            .clone()],
        );
        import(&pool, &p).await.unwrap();

        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM workspace_notes_fts WHERE workspace_notes_fts MATCH 'findme'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "FTS triggers must fire on merge inserts");
    }
}
