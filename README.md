# lighter-timescaledb-ingestor

Market-data ingestor for Lighter (Robinhood Chain), modeled on the sibling
[`bybit-timescaledb-ingestor`](https://github.com/umbertov/bybit-timescaledb-ingestor)
so the same kind of markout/fill research `mm-rs` already does for Bybit
becomes possible for Lighter once enough history accumulates. There is no
historical Level 2 (L2) order-book data available for Lighter anywhere, paid or free
-- it has to be collected going forward, which is what this repo is for.

## Status

- **Symbol sync** (`/api/v1/orderBooks` -> `symbols` table): real and
  idempotent. The public Application Programming Interface (API) returns
  each market's type and Lighter market ID.
- **Trade and order-book ingestion**: real and working. A single WebSocket
  subscribes to every synced market's `order_book/{id}` and `trade/{id}`
  channels. Order-book gap detection uses the `nonce`/`begin_nonce` chain;
  on a gap, the whole connection is dropped and resubscribed from scratch.
  Trades and liquidations both land in `trades`, distinguished by
  `trade_type`. Verified end to end against the real Robinhood Chain API
  and WebSocket stream on 2026-09-21: thousands of order-book deltas,
  snapshots for all 84 market IDs, and trade/liquidation rows with no duplicates.
  The database enforces uniqueness on `(time, symbol, lighter_trade_id)`.
  Market curation (crypto vs. tokenized asset, see below) is deferred:
  every synced market is ingested today.
- **Spot markets**: the existing loop already subscribes to every API market,
  including spot IDs. The live stream accepted `order_book/2048` and
  `trade/2048` on 2026-09-24. This change stores each market type explicitly.
  A 50-second live run wrote 1,239 spot order-book messages and 94 spot trades
  to `market_data` on 2026-09-24.
- **Private fills** (a `bin/private_ws`-equivalent to `bybit-timescaledb-ingestor`'s,
  for the bot's own fills): out of scope until `mm-rs`'s Lighter connector
  is actually trading -- there's nothing to record yet.

## An important correction to the original plan

The live `/api/v1/orderBooks` response had 84 markets on 2026-09-21.
On 2026-09-24, it reported 57 perps and 27 spot markets. Spot symbols use names such
as `ETH/USDG` and `TSLA/USDG`. The Application Programming Interface (API)
marks these markets as `spot`.
Market curation remains open. Any curation must consider asset class and
market type. See `./todos/003-curate-crypto-markets.md`.

## Setup

The real TimescaleDB instance runs on the Virtual Machine (VM) host. It is
already tuned for the host's actual Random Access Memory (RAM) and runs `bybit-timescaledb-ingestor`'s
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

### Running it persistently (inside the VM)

`systemd/lighter-timescaledb-ingestor.service` runs the release binary
as a `systemd --user` service, restarting on failure on top of the
binary's own WebSocket (WS) reconnect loop:

```sh
cargo build --release
mkdir -p ~/.config/systemd/user
ln -sf "$(pwd)/systemd/lighter-timescaledb-ingestor.service" ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now lighter-timescaledb-ingestor
loginctl enable-linger "$USER"  # survive logout, not just this session
journalctl --user -u lighter-timescaledb-ingestor -f  # tail logs
```

## What's shared with `mm-rs`

[`lighter-rs-types`](https://github.com/umbertov/lighter-rs-types) --
wire-format types for Lighter's Representational State Transfer (REST) and
WebSocket APIs, also used by
`mm-rs`'s `LighterConnector`. Extend that crate, not a private copy here,
when a new Lighter message shape is needed. Read its README's
sourcing-discipline section before adding anything to it.
