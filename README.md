# lighter-timescaledb-ingestor

Market-data ingestor for Lighter (Robinhood Chain), modeled on the sibling
[`bybit-timescaledb-ingestor`](https://github.com/umbertov/bybit-timescaledb-ingestor)
so the same kind of markout/fill research `mm-rs` already does for Bybit
becomes possible for Lighter once enough history accumulates. There is no
historical L2 order-book data available for Lighter anywhere, paid or free
-- it has to be collected going forward, which is what this repo is for.

## Status

- **Symbol sync** (`/api/v1/orderBooks` -> `symbols` table): real, working,
  idempotent. Verified end to end against a live TimescaleDB instance and
  the real Robinhood Chain API on 2026-09-21: 84 markets synced.
- **Trades / order-book ingestion**: real, working. A single WS connection
  subscribes to every synced market's `order_book/{id}` and `trade/{id}`
  channels. Order-book gap detection uses the `nonce`/`begin_nonce` chain;
  on a gap, the whole connection is dropped and resubscribed from scratch.
  Trades and liquidations both land in `trades`, distinguished by
  `trade_type`. Verified end to end against the real Robinhood Chain API
  and WS stream on 2026-09-21: thousands of order-book deltas, snapshots
  for all 84 markets, and trade/liquidation rows written with zero
  duplicates under the `(time, symbol, lighter_trade_id)` unique index.
  Market curation (crypto vs. tokenized asset, see below) is deferred:
  every synced market is ingested today.
- **Private fills** (a `bin/private_ws`-equivalent to `bybit-timescaledb-ingestor`'s,
  for the bot's own fills): out of scope until `mm-rs`'s Lighter connector
  is actually trading -- there's nothing to record yet.

## An important correction to the original plan

The plan this repo was scoped from said Lighter has "57 perp markets."
The live `/api/v1/orderBooks` response has **84** markets as of 2026-09-21,
and they're not all perps: alongside plain symbols (`BTC`, `ETH`, `AAPL`,
`SPY`, ...) there are `SYMBOL/USDG`-suffixed pairs (`TSLA/USDG`,
`NVDA/USDG`, `SGOV/USDG`, ...) that are a distinct market type, not
duplicates. Whatever crypto-vs-tokenized-asset curation happens here needs
to account for both axes (asset class AND market type), not just filter by
symbol name the way `mm-rs`'s `classify_universe.py` did for Bybit. See
`./todos/003-curate-crypto-markets.md`.

## Setup

The real TimescaleDB instance runs on the VM host (already tuned for
its actual RAM, already running `bybit-timescaledb-ingestor`'s
`[other database]` database), reachable from inside the VM at the
QEMU/SLIRP gateway IP -- see `CLAUDE.md`, "Where the TimescaleDB
instance actually lives", before reaching for `docker-compose.yml`'s
`db` service, which is a superseded, unused local convenience:

```sh
cp .env.example .env
# apply migrations/*/up.sql in order against $DATABASE_URL, e.g. via psql
# (a `diesel` CLI isn't in this repo's devShell yet -- see ./todos/)
cargo run --bin lighter-timescaledb-rs
```

## What's shared with `mm-rs`

[`lighter-rs-types`](https://github.com/umbertov/lighter-rs-types) --
wire-format types for Lighter's REST/WebSocket API, also depended on by
`mm-rs`'s `LighterConnector`. Extend that crate, not a private copy here,
when a new Lighter message shape is needed. Read its README's
sourcing-discipline section before adding anything to it.
