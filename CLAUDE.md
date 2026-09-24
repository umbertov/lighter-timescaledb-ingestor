# Project guidance

Read `README.md` before you change the project.

## Data model

- Symbol records store the Lighter market ID and market type.
- Trades and order-book messages use TimescaleDB hypertables.
- Store order-book bids and asks as JSONB price-level arrays.
- Use `lighter_market_id` as the symbol upsert conflict key.
- Check `lighter-rs-types` before you add Lighter message shapes.

## Migrations

- Make each `up.sql` safe to run twice.
- Test migrations against a disposable database before you use production data.
- Do not revert a migration when Diesel bookkeeping differs from the schema.
- Inspect the schema first, then repair migration bookkeeping if needed.

## Development

- Use `.env` for local settings. Do not commit that file.
- Use `.env.example` as the portable settings template.
- Do not log database connection strings.
- Use fixtures for automated tests that need market data.
- Do not call the live API repeatedly from a test suite.
