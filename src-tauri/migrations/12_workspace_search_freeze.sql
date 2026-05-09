-- Adds the infrastructure for v1.1 workspace MCP tools:
--   • FTS5 virtual tables for note + card full-text search
--     (sync triggers keep them aligned with the base tables).
--   • `frozen` flag on notes + cards (1 = blocked from agent edits;
--     UI is free to edit).
--   • `repo_url` on workspaces (workspace-level GitHub/GitLab URL,
--     overrides nothing — the per-board source_config still wins
--     when set; this is the default the agent can fall back to).

-- ── Frozen flag ────────────────────────────────────────────────────
-- Default 0 so existing rows stay editable by everyone.
ALTER TABLE workspace_notes       ADD COLUMN frozen INTEGER NOT NULL DEFAULT 0;
ALTER TABLE workspace_board_cards ADD COLUMN frozen INTEGER NOT NULL DEFAULT 0;

-- ── Workspace-level repo URL ───────────────────────────────────────
ALTER TABLE workspaces ADD COLUMN repo_url TEXT;

-- ── Note FTS5 ──────────────────────────────────────────────────────
-- `content='workspace_notes', content_rowid='rowid'` makes this a
-- contentless FTS pointing at the base table; we still need INSERT/
-- UPDATE/DELETE triggers because contentless FTS only auto-syncs when
-- we also use the FTS5 `content_rowid` integration, which sqlite-rs
-- doesn't quite cleanly. Explicit triggers are simpler + portable.
CREATE VIRTUAL TABLE IF NOT EXISTS workspace_notes_fts USING fts5(
    title,
    content,
    workspace_id UNINDEXED,
    note_id UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Backfill any existing rows so the index isn't stale on first
-- search (the migration runs once per database, including alpha-user
-- DBs that already have notes from earlier sessions).
INSERT INTO workspace_notes_fts (title, content, workspace_id, note_id)
    SELECT title, content, workspace_id, id FROM workspace_notes;

CREATE TRIGGER IF NOT EXISTS workspace_notes_ai
    AFTER INSERT ON workspace_notes
BEGIN
    INSERT INTO workspace_notes_fts (title, content, workspace_id, note_id)
    VALUES (new.title, new.content, new.workspace_id, new.id);
END;

CREATE TRIGGER IF NOT EXISTS workspace_notes_ad
    AFTER DELETE ON workspace_notes
BEGIN
    DELETE FROM workspace_notes_fts WHERE note_id = old.id;
END;

CREATE TRIGGER IF NOT EXISTS workspace_notes_au
    AFTER UPDATE OF title, content ON workspace_notes
BEGIN
    DELETE FROM workspace_notes_fts WHERE note_id = old.id;
    INSERT INTO workspace_notes_fts (title, content, workspace_id, note_id)
    VALUES (new.title, new.content, new.workspace_id, new.id);
END;

-- ── Card FTS5 ──────────────────────────────────────────────────────
CREATE VIRTUAL TABLE IF NOT EXISTS workspace_board_cards_fts USING fts5(
    title,
    description,
    column_id UNINDEXED,
    card_id UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO workspace_board_cards_fts (title, description, column_id, card_id)
    SELECT title, description, column_id, id FROM workspace_board_cards;

CREATE TRIGGER IF NOT EXISTS workspace_cards_ai
    AFTER INSERT ON workspace_board_cards
BEGIN
    INSERT INTO workspace_board_cards_fts (title, description, column_id, card_id)
    VALUES (new.title, new.description, new.column_id, new.id);
END;

CREATE TRIGGER IF NOT EXISTS workspace_cards_ad
    AFTER DELETE ON workspace_board_cards
BEGIN
    DELETE FROM workspace_board_cards_fts WHERE card_id = old.id;
END;

CREATE TRIGGER IF NOT EXISTS workspace_cards_au
    AFTER UPDATE OF title, description, column_id ON workspace_board_cards
BEGIN
    DELETE FROM workspace_board_cards_fts WHERE card_id = old.id;
    INSERT INTO workspace_board_cards_fts (title, description, column_id, card_id)
    VALUES (new.title, new.description, new.column_id, new.id);
END;
