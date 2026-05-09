use sqlx::SqlitePool;

use crate::db::models::HistoryEntry;

pub async fn list_recent(pool: &SqlitePool, limit: i32) -> Result<Vec<HistoryEntry>, sqlx::Error> {
    sqlx::query_as::<_, HistoryEntry>(
        "SELECT h.*, r.name AS request_name
         FROM history h
         LEFT JOIN requests r ON h.request_id = r.id
         ORDER BY h.created_at DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn clear_all(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM history").execute(pool).await?;
    Ok(())
}

pub async fn delete_by_id(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM history WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn count_all(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM history")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Delete history entries older than `seconds` from now.
/// `created_at` is stored in SQLite default datetime format ("YYYY-MM-DD HH:MM:SS" UTC).
pub async fn purge_older_than(pool: &SqlitePool, seconds: i64) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM history WHERE created_at < datetime('now', ?)",
    )
    .bind(format!("-{} seconds", seconds))
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}
