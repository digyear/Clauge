-- Keep the provider-native cwd (`project_path`) separate from the stable
-- main-repository identity used to group linked worktrees. Existing managed
-- sessions can backfill rows created before this column was introduced.

ALTER TABLE agent_discovered_sessions ADD COLUMN project_root TEXT;

UPDATE agent_discovered_sessions
SET project_root = (
    SELECT agent_sessions.project_path
    FROM agent_sessions
    WHERE agent_sessions.worktree_path = agent_discovered_sessions.project_path
    ORDER BY agent_sessions.last_used_at DESC
    LIMIT 1
)
WHERE project_root IS NULL
  AND project_path IS NOT NULL
  AND EXISTS (
      SELECT 1
      FROM agent_sessions
      WHERE agent_sessions.worktree_path = agent_discovered_sessions.project_path
  );

CREATE INDEX IF NOT EXISTS idx_agent_discovered_sessions_project_root
    ON agent_discovered_sessions(project_root, updated_at DESC);
