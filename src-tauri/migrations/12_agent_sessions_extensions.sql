-- agent_sessions extensions for the workspace card-driven chat model
-- (added 2026-05-09).
--
-- Kept as ALTERs (not folded into migration 4) because alpha testers
-- have real `agent_sessions` rows from Agent mode usage; we don't want
-- to nuke their session history.
--
-- Columns:
--   origin       — 'manual' (user-spawned terminal session, shows in
--                  Agent panel) | 'card' (drawer-spawned hidden session
--                  for a workspace card; filtered out of the panel).
--   card_id      — backref to the card that owns this hidden session.
--                  NULL for manual sessions.
--   coworker_id  — which coworker (persona) is driving this session.
--                  NULL for manual sessions; required for card-origin.

ALTER TABLE agent_sessions
    ADD COLUMN origin TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE agent_sessions
    ADD COLUMN card_id TEXT;
ALTER TABLE agent_sessions
    ADD COLUMN coworker_id TEXT;

-- Lookup index: "give me the hidden session for (card, coworker)"
-- runs every time the drawer opens or the user switches coworker.
CREATE INDEX IF NOT EXISTS idx_agent_sessions_card_coworker
    ON agent_sessions(card_id, coworker_id) WHERE card_id IS NOT NULL;
