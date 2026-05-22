use sqlx::SqlitePool;

use crate::db::models::Setting;

pub async fn get_by_key(pool: &SqlitePool, key: &str) -> Result<Option<Setting>, sqlx::Error> {
    sqlx::query_as::<_, Setting>("SELECT * FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
}

pub async fn upsert(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Setting>, sqlx::Error> {
    sqlx::query_as::<_, Setting>("SELECT * FROM settings")
        .fetch_all(pool)
        .await
}

/// Read a numeric setting; fall back to `default` when the row is
/// missing, the value doesn't parse, or any DB error happens. Never
/// errors out — these are tuning knobs, the app shouldn't crash on
/// malformed values. Used by SQL / NoSQL connection builders + AI
/// tools to source timeouts / limits from the Settings UI.
pub async fn get_u64_or(pool: &SqlitePool, key: &str, default: u64) -> u64 {
    match get_by_key(pool, key).await {
        Ok(Some(s)) => s.value.parse::<u64>().unwrap_or(default),
        _ => default,
    }
}

pub async fn get_i64_or(pool: &SqlitePool, key: &str, default: i64) -> i64 {
    match get_by_key(pool, key).await {
        Ok(Some(s)) => s.value.parse::<i64>().unwrap_or(default),
        _ => default,
    }
}

/// Boolean settings are stored as the strings "true"/"false" or "1"/"0".
/// Anything unrecognised falls back to `default`.
pub async fn get_bool_or(pool: &SqlitePool, key: &str, default: bool) -> bool {
    match get_by_key(pool, key).await {
        Ok(Some(s)) => match s.value.as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => default,
        },
        _ => default,
    }
}
