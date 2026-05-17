-- Track which app mode (rest / sql / nosql / ssh / explorer / agent /
-- workspace) consumed each Clauge AI request. Powers the per-mode
-- breakdown card in Settings → AI → Clauge AI tab. Nullable so
-- existing rows from before this migration remain valid.
ALTER TABLE credit_usage_log ADD COLUMN mode TEXT;
