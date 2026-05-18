-- Pro v3 — per-plan credit allowance moves from KV/code constant to D1.
--
-- Before: monthly = `pro:default_allowance` KV (default 1000), yearly =
-- allowance × periodMonthsFromBounds (= 12), lifetime = `LIFETIME_ONE_TIME_CREDITS`
-- code constant (= 20000). Three sources of truth, two of them invisible to
-- operators.
--
-- After: every plan's credit grant lives next to its price in billing_pricing.
-- Operator changes via D1 console alongside price; pricing endpoint exposes
-- the value so the marketing site and desktop UI render the right number
-- without redeploys.

ALTER TABLE billing_pricing
  ADD COLUMN credits INTEGER NOT NULL DEFAULT 0 CHECK (credits >= 0);

UPDATE billing_pricing SET credits = 1000  WHERE plan_id = 'monthly';
UPDATE billing_pricing SET credits = 12000 WHERE plan_id = 'yearly';
UPDATE billing_pricing SET credits = 20000 WHERE plan_id = 'lifetime';
