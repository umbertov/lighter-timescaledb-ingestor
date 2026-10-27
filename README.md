# lighter-timescaledb-ingestor

This Rust service stores Lighter market data in TimescaleDB. It syncs market symbols, then reads trades and order-book updates from the public WebSocket API.

## Status

The service supports perpetual and spot markets. It stores trade, liquidation, deleverage, and market-settlement events in `trades`, with a type for each row. It stores order-book updates as JSONB price levels. It detects order-book gaps with the nonce chain and reconnects when a gap occurs. It also reconnects after WebSocket read failures.
Container uptime does not guarantee that the WebSocket stays connected. The service logs failures and reconnects.

The service also collects settled funding for every perp market in `funding_rates`. It polls `/api/v1/fundings`, which returns settled funding only. Predicted rates from the `market_stats` WebSocket channel are not stored.

Private account fills are out of scope until the trading client uses Lighter.

## Setup

Copy the sample settings file. Use its password only for local development.

```sh
cp .env.example .env
```

Start the database:

```sh
nix run .#compose -- up -d db
```

Apply each `migrations/*/up.sql` file in timestamp order or use Diesel CLI. The `DATABASE_URL` value in `.env` connects from the host.

The ingestor requires two funding settings. It stops at startup when either one is missing:

- `LIGHTER_FUNDING_POLL_INTERVAL_SECS`: seconds between funding polls, a whole number of at least 1.
- `LIGHTER_FUNDING_BACKFILL_START`: the first settlement to collect for a market with no stored funding, in RFC 3339 UTC, for example `2026-01-01T00:00:00Z`.

Start the ingestor after you apply the migrations:

```sh
nix run .#up
```

## Funding rates

The poller runs after the symbol sync. For each perp market, it requests `/api/v1/fundings` with `resolution=1h`. It starts after the newest stored settlement, or at the backfill start when the market has no stored funding. It stores each row with `ON CONFLICT (symbol, time) DO NOTHING`, so restarts do not duplicate rows.

Each `funding_rates` row holds one settlement: `time` is the settlement time, `rate` is the exact decimal from Lighter, and `direction` is Lighter's direction string. The `value` field from the API is not stored because the docs do not define it.

The poller waits 1.1 seconds after each request. This keeps a Standard account under its 60 requests per minute REST limit. The API returns at most 750 rows per call, and the poller pages through a full page.

A failed request, a malformed response, or a database error stops the poller and exits the process. Docker Compose restarts the service because the `ingestor` service sets `restart: unless-stopped`.

Query the newest stored settlement for one market:

```sql
SELECT max(time) FROM funding_rates WHERE symbol = (SELECT id FROM symbols WHERE name = 'NVDA' AND market_type = 'perp');
```

## Export data

Export all trades and order-book messages to Parquet files:

```sh
cargo run -- export --output-dir ./export
```

Export one ticker or a group of tickers. Repeat `--symbol` for each ticker name:

```sh
cargo run -- export --output-dir ./export --symbol BTC --symbol ETH
```

Export only the last N hours. For example, this command exports the last 2 days:

```sh
cargo run -- export --output-dir ./export --hours 48
```

The `--hours` value must be at least 1. Combine it with `--symbol` and `--dataset`.

Select one dataset with `--dataset trades` or `--dataset orderbooks`. The default is `both`.
The export includes the ticker name in the `symbol` column. It does not include the database symbol ID.
The command writes `trades.parquet` and `orderbook_messages.parquet` when it selects both datasets.
The command writes batches of 8,192 rows. It does not load a full table into memory.
Order-book bid and ask arrays use JSON strings in Parquet.
By default, the command replaces existing output files. Add `--no-overwrite` to stop with an error when an output file exists. The command checks all output files before it reads any data.

The `up` command runs Docker Compose with `--build`. Compose builds the ingestor image with `Dockerfile`.
The Dockerfile builds the Nix default package and copies its binary into a small runtime image.
CI builds the flake Docker image with the Nix cache. CI does not use `Dockerfile`.
Run `nix run .#compose -- down` to stop the services.
Run Nix commands from the project root. Docker must be installed and its daemon must run.

The database uses PostgreSQL 18 with TimescaleDB. Run the ingestor and database with Docker Compose.

## Lighter message types

The `lighter-rs-types` crate in `crates/` provides message types for the Lighter REST and WebSocket APIs. Check that crate before you add a message shape.

Build all workspace crates from the repository root:

```sh
cargo build --workspace
```

## Continuous integration

The Nix package source contains Rust code and Cargo files. Changes to documentation and Compose files do not rebuild the Rust package.
Pushes to `main` start CI only when Rust, Cargo, Nix, or workflow files change. Pull requests skip the full checks job when these files do not change.
Manual runs execute the full checks without publishing. GitHub Actions checks Rust formatting, runs Clippy, runs workspace tests, and builds the Docker image.
Pull request checks do not receive an OpenID Connect token or publish images.
Pushes to `main` run the Nix cache and build the image. A successful `main` push publishes the image.
The publish job adds the `latest` tag and a commit tag after the checks pass.
The workflow file has a code owner. Require a code owner review before you merge workflow changes.
