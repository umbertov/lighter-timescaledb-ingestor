# Project guidance

Read `README.md` before you change the project. Maintain `README.md` and `CLAUDE.md` after you change the project.

## Data model

- Symbol records store the Lighter market ID and market type.
- Trades and order-book messages use TimescaleDB hypertables.
- Store order-book bids and asks as JSONB price-level arrays.
- Use `lighter_market_id` as the symbol upsert conflict key.
- Export Parquet data through the `export` command. Use the ticker name in the exported `symbol` column.
- Stream query results into bounded Arrow batches. Do not load full hypertables into memory.
- Check `lighter-rs-types` before you add Lighter message shapes.

## Migrations

- Make each `up.sql` safe to run twice.
- Test migrations against a disposable database before you use production data.
- Do not revert a migration when Diesel bookkeeping differs from the schema.
- Inspect the schema first, then repair migration bookkeeping if needed.

## Development

- Copy `.env.example` to `.env` before you start Compose.
- Start PostgreSQL 18 with `nix run .#compose -- up -d db`.
- Apply migrations before you start the ingestor with `nix run .#up`.
- Use `nix run .#compose -- down` to stop the Compose services.
- Keep `.env` local. Do not commit it.
- Keep `.env.example` as the portable settings template.
- Do not log database connection strings.
- Use fixtures for automated tests that need market data.
- Do not call the live API repeatedly from a test suite.

## CI

- Keep formatting, Clippy, workspace tests, and the Docker image build in `.github/workflows/ci.yml`.
- Run full CI only when Rust, Cargo, Nix, or workflow paths change.
- Use the same path list for push filters and pull request checks.
- Limit the Nix package source to Rust build inputs.
- Publish images to GitHub Container Registry only after all checks pass.
