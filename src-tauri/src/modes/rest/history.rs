use sqlx::SqlitePool;
use tauri::State;

use crate::db::models::HistoryEntry;
use crate::shared::repos::history as history_repo;

#[tauri::command]
pub async fn list_history(
    pool: State<'_, SqlitePool>,
    limit: i32,
) -> Result<Vec<HistoryEntry>, String> {
    history_repo::list_recent(pool.inner(), limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_history(pool: State<'_, SqlitePool>) -> Result<(), String> {
    history_repo::clear_all(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_history_entry(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    history_repo::delete_by_id(pool.inner(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn count_history(pool: State<'_, SqlitePool>) -> Result<i64, String> {
    history_repo::count_all(pool.inner())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn purge_history(
    pool: State<'_, SqlitePool>,
    seconds: i64,
) -> Result<u64, String> {
    if seconds <= 0 {
        return Ok(0);
    }
    history_repo::purge_older_than(pool.inner(), seconds)
        .await
        .map_err(|e| e.to_string())
}
