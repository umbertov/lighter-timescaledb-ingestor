use color_eyre::eyre::Result;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use std::collections::HashMap;
use tracing::info;

use models::NewSymbol;

pub mod models;
pub mod schema;
pub mod ws;

/// Upserts one market by Lighter's stable market ID.
pub fn upsert_symbol(
    symbol_name: &str,
    market_type_value: &str,
    market_id: i32,
    conn: &mut PgConnection,
) -> Result<i32> {
    use crate::schema::symbols::dsl::{id, lighter_market_id, market_type, name, symbols};

    let record = NewSymbol {
        name: symbol_name,
        market_type: market_type_value,
        lighter_market_id: market_id,
    };
    diesel::insert_into(symbols)
        .values(&record)
        .on_conflict(lighter_market_id)
        .do_update()
        .set((name.eq(symbol_name), market_type.eq(market_type_value)))
        .returning(id)
        .get_result(conn)
        .map_err(|e| {
            tracing::error!("upsert symbol {symbol_name} (market_id {market_id}) failed: {e}");
            e.into()
        })
}

pub fn insert_trades(rows: &[models::TradeRow], conn: &mut PgConnection) -> Result<usize> {
    use crate::schema::trades;

    Ok(diesel::insert_into(trades::table)
        .values(rows)
        .on_conflict_do_nothing()
        .execute(conn)?)
}

pub fn insert_orderbook_messages(
    rows: &[models::OrderbookMessageRow],
    conn: &mut PgConnection,
) -> Result<usize> {
    use crate::schema::orderbook_messages;

    Ok(diesel::insert_into(orderbook_messages::table)
        .values(rows)
        .execute(conn)?)
}

/// Creates an r2d2 connection pool to the given database.
pub fn establish_postgres_connection_pool(
    database_url: &str,
) -> Result<Pool<ConnectionManager<PgConnection>>> {
    info!("connecting to {database_url}");
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    Ok(Pool::builder()
        .test_on_check_out(true)
        .max_size(2)
        .build(manager)?)
}

pub fn establish_postgres_connection(database_url: &str) -> Result<PgConnection> {
    info!("connecting to {database_url}");
    Ok(PgConnection::establish(database_url)?)
}

/// Maps Lighter's own `market_id` to this database's local `symbols.id`.
/// Loaded once at startup: the full symbol set is already known after
/// `sync_symbols` runs, so there is no lazy DB fallback on a cache miss.
pub fn load_symbol_cache(conn: &mut PgConnection) -> Result<HashMap<i32, i32>> {
    use crate::schema::symbols::dsl::{id, lighter_market_id, symbols};

    Ok(symbols
        .select((lighter_market_id, id))
        .load::<(i32, i32)>(conn)?
        .into_iter()
        .collect())
}
