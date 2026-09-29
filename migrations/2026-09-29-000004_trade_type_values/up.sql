-- The previous validated constraint already limits existing rows to this set.
-- Add the wider check without a full hypertable scan.
ALTER TABLE trades
  DROP CONSTRAINT IF EXISTS trades_trade_type_check,
  ADD CONSTRAINT trades_trade_type_check
  CHECK (trade_type IN ('trade', 'liquidation', 'deleverage', 'market-settlement')) NOT VALID;
