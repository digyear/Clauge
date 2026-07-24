-- Persistent catalog of provider-native sessions discovered outside
-- ZeroAny Workbench's managed agent_sessions table. Provider stores remain the
-- source of truth; this table is only a local index plus hide/adoption
-- metadata.

CREATE TABLE IF NOT EXISTS agent_discovered_sessions (
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
    UNIQUE(provider, external_session_id),
    FOREIGN KEY (adopted_agent_session_id) REFERENCES agent_sessions(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_discovered_sessions_provider_project
    ON agent_discovered_sessions(provider, project_name, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_agent_discovered_sessions_last_seen
    ON agent_discovered_sessions(last_seen_at DESC);

CREATE INDEX IF NOT EXISTS idx_agent_discovered_sessions_adopted
    ON agent_discovered_sessions(adopted_agent_session_id)
    WHERE adopted_agent_session_id IS NOT NULL;
