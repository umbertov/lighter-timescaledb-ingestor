DROP INDEX IF EXISTS symbols_market_type_name_uniq;

ALTER TABLE symbols DROP COLUMN IF EXISTS market_type;

ALTER TABLE symbols
  ADD CONSTRAINT symbols_name_key UNIQUE (name);
