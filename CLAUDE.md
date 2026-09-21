# CLAUDE.md

## What This Is

A TimescaleDB market-data ingestor for Lighter (Robinhood Chain), modeled
on the sibling `bybit-timescaledb-ingestor` repo (also at
`[local sibling repository path]` on umbertov's machine
-- read it directly for proven conventions rather than re-deriving them).
Read `README.md` first for current status and the crypto-vs-tokenized-asset
market-count correction.

## Where things actually stand

Symbol sync (`sync_symbols` in `src/main.rs`) is real and was verified
end to end this session: a live `docker compose up -d db`, migrations
applied via `psql`, `cargo run` against the real
`https://api.rh.lighter.xyz` API, confirmed idempotent (rerunning didn't
duplicate rows). Everything else -- trades, order-book streaming, private
fills -- is unimplemented. `./todos/` (gitignored, not GitHub issues --
these are granular implementation notes, not cross-repo-visible decisions)
has the specifics of what's blocking each one.

**Before implementing trades or order-book ingestion**, resolve the two
open questions in `lighter-rs-types`' own `./todos/` first (that repo,
sibling to this one): whether a WS trade channel exists at all, and
whether order-book WS messages carry any sequence/gap-detection field.
Building a streaming ingestion loop around either one without checking
would be building on a guess -- exactly the kind of thing this project's
standing rule says to avoid (see `mm-rs`'s CLAUDE.md, "Be absolutely
critical of the code you read... flag rather than paper over").

## Conventions (inherited from `bybit-timescaledb-ingestor`)

- Diesel + TimescaleDB hypertables (`tsdb.hypertable`, `segmentby='symbol'`,
  `orderby='time'`, `chunk_interval='1 day'`), `symbols` table keyed by a
  local serial id joined against everything else -- but keyed here by
  Lighter's own `market_id` (`lighter_market_id` column), not insertion
  order, since re-running symbol sync must not create duplicate rows for
  the same market.
- `bids`/`asks` stored as raw JSONB (via `lighter_rs_types::orderbook::PriceLevel`),
  not forced into fixed-depth columns -- same reasoning as
  `bybit-timescaledb-ingestor`'s `orderbook_l50_messages`/`_snapshots`.
- `.cargo/config.toml` pins `target = "x86_64-unknown-linux-gnu"` and
  `flake.nix`'s devShell sets `RUSTFLAGS = "-C relocation-model=static"` +
  an explicit `CARGO_TARGET_..._LINKER` -- without both, `cargo build`
  fails with `rust-lld: relocation R_X86_64_32 cannot be used against
  local symbol` on this nixpkgs/rust-overlay combination. Don't remove
  either without re-testing a clean build.
- `openssl = { features = ["vendored"] }` is a direct dependency purely
  because `pq-sys`'s `bundled` libpq needs OpenSSL and pkg-config can't
  find a system one in the nix devShell -- not because this crate uses TLS
  directly (`reqwest`/`tokio-tungstenite` both use `rustls`).
- A `diesel` CLI isn't wired into this repo's devShell yet (migrations
  were applied via raw `psql` this session) -- `bybit-timescaledb-ingestor`
  doesn't have one either, worth fixing in both if it becomes a recurring
  friction point rather than copying the gap forward silently.

## Testing without spending real API calls needlessly

`sync_symbols` hits the live Lighter API on every run -- fine for
occasional manual runs, don't loop it in a test suite. If this repo grows
a real test suite, prefer fixtures over live calls the way is reasonable,
but there wasn't one substantial enough to need that distinction as of
this session (one integration-shaped `main.rs` flow, manually verified).
