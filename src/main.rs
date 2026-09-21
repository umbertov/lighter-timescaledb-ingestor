//! Lighter (Robinhood Chain) market-data ingestor.
//!
//! Status: symbol sync (`/api/v1/orderBooks` -> `symbols` table) is real
//! and works end to end. The actual streaming ingestion (order-book WS,
//! REST-polled trades) is NOT implemented yet -- see `./todos/` for why
//! and what's blocking it. Running this binary today only populates
//! `symbols`; it does not yet write to `trades` or `orderbook_messages`.
use clap::Parser;
use color_eyre::eyre::{Result, WrapErr};
use diesel::PgConnection;
use lighter_rs_types::OrderBooksResponse;
use mimalloc::MiMalloc;
use tracing::info;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Parser, Debug)]
struct IngestorConfig {
    /// Lighter REST API base URL. Defaults to Robinhood Chain; pass
    /// https://testnet.zklighter.elliot.ai for the zkLighter testnet.
    #[arg(
        long,
        env = "LIGHTER_API_URL",
        default_value = "https://api.rh.lighter.xyz"
    )]
    api_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();
    color_eyre::install()?;

    let database_url = std::env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?;
    let args = IngestorConfig::parse();

    let mut conn = lighter_timescaledb_rs::establish_postgres_connection(&database_url)
        .wrap_err("connecting to postgres")?;

    sync_symbols(&args.api_url, &mut conn).await?;

    info!(
        "symbol sync done. streaming ingestion (trades/orderbook) is not \
         implemented yet -- see ./todos/"
    );
    Ok(())
}

/// Fetches every market from `/api/v1/orderBooks` and upserts it into the
/// `symbols` table, keyed by Lighter's own `market_id`.
async fn sync_symbols(api_url: &str, conn: &mut PgConnection) -> Result<()> {
    let http = reqwest::Client::new();
    let resp: OrderBooksResponse = http
        .get(format!("{api_url}/api/v1/orderBooks"))
        .send()
        .await?
        .json()
        .await?;

    for market in &resp.order_books {
        let id =
            lighter_timescaledb_rs::get_or_upsert_symbol(&market.symbol, market.market_id, conn)?;
        info!(
            "synced symbol {} (lighter market_id={}, db id={id})",
            market.symbol, market.market_id
        );
    }

    info!("synced {} markets from {api_url}", resp.order_books.len());
    Ok(())
}
