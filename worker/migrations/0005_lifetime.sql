-- Pro v3 — Lifetime plan support.
--
-- Adds a one-time "Lifetime" tier ($299) on top of the existing recurring
-- Monthly + Yearly. Lifetime users get 20,000 credits/year refilled on
-- their purchase anniversary (lazy refill at /api/ai/{balance,chat} time —
-- no new cron job).
--
-- The recurring subscription columns (polar_subscription_id, cancel_at_
-- period_end, past_due_started_at) stay NULL/0 for lifetime users — they
-- just don't apply. The past-due sweep filters on is_lifetime=0.

-- ─── users: lifetime flag + order id ────────────────────────────────────
ALTER TABLE users ADD COLUMN is_lifetime INTEGER NOT NULL DEFAULT 0
  CHECK (is_lifetime IN (0, 1));

ALTER TABLE users ADD COLUMN polar_lifetime_order_id TEXT;

-- ─── billing_pricing + billing_discount: drop the plan_id CHECK ────────
-- SQLite has no ALTER for CHECK constraints — the existing
-- `plan_id IN ('monthly','yearly')` would reject 'lifetime' inserts.
-- We recreate WITHOUT the CHECK so any future plan tier (lifetime now,
-- maybe "team" later) is a pure INSERT — no more schema migrations.
-- Source of truth for valid plan ids stays in worker code (planToProductId).
CREATE TABLE billing_pricing_new (
  plan_id   TEXT PRIMARY KEY,
  price_usd INTEGER NOT NULL CHECK (price_usd > 0)
);
INSERT INTO billing_pricing_new SELECT plan_id, price_usd FROM billing_pricing;
DROP TABLE billing_pricing;
ALTER TABLE billing_pricing_new RENAME TO billing_pricing;

CREATE TABLE billing_discount_new (
  plan_id TEXT PRIMARY KEY,
  percent INTEGER NOT NULL CHECK (percent > 0 AND percent < 100),
  code    TEXT
);
INSERT INTO billing_discount_new SELECT plan_id, percent, code FROM billing_discount;
DROP TABLE billing_discount;
ALTER TABLE billing_discount_new RENAME TO billing_discount;

-- New pricing + per-plan discount codes (visible in UpgradeModal).
INSERT OR REPLACE INTO billing_pricing (plan_id, price_usd) VALUES
  ('monthly',   12),
  ('yearly',   100),
  ('lifetime', 299);

INSERT OR REPLACE INTO billing_discount (plan_id, percent, code) VALUES
  ('monthly',  15, 'MONTHLY15'),
  ('yearly',   20, 'YEARLY20'),
  ('lifetime', 25, 'LIFE25');
