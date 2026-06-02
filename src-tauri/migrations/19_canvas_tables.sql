-- Canvas mode tables — per-workspace tile positions and viewport state.
--
-- Tile rows are resolved lazily when the user first opens Canvas mode
-- for a workspace: the frontend passes the union of currently-open tab
-- references across all modes (agent terminal, ssh terminal, shell
-- terminal, sql editor, rest request, mongo query, redis query,
-- workspace note, workspace board), and the backend upserts missing
-- tiles + deletes orphans.
--
-- tab_id is opaque to Canvas — it's whatever ID the source mode uses.
-- tab_kind tells the frontend adapter registry which adapter to invoke.

CREATE TABLE IF NOT EXISTS canvas_tiles (
    workspace_id   TEXT NOT NULL,
    -- no FK: orphan cleanup happens at app level via canvas_resolve_tiles.
    tab_id         TEXT NOT NULL,
    tab_kind       TEXT NOT NULL CHECK(tab_kind IN (
        'agent_terminal', 'ssh_terminal', 'shell_terminal',
        'sql_editor', 'rest_request',
        'mongo_query', 'redis_query',
        'workspace_note', 'workspace_board'
    )),
    x              REAL NOT NULL,
    y              REAL NOT NULL,
    width          REAL NOT NULL,
    height         REAL NOT NULL,
    z_order        INTEGER NOT NULL DEFAULT 0,
    minimized      INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at     TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (workspace_id, tab_id)
);

CREATE TABLE IF NOT EXISTS canvas_viewports (
    workspace_id     TEXT PRIMARY KEY,
    -- no FK: orphan cleanup happens at app level via canvas_resolve_tiles.
    offset_x         REAL NOT NULL DEFAULT 0,
    offset_y         REAL NOT NULL DEFAULT 0,
    zoom             REAL NOT NULL DEFAULT 1.0,
    last_focused_tab TEXT,
    updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
