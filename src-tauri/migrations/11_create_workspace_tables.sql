-- Workspaces — a Workspace is a container for Notes and Boards. Project
-- link is OPTIONAL: a workspace may exist on its own (research notes,
-- personal todos) or be bound to a project path (auto-creates a default
-- board on creation; useful for issue-sync later). Items inherit Project
-- from their parent workspace; we never store project on notes/boards.
--
-- Attribution columns: every editable row carries created_by/updated_by.
-- Format: 'user' (anonymous), 'user:<github-login>' (GitHub-synced), or
-- the agent's CliRunner.id() string ('claude', 'codex', etc.) when an
-- AI worker mutates via MCP. Last-editor only in v1; full edit trail
-- deferred to v1.5.

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    project_path TEXT,
    project_name TEXT,
    color TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_by TEXT NOT NULL DEFAULT 'user',
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT NOT NULL DEFAULT 'user'
);

CREATE TABLE IF NOT EXISTS workspace_notes (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '[]',
    linked_session_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_by TEXT NOT NULL DEFAULT 'user',
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT NOT NULL DEFAULT 'user',
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
    FOREIGN KEY (linked_session_id) REFERENCES agent_sessions(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS workspace_boards (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'manual',
    source_config TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS workspace_board_columns (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL,
    name TEXT NOT NULL,
    color TEXT,
    position INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (board_id) REFERENCES workspace_boards(id) ON DELETE CASCADE
);

-- review_pending: 1 when an agent moved this card into a Review-class
-- column. The user clears the flag by approving (move to Done) or
-- requesting changes (move back to Doing). Drives the "Pending review"
-- badge in the UI. Independent of column name so user-renamed columns
-- still work.
CREATE TABLE IF NOT EXISTS workspace_board_cards (
    id TEXT PRIMARY KEY,
    column_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    priority TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    position INTEGER NOT NULL,
    external_id TEXT,
    external_url TEXT,
    linked_session_id TEXT,
    review_pending INTEGER NOT NULL DEFAULT 0,
    review_checklist TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_by TEXT NOT NULL DEFAULT 'user',
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT NOT NULL DEFAULT 'user',
    FOREIGN KEY (column_id) REFERENCES workspace_board_columns(id) ON DELETE CASCADE,
    FOREIGN KEY (linked_session_id) REFERENCES agent_sessions(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_workspace_notes_workspace ON workspace_notes(workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_boards_workspace ON workspace_boards(workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_columns_board ON workspace_board_columns(board_id);
CREATE INDEX IF NOT EXISTS idx_workspace_cards_column ON workspace_board_cards(column_id);
