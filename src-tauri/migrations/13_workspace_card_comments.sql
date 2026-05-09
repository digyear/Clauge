-- Real comments table for workspace board cards. Replaces the
-- markdown-blockquote-in-description hack used through v12: every
-- "Post & @claude", "Approve with comment", "Request changes" call
-- appended a `> **<actor> · <stamp>**` block to the description and
-- reparsed on render. That worked for short threads but doesn't
-- support replies, edits, deletes, or independent ordering — and
-- agents bumping the description was the wrong primitive.
--
-- Every comment-creating MCP tool (cards_add_comment, cards_approve,
-- cards_request_changes, cards_mention_session) writes a row here
-- instead. The card's `updated_at` / `updated_by` are still bumped
-- by the insert helper so the inbox + per-card unread tracking
-- continue to work without query changes.

CREATE TABLE IF NOT EXISTS workspace_card_comments (
    id          TEXT PRIMARY KEY,
    card_id     TEXT NOT NULL,
    -- Same actor format as everything else in workspace tables:
    --   'user' | 'user:<github>' | 'claude' | 'codex' | 'gemini' | 'opencode' | …
    actor       TEXT NOT NULL,
    body        TEXT NOT NULL,
    -- Reserved for threaded replies (one level deep is enough for v1).
    -- Always NULL today; reading code already handles a NULL parent.
    parent_id   TEXT,
    created_at  TEXT NOT NULL,
    FOREIGN KEY (card_id)   REFERENCES workspace_board_cards(id)   ON DELETE CASCADE,
    FOREIGN KEY (parent_id) REFERENCES workspace_card_comments(id) ON DELETE SET NULL
);

-- Primary read pattern: "all comments for this card, oldest first".
CREATE INDEX IF NOT EXISTS idx_card_comments_card_id_created_at
    ON workspace_card_comments(card_id, created_at);
