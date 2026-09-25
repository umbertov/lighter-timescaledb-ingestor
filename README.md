# lighter-timescaledb-ingestor

This Rust service stores Lighter market data in TimescaleDB. It syncs market symbols, then reads trades and order-book updates from the public WebSocket API.

## Status

The service supports perpetual and spot markets. It stores trade and liquidation events in `trades`, with a trade type for each row. It stores order-book updates as JSONB price levels. It detects order-book gaps with the nonce chain and reconnects when a gap occurs.

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

Start the ingestor after you apply the migrations:

```sh
nix run .#up
```

The `up` command builds and loads the flake Docker image, then starts the ingestor and TimescaleDB.
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

GitHub Actions checks Rust formatting, runs Clippy, runs workspace tests, and builds the Docker image for pull requests.
Pushes to `main` and tags that start with `v` publish the image to GitHub Container Registry.
The workflow caches Nix store paths between runs and passes the checked image to the publish job as an artifact.
Main branch pushes publish `latest` and a commit tag. Version tags publish the matching version tag.
