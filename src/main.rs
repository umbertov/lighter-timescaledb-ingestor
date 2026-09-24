//! Lighter (Robinhood Chain) market-data ingestor.
//!
//! Symbol sync (`/api/v1/orderBooks` -> `symbols` table) runs once at
//! startup, then WebSocket ingestion runs for every perp and spot market.
use clap::Parser;
use color_eyre::eyre::{eyre, Result, WrapErr};
use diesel::{Connection, PgConnection};
use lighter_rs_types::OrderBooksResponse;
use lighter_timescaledb_rs::ws::{self, WriteMsg};
use mimalloc::MiMalloc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const RECONNECT_DELAY: Duration = Duration::from_millis(2000);
const WRITE_CHANNEL_CAPACITY: usize = 1024;

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

    /// Lighter WS stream base URL. Defaults to Robinhood Chain, read-only mode.
    #[arg(
        long,
        env = "LIGHTER_WS_URL",
        default_value = "wss://api.rh.lighter.xyz/stream?readonly=true"
    )]
    ws_url: String,
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

    let symbol_cache =
        lighter_timescaledb_rs::load_symbol_cache(&mut conn).wrap_err("loading symbol cache")?;
    let market_ids: Vec<i32> = symbol_cache.keys().copied().collect();
    info!("loaded {} markets for WS ingestion", market_ids.len());

    let pool = lighter_timescaledb_rs::establish_postgres_connection_pool(&database_url)
        .wrap_err("establishing connection pool")?;
    let (tx, rx) = mpsc::channel::<WriteMsg>(WRITE_CHANNEL_CAPACITY);
    tokio::spawn(ws::writer_task(rx, pool));

    loop {
        if let Err(e) = ws::run_ws_ingestion(&args.ws_url, &market_ids, &symbol_cache, &tx).await {
            error!("ws ingestion died: {e}, reconnecting in {RECONNECT_DELAY:?}");
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    }
}

/// Fetches every market from `/api/v1/orderBooks` and upserts it into the
/// `symbols` table, keyed by Lighter's own `market_id`.
async fn sync_symbols(api_url: &str, conn: &mut PgConnection) -> Result<()> {
    let http = reqwest::Client::new();
    let resp: OrderBooksResponse = http
        .get(format!("{api_url}/api/v1/orderBooks"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    if resp.code != 200 {
        return Err(eyre!("orderBooks returned code {}", resp.code));
    }
    if resp.order_books.is_empty() {
        return Err(eyre!("orderBooks returned no markets"));
    }

    if let Some(market) = resp
        .order_books
        .iter()
        .find(|market| !matches!(market.market_type.as_str(), "perp" | "spot"))
    {
        return Err(eyre!(
            "unsupported market type {:?} for market_id {}",
            market.market_type,
            market.market_id
        ));
    }

    conn.transaction::<_, color_eyre::eyre::Report, _>(|conn| {
        for market in &resp.order_books {
            let id = lighter_timescaledb_rs::upsert_symbol(
                &market.symbol,
                &market.market_type,
                market.market_id,
                conn,
            )?;
            info!(
                "synced {} market {} (lighter market_id={}, db id={id})",
                market.market_type, market.symbol, market.market_id
            );
        }
        Ok(())
    })?;

    info!("synced {} markets from {api_url}", resp.order_books.len());
    Ok(())
}
