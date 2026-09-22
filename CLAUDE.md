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

## Where the TimescaleDB instance actually lives

There is no local docker-compose Postgres for real dev work on
umbertov's machine -- `docker-compose.yml`'s `db` service (port 5433)
was a session-1 convenience that got superseded once it became clear a
real, already-correctly-tuned TimescaleDB instance exists one hop away.
umbertov's dev machine is a VM; the real Postgres runs on the **host**,
tuned for the host's actual [host memory removed] RAM (`shared_buffers = [memory setting removed]`,
`effective_cache_size = [memory setting removed]` in its `postgresql.conf` -- do not
"fix" these from inside the VM by comparing against `free -h` run
*inside* the VM, which only sees the VM's own much smaller memory
allocation; that comparison is meaningless and almost caused an
unnecessary retune of a correctly-configured production-adjacent
instance this session).

- Reachable from inside the VM at the QEMU/SLIRP gateway IP, **not**
  `localhost`: `[host address removed]`, port 5432 (find the gateway via `ip route
  show default` if the IP ever changes). `localhost:5432` inside the VM
  does not reach it.
- Auth: `postgres` user, no password, e.g.
  `DATABASE_URL=postgresql://postgres@[host address removed]/market_data`.
- This host instance also serves `bybit-timescaledb-ingestor`'s
  `[other database]` database. `market_data` is a **separate database
  on the same instance** (created via `CREATE DATABASE market_data
  TEMPLATE template0 LC_COLLATE 'C.UTF-8' LC_CTYPE 'C.UTF-8'` -- the
  instance's default collation is `C.UTF-8`, not `C`, so a plain
  `CREATE DATABASE market_data` fails on a collation mismatch) --
  table names need no `lighter_`-prefix namespacing, since the
  databases are already isolated from each other.
- The host instance runs TimescaleDB 2.30.0, which is what the
  `tsdb.hypertable` WITH-clause hypertable syntax in
  `migrations/2026-09-21-000001_lighter_tables_v1/up.sql` needs (that
  syntax was added in TimescaleDB 2.18; it does NOT work against the
  2.17.2 bundled in the locally cached `timescale/timescaledb:latest-pg17`
  docker image used by this repo's own now-superseded `docker-compose.yml`
  `db` service).
- Apply migrations the same way as before: raw `psql -f
  migrations/*/up.sql` in order against that `DATABASE_URL` (no diesel
  CLI in this devShell yet, see below).

## Migration idempotency, and backfilling diesel's bookkeeping

Every `up.sql` must be safe to run twice in a row against the same
database (`CREATE TABLE IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS`,
`ADD COLUMN IF NOT EXISTS`, etc.) -- test this explicitly (`psql -f
up.sql` twice against a scratch database, or `diesel migration run`
twice) before considering a new migration done, not just a single clean
apply. This is not a hypothetical: `2026-09-21-000001_lighter_tables_v1`
originally shipped with un-guarded `CREATE INDEX` statements, which was
fine as long as everything ran through raw `psql` by hand, but broke the
moment `diesel migration run` was used for real against a database
whose tables/indexes already existed -- it errored out mid-migration on
`relation "trades_symbol_lighter_trade_id_idx" already exists`.

Since this repo's migrations were originally applied by hand via `psql`
(no diesel CLI in the devShell, see above) rather than through diesel
itself, `__diesel_schema_migrations` did not reflect that
`lighter_tables_v1`/`trades_ws_and_dedup` were already applied. Running
`diesel migration revert -a && diesel migration run` against that state
replayed migrations diesel thought were pending, exposed the
non-idempotency above, and then a follow-up `migration revert` against
the still-live hypertables failed on `DROP EXTENSION ... other objects
depend on it` (TimescaleDB's own columnstore/retention background jobs
still referenced it). No data was lost either time -- both failures were
non-destructive statements erroring out, not destructive ones
succeeding -- but the fix was to backfill the missing bookkeeping rows
by hand once satisfied the schema already matched:
`INSERT INTO __diesel_schema_migrations (version) VALUES
('20260921000001'), ('20260921000002') ON CONFLICT (version) DO
NOTHING;` (version = the migration folder's timestamp with the dashes
removed). Do this instead of a destructive revert/recreate cycle
whenever diesel's bookkeeping falls behind schema state that was
actually applied out-of-band.

## Testing without spending real API calls needlessly

`sync_symbols` hits the live Lighter API on every run -- fine for
occasional manual runs, don't loop it in a test suite. If this repo grows
a real test suite, prefer fixtures over live calls the way is reasonable,
but there wasn't one substantial enough to need that distinction as of
this session (one integration-shaped `main.rs` flow, manually verified).
