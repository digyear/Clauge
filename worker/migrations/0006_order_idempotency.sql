-- Pro v3 — order.paid idempotency.
--
-- The webhook-id dedup in handleBillingWebhook catches identical deliveries.
-- It does NOT catch legitimate re-deliveries of the same business event
-- (Polar may retry order.paid with a fresh delivery ID after operator action
-- or transient errors). Without an order-id check, every retry re-grants
-- `allowance × months` credits.
--
-- This column stores the most recent Polar order ID whose recurring
-- credit grant was applied. handleOrderPaid short-circuits when the
-- incoming order.id matches. Lifetime purchases use the existing
-- `polar_lifetime_order_id` column for the same purpose.

ALTER TABLE users ADD COLUMN last_granted_order_id TEXT;
