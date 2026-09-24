ALTER TABLE symbols
  ADD COLUMN IF NOT EXISTS market_type TEXT NOT NULL DEFAULT 'perp'
  CHECK (market_type IN ('perp', 'spot'));

ALTER TABLE symbols ALTER COLUMN market_type DROP DEFAULT;

ALTER TABLE symbols DROP CONSTRAINT IF EXISTS symbols_name_key;

CREATE UNIQUE INDEX IF NOT EXISTS symbols_market_type_name_uniq
  ON symbols (market_type, name);
