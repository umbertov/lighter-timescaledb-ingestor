ALTER TABLE trades DROP CONSTRAINT IF EXISTS trades_trade_type_check;

ALTER TABLE trades
  ADD CONSTRAINT trades_trade_type_check
  CHECK (trade_type IN ('trade', 'liquidation'));
