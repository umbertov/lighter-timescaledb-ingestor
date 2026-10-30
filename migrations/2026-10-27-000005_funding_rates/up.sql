-- Settled funding from /api/v1/fundings, one row per perp market and settlement.
-- Lighter settles funding hourly. src/funding.rs inserts with ON CONFLICT (symbol, time)
-- DO NOTHING, so restarts and backfills do not create duplicates.
-- Predicted rates from the market_stats WebSocket channel never enter this table.
CREATE TABLE IF NOT EXISTS funding_rates (
  time TIMESTAMPTZ NOT NULL, -- Settlement time, from Lighter's Unix seconds
  symbol INTEGER NOT NULL, -- Foreign key to symbols.id
  rate NUMERIC NOT NULL, -- Exact decimal from Lighter's rate string
  direction TEXT NOT NULL, -- Lighter's direction string, for example 'long'

  PRIMARY KEY (symbol, time),

  CONSTRAINT fk_symbol_funding_rates FOREIGN KEY (symbol) REFERENCES symbols(id) ON DELETE CASCADE ON UPDATE CASCADE
) WITH (
  tsdb.hypertable,
  tsdb.partition_column='time',
  tsdb.segmentby='symbol',
  tsdb.orderby='time',
  tsdb.chunk_interval='7 days'
);
