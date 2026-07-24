//! Schema migrations for the ZeroAny Workbench SQLite database.
//!
//! Migrations live as numbered SQL files under `src-tauri/migrations/`
//! (`V<n>__<description>.sql`). The `sqlx::migrate!` macro embeds them at
//! compile time, computes per-migration checksums, and runs each one
//! exactly once per database — tracked in the `_sqlx_migrations` table.
//!
//! Adding a new migration: drop a numbered `.sql` file in `migrations/`,
//! rebuild. No code changes required here.
//!
//! For databases that pre-date the introduction of this migrator (alpha
//! users on the old hand-rolled runner), [`run`] first calls
//! [`super::bootstrap::seed_existing_install`] to detect what's already
//! applied and seed `_sqlx_migrations` with the matching checksums.
//! Without that step, sqlx-migrate would attempt to re-run V1–Vn against
//! schemas that already exist, hit duplicate-table / duplicate-column
//! errors, roll back the transaction, and fail.

use sqlx::sqlite::SqlitePool;

use super::bootstrap;

/// Compile-time-embedded migration set. The path is relative to the
/// crate root (`src-tauri/`).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Bring the database to the latest schema version, preserving existing data.
///
/// Steps:
///   1. Bootstrap `_sqlx_migrations` for legacy databases (recover state-B
///      from the old broken v7, then schema-probe each version's signature
///      and seed the tracking table with matching checksums).
///   2. Run any unapplied migrations transactionally via sqlx-migrate.
pub async fn run(pool: &SqlitePool) -> Result<(), String> {
    bootstrap::seed_existing_install(pool, &MIGRATOR)
        .await
        .map_err(|e| format!("migration bootstrap: {}", e))?;

    MIGRATOR
        .run(pool)
        .await
        .map_err(|e| format!("migration apply: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn fresh_database_has_discovered_session_project_root() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        run(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('agent_discovered_sessions') \
             WHERE name = 'project_root'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }
}
