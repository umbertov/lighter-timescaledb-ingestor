use diesel::prelude::*;

use crate::schema::{orderbook_messages, symbols, trades};

#[derive(Queryable, Selectable, QueryableByName, Debug)]
#[diesel(table_name = symbols)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Symbol {
    pub id: i32,
    pub name: String,
    pub lighter_market_id: i32,
}

#[derive(Insertable)]
#[diesel(table_name = symbols)]
pub struct NewSymbol<'a> {
    pub name: &'a str,
    pub lighter_market_id: i32,
}

#[derive(Queryable, Selectable, Insertable, Debug)]
#[diesel(table_name = trades)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TradeRow {
    pub time: chrono::DateTime<chrono::Utc>,
    pub symbol: i32,
    pub lighter_trade_id: i64,
    pub price: f64,
    pub size: f64,
    pub is_maker_ask: bool,
    pub trade_type: String,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = orderbook_messages)]
pub struct OrderbookMessageRow {
    pub time: chrono::DateTime<chrono::Utc>,
    pub symbol: i32,
    pub message_type: String,
    pub sequence: Option<i64>,
    pub bids: serde_json::Value,
    pub asks: serde_json::Value,
}
