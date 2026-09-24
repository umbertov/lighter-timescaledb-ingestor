# lighter-timescaledb-ingestor

This Rust service stores Lighter market data in TimescaleDB. It syncs market symbols, then reads trades and order-book updates from the public WebSocket API.

## Status

The service supports perpetual and spot markets. It stores trade and liquidation events in `trades`, with a trade type for each row. It stores order-book updates as JSONB price levels. It detects order-book gaps with the nonce chain and reconnects when a gap occurs.

Private account fills are out of scope until the trading client uses Lighter.

## Setup

Start the local database with Docker Compose:

```sh
docker compose up -d db
```

Copy the sample settings file and edit it for your database:

```sh
cp .env.example .env
```

Apply each `migrations/*/up.sql` file in timestamp order. Then start the service:

```sh
cargo run --bin lighter-timescaledb-rs
```

The sample database password is for local development only. Change it before you expose the database outside your machine.

## Lighter message types

The `lighter-rs-types` crate provides message types for the Lighter REST and WebSocket APIs. Check that crate before you add a message shape.
