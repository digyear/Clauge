-- Clauge D1 schema — initial migration
-- Apply locally:  wrangler d1 execute clauge-db --file=migrations/0001_init.sql --local
-- Apply remote:   wrangler d1 execute clauge-db --file=migrations/0001_init.sql --remote

-- ─── users ─────────────────────────────────────────────────────────
-- One row per human, regardless of how many providers they link.
CREATE TABLE IF NOT EXISTS users (
  user_id           INTEGER PRIMARY KEY AUTOINCREMENT,
  primary_email     TEXT,
  display_name      TEXT,
  first_name        TEXT,
  last_name         TEXT,
  avatar_url        TEXT,
  slug              TEXT NOT NULL UNIQUE,
  plan              TEXT NOT NULL DEFAULT 'free',
  plan_expires_at   TEXT,
  polar_customer_id TEXT,
  created_at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_users_slug_lower
  ON users (LOWER(slug));

-- ─── oauth_identities ──────────────────────────────────────────────
-- A user may have one row per provider; (provider, provider_user_id) is unique.
CREATE TABLE IF NOT EXISTS oauth_identities (
  provider          TEXT NOT NULL,
  provider_user_id  TEXT NOT NULL,
  user_id           INTEGER NOT NULL,
  provider_login    TEXT,
  email             TEXT,
  linked_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  last_seen_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (provider, provider_user_id),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  UNIQUE (user_id, provider)
);

CREATE INDEX IF NOT EXISTS idx_oauth_user
  ON oauth_identities (user_id);

-- ─── sync_blobs ────────────────────────────────────────────────────
-- One row per (user, kind). payload is gzipped JSON of the domain's data.
CREATE TABLE IF NOT EXISTS sync_blobs (
  user_id      INTEGER NOT NULL,
  kind         TEXT    NOT NULL,
  payload      BLOB    NOT NULL,
  content_hash TEXT    NOT NULL,
  updated_at   TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (user_id, kind),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  CHECK (kind IN ('rest','sql','nosql','agent','ssh','explorer'))
);
