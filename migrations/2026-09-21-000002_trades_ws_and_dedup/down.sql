ALTER TABLE trades DROP COLUMN trade_type;
DROP INDEX IF EXISTS trades_dedup_uniq;
