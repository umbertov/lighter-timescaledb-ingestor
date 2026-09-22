-- Ensure the TimescaleDB extension is available
CREATE EXTENSION IF NOT EXISTS timescaledb;

--------------------------------------------------
-- Symbol Table
--------------------------------------------------
CREATE TABLE IF NOT EXISTS symbols (
  id SERIAL PRIMARY KEY,
  name VARCHAR(255) NOT NULL UNIQUE,
  -- Lighter's own numeric market_id, e.g. 0 for the BTC perp on Robinhood
  -- Chain. Kept distinct from `id` (this table's own serial key, shared
  -- shape with bybit-timescaledb-ingestor's `symbols` table) since Lighter
  -- assigns its own ids independently of insertion order here.
  lighter_market_id INTEGER NOT NULL UNIQUE
);

--------------------------------------------------
-- Trades Table
--------------------------------------------------
-- REST-polled from /api/v1/recentTrades, deduplicated by lighter_trade_id --
-- see lighter-rs-types' trade.rs doc comment for why this isn't WS-pushed.
CREATE TABLE IF NOT EXISTS trades (
  id BIGSERIAL,
  time TIMESTAMPTZ NOT NULL,
  symbol INTEGER NOT NULL, -- Foreign key to symbols.id
  lighter_trade_id BIGINT NOT NULL,
  price DOUBLE PRECISION NOT NULL,
  size DOUBLE PRECISION NOT NULL,
  is_maker_ask BOOLEAN NOT NULL,

  PRIMARY KEY (time, id),

  CONSTRAINT fk_symbol_trades FOREIGN KEY (symbol) REFERENCES symbols(id) ON DELETE CASCADE ON UPDATE CASCADE
) WITH (
   tsdb.hypertable,
   tsdb.partition_column='time',
   tsdb.segmentby='symbol',
   tsdb.orderby='time',
   tsdb.chunk_interval='1 day'
);

CREATE INDEX IF NOT EXISTS trades_symbol_lighter_trade_id_idx
  ON trades (symbol, lighter_trade_id DESC);

--------------------------------------------------
-- Order Book Tables
--------------------------------------------------
-- `bids`/`asks` are stored as raw JSONB arrays of
-- lighter_rs_types::orderbook::PriceLevel ({price, size} decimal strings),
-- the same "don't force a fixed depth into columns" approach
-- bybit-timescaledb-ingestor uses for orderbook_l50_messages/snapshots.
--
-- Not yet populated by any ingestion code as of this migration -- see
-- ./todos/001-implement-orderbook-ws-ingestion.md. `sequence` is nullable
-- because it is NOT CONFIRMED to exist on Lighter's WS messages at all
-- (see lighter-rs-types' todos/002); do not assume it will ever be
-- non-null until that's resolved.
CREATE TABLE IF NOT EXISTS orderbook_messages (
  id BIGSERIAL,
  time TIMESTAMPTZ NOT NULL,
  symbol INTEGER NOT NULL,
  message_type TEXT NOT NULL CHECK (message_type IN ('snapshot', 'delta')),
  sequence BIGINT,
  bids JSONB NOT NULL,
  asks JSONB NOT NULL,
  PRIMARY KEY (time, id),
  CONSTRAINT fk_symbol_orderbook_messages
    FOREIGN KEY (symbol) REFERENCES symbols(id) ON DELETE CASCADE ON UPDATE CASCADE
) WITH (
  tsdb.hypertable,
  tsdb.partition_column='time',
  tsdb.segmentby='symbol',
  tsdb.orderby='time',
  tsdb.chunk_interval='1 day'
);

CREATE INDEX IF NOT EXISTS orderbook_messages_symbol_time_idx
  ON orderbook_messages (symbol, time DESC);
CREATE INDEX IF NOT EXISTS orderbook_messages_time_brin
  ON orderbook_messages USING BRIN(time);

SELECT add_retention_policy(
  'orderbook_messages',
  drop_after => INTERVAL '4 days',
  if_not_exists => TRUE
);
