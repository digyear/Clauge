-- Soft-delete tombstones for synced cards (added 2026-05-09).
--
-- When a user deletes a card that was imported from GitHub/GitLab
-- (i.e. has an external_id), we record the (board_id, external_id)
-- pair here. The next sync skips any external issue whose pair
-- appears in this table — so deleting a Done issue doesn't cause
-- it to keep coming back to Todo on every refresh.
--
-- Local-only cards (no external_id) just hard-delete; this table
-- is only relevant for sync-driven content.

CREATE TABLE IF NOT EXISTS workspace_dismissed_externals (
    board_id     TEXT NOT NULL,
    external_id  TEXT NOT NULL,
    dismissed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (board_id, external_id),
    FOREIGN KEY (board_id) REFERENCES workspace_boards(id) ON DELETE CASCADE
);
