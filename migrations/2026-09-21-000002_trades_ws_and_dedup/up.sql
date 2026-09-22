-- `trades` previously had no real dedup key: insert_trades' on_conflict_do_nothing
-- had no conflict target, and trades_symbol_lighter_trade_id_idx (from the
-- v1 migration) is non-unique. On reconnect, subscribed/trade's history
-- replay would insert duplicate rows silently. `time` is now the trade's
-- own exchange-assigned timestamp (see TradeRow), so it is a stable dedup
-- key, not wall-clock ingestion time.
CREATE UNIQUE INDEX IF NOT EXISTS trades_dedup_uniq
  ON trades (time, symbol, lighter_trade_id);

-- trade/{market_id}'s subscribed/update messages carry both `trades`
-- (regular fills) and `liquidation_trades` (forced liquidations) arrays,
-- distinguished only by lighter_rs_types::trade::Trade.trade_type. Land
-- both in the same table instead of dropping liquidation records.
ALTER TABLE trades ADD COLUMN IF NOT EXISTS trade_type TEXT NOT NULL DEFAULT 'trade'
  CHECK (trade_type IN ('trade', 'liquidation'));
ALTER TABLE trades ALTER COLUMN trade_type DROP DEFAULT;
