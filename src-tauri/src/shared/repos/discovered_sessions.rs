use sqlx::SqlitePool;

use crate::modes::agent::models::{
    AgentDiscoveredSession, DiscoveredSessionListOptions, DiscoveredSessionUpsert,
};

pub async fn upsert_discovered_session(
    pool: &SqlitePool,
    item: &DiscoveredSessionUpsert,
) -> Result<AgentDiscoveredSession, sqlx::Error> {
    let id = format!("{}:{}", item.provider, item.external_session_id);
    let created_at = item
        .created_at
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&item.last_seen_at);
    let updated_at = item
        .updated_at
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&item.last_seen_at);

    sqlx::query(
        "INSERT INTO agent_discovered_sessions (
            id, provider, external_session_id, project_path, project_name,
            title, preview, created_at, updated_at, last_seen_at,
            parent_external_session_id, session_kind, source_path
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(provider, external_session_id) DO UPDATE SET
            project_path = COALESCE(excluded.project_path, agent_discovered_sessions.project_path),
            project_name = COALESCE(excluded.project_name, agent_discovered_sessions.project_name),
            title = COALESCE(excluded.title, agent_discovered_sessions.title),
            preview = COALESCE(excluded.preview, agent_discovered_sessions.preview),
            updated_at = CASE
                WHEN excluded.updated_at > agent_discovered_sessions.updated_at THEN excluded.updated_at
                ELSE agent_discovered_sessions.updated_at
            END,
            last_seen_at = excluded.last_seen_at,
            parent_external_session_id = COALESCE(excluded.parent_external_session_id, agent_discovered_sessions.parent_external_session_id),
            session_kind = COALESCE(excluded.session_kind, agent_discovered_sessions.session_kind),
            source_path = COALESCE(excluded.source_path, agent_discovered_sessions.source_path)",
    )
    .bind(&id)
    .bind(&item.provider)
    .bind(&item.external_session_id)
    .bind(&item.project_path)
    .bind(&item.project_name)
    .bind(&item.title)
    .bind(&item.preview)
    .bind(created_at)
    .bind(updated_at)
    .bind(&item.last_seen_at)
    .bind(&item.parent_external_session_id)
    .bind(&item.session_kind)
    .bind(&item.source_path)
    .execute(pool)
    .await?;

    get_by_provider_external(pool, &item.provider, &item.external_session_id).await
}

pub async fn get_by_provider_external(
    pool: &SqlitePool,
    provider: &str,
    external_session_id: &str,
) -> Result<AgentDiscoveredSession, sqlx::Error> {
    sqlx::query_as::<_, AgentDiscoveredSession>(
        "SELECT * FROM agent_discovered_sessions
         WHERE provider = ? AND external_session_id = ?",
    )
    .bind(provider)
    .bind(external_session_id)
    .fetch_one(pool)
    .await
}

pub async fn list_discovered_sessions(
    pool: &SqlitePool,
    opts: &DiscoveredSessionListOptions,
) -> Result<Vec<AgentDiscoveredSession>, sqlx::Error> {
    let include_hidden = opts.include_hidden.unwrap_or(false);
    let provider = opts
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let project_path = opts
        .project_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let search = opts
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let search_like = search.map(|s| format!("%{}%", s));

    sqlx::query_as::<_, AgentDiscoveredSession>(
        "SELECT * FROM agent_discovered_sessions
         WHERE (? = 1 OR hidden = 0)
           AND (? IS NULL OR provider = ?)
           AND (? IS NULL OR project_path = ?)
           AND (
                ? IS NULL
                OR external_session_id LIKE ?
                OR COALESCE(project_name, '') LIKE ?
                OR COALESCE(project_path, '') LIKE ?
                OR COALESCE(title, '') LIKE ?
                OR COALESCE(preview, '') LIKE ?
           )
         ORDER BY provider ASC, COALESCE(project_name, project_path, '') ASC, updated_at DESC",
    )
    .bind(if include_hidden { 1 } else { 0 })
    .bind(provider)
    .bind(provider)
    .bind(project_path)
    .bind(project_path)
    .bind(search_like.as_deref())
    .bind(search_like.as_deref())
    .bind(search_like.as_deref())
    .bind(search_like.as_deref())
    .bind(search_like.as_deref())
    .bind(search_like.as_deref())
    .fetch_all(pool)
    .await
}

pub async fn set_hidden(
    pool: &SqlitePool,
    id: &str,
    hidden: bool,
    hidden_at: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE agent_discovered_sessions
         SET hidden = ?, hidden_at = ?
         WHERE id = ?",
    )
    .bind(if hidden { 1 } else { 0 })
    .bind(if hidden { hidden_at } else { None })
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn memory_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE agent_discovered_sessions (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                external_session_id TEXT NOT NULL,
                project_path TEXT,
                project_name TEXT,
                title TEXT,
                preview TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                parent_external_session_id TEXT,
                session_kind TEXT,
                source_path TEXT,
                hidden INTEGER NOT NULL DEFAULT 0,
                hidden_at TEXT,
                adopted_agent_session_id TEXT,
                UNIQUE(provider, external_session_id)
            );",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn upsert_deduplicates_by_provider_and_external_id() {
        let pool = memory_pool().await;
        let base = DiscoveredSessionUpsert {
            provider: "claude".into(),
            external_session_id: "sid-1".into(),
            project_path: Some("/repo".into()),
            project_name: Some("repo".into()),
            title: Some("First".into()),
            preview: Some("hello".into()),
            created_at: Some("2026-01-01T00:00:00Z".into()),
            updated_at: Some("2026-01-01T00:00:00Z".into()),
            last_seen_at: "2026-01-01T00:00:00Z".into(),
            parent_external_session_id: None,
            session_kind: Some("conversation".into()),
            source_path: Some("/tmp/a.jsonl".into()),
        };
        let first = upsert_discovered_session(&pool, &base).await.unwrap();

        let second = DiscoveredSessionUpsert {
            preview: Some("updated".into()),
            updated_at: Some("2026-01-02T00:00:00Z".into()),
            last_seen_at: "2026-01-03T00:00:00Z".into(),
            ..base
        };
        let row = upsert_discovered_session(&pool, &second).await.unwrap();
        let rows = list_discovered_sessions(
            &pool,
            &DiscoveredSessionListOptions {
                include_hidden: Some(true),
                provider: None,
                project_path: None,
                search: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(first.id, row.id);
        assert_eq!(rows.len(), 1);
        assert_eq!(row.preview.as_deref(), Some("updated"));
        assert_eq!(row.updated_at, "2026-01-02T00:00:00Z");
        assert_eq!(row.last_seen_at, "2026-01-03T00:00:00Z");
    }
}
