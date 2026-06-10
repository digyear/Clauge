// Workspace boards sync (boards / columns / cards / comments).
//
// Machine-local columns are EXCLUDED from the wire:
//   workspaces.project_path / project_name      — absolute paths differ per machine
//   workspace_board_cards.linked_session_id     — agent sessions are device-local
//   workspace_board_cards.claimed_session_id    — live claim, backed by a local session
//   workspace_board_cards.claimed_coworker_id   — live claim, one active chat per card
// Stripping the claim columns means synced cards arrive unclaimed on the
// other device — correct under the single-claim model: a claim is a live
// conversation on THIS machine, not durable card state.
//
// FK dependency: cards carry created_by_coworker_id / updated_by_coworker_id
// and comments carry coworker_id, all referencing workspace_coworkers — the
// `coworkers` kind must pull before this one (see sync::pull_order_rank).
//
// Import is merge-only (UPSERT-union, newer updated_at wins; comments are
// immutable → insert-only). Never wipes local rows.

use sqlx::SqlitePool;

use crate::cloud::domains::util::{
    self, empty_payload, encode, select_rows_as_json, SyncPayload, TableSpec,
};

pub const KIND: &str = "workspace_boards";

// `workspaces` is exported by both workspace kinds intentionally — merge
// semantics make the duplicate import idempotent, and each kind stays
// self-sufficient if the other has no remote blob yet.
const WORKSPACE_COLS: &[&str] = &[
    "id", "name", "color", "repo_url", "created_at", "created_by", "updated_at", "updated_by",
];
const BOARD_COLS: &[&str] = &[
    "id", "workspace_id", "name", "source", "source_config", "position",
    "created_at", "updated_at",
];
const COLUMN_COLS: &[&str] = &[
    "id", "board_id", "name", "color", "position", "created_at",
];
const CARD_COLS: &[&str] = &[
    "id", "column_id", "title", "description", "priority", "tags", "position",
    "external_id", "external_url", "pr_url", "review_pending", "review_checklist",
    "frozen", "created_at", "created_by", "created_by_coworker_id",
    "updated_at", "updated_by", "updated_by_coworker_id",
];
const COMMENT_COLS: &[&str] = &[
    "id", "card_id", "actor", "coworker_id", "body", "parent_id", "created_at",
];

fn specs() -> &'static [TableSpec] {
    // FK-parents-first: workspaces → boards → columns → cards → comments.
    &[
        TableSpec {
            table: "workspaces",
            pk: "id",
            updated_at: Some("updated_at"),
            columns: WORKSPACE_COLS,
        },
        TableSpec {
            table: "workspace_boards",
            pk: "id",
            updated_at: Some("updated_at"),
            columns: BOARD_COLS,
        },
        TableSpec {
            table: "workspace_board_columns",
            pk: "id",
            updated_at: None,
            columns: COLUMN_COLS,
        },
        TableSpec {
            table: "workspace_board_cards",
            pk: "id",
            updated_at: Some("updated_at"),
            columns: CARD_COLS,
        },
        TableSpec {
            table: "workspace_card_comments",
            pk: "id",
            updated_at: None,
            columns: COMMENT_COLS,
        },
    ]
}

pub async fn build_payload(pool: &SqlitePool) -> Result<SyncPayload, String> {
    let mut payload = empty_payload(KIND);
    let selects: &[(&str, &[&str])] = &[
        ("workspaces", WORKSPACE_COLS),
        ("workspace_boards", BOARD_COLS),
        ("workspace_board_columns", COLUMN_COLS),
        ("workspace_board_cards", CARD_COLS),
        // ORDER BY created_at keeps comment parents before their replies
        // (parent_id self-FK) so insert-order is FK-safe on import.
        ("workspace_card_comments", COMMENT_COLS),
    ];
    for (table, cols) in selects {
        payload.tables.insert(
            (*table).into(),
            select_rows_as_json(
                pool,
                &format!("SELECT {} FROM {} ORDER BY created_at ASC", cols.join(", "), table),
            )
            .await?,
        );
    }
    Ok(payload)
}

pub async fn export(pool: &SqlitePool) -> Result<(String, String), String> {
    encode(&build_payload(pool).await?)
}

/// Both plain import and merge use merge semantics — never wipe boards.
pub async fn import(pool: &SqlitePool, payload: &SyncPayload) -> Result<(), String> {
    util::merge_import(pool, payload, specs()).await
}

pub fn merge_specs() -> &'static [TableSpec] {
    specs()
}
