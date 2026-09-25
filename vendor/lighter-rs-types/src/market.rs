//! `/api/v1/orderBooks` market metadata.
//!
//! Fields cross-checked against the official Python SDK's
//! `lighter.models.order_book.OrderBook` (installed at
//! `mm-lighter-py/.venv/lib/python3.12/site-packages/lighter/models/order_book.py`
//! on umbertov's machine) plus a live response from
//! `https://api.rh.lighter.xyz/api/v1/orderBooks` (checked 2026-09-21), which
//! returns a few fields that SDK version doesn't model
//! (`is_taker_fee_enabled`, `is_maker_fee_enabled`, `created_at`,
//! `multiplier`) -- omitted here as not needed by either consumer today.
//! Both sources agree on every field kept below.
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBooksResponse {
    pub code: i64,
    pub order_books: Vec<OrderBookInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookInfo {
    pub symbol: String,
    pub market_id: i32,
    /// `"perp"` | `"spot"`.
    pub market_type: String,
    pub base_asset_id: i32,
    pub quote_asset_id: i32,
    /// `"active"` | `"inactive"`.
    pub status: String,
    /// Decimal string, e.g. `"0.0000"`. Robinhood Chain's Standard-account
    /// fee tier is confirmed 0% maker/taker across all markets (checked
    /// live) -- the reason this exchange integration exists at all.
    pub taker_fee: String,
    pub maker_fee: String,
    pub liquidation_fee: String,
    pub min_base_amount: String,
    pub min_quote_amount: String,
    pub order_quote_limit: String,
    /// Scale for the wire-format integer `price`/`base_amount` fields used
    /// when signing an order transaction, i.e.
    /// `wire_price = round(price * 10^supported_price_decimals)`. This is
    /// the standard convention for this class of DEX API, but has not been
    /// confirmed against a live signed-and-accepted order -- see mm-rs's
    /// `LighterConnector` doc comments.
    pub supported_size_decimals: u32,
    pub supported_price_decimals: u32,
    pub supported_quote_decimals: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_live_order_books_response() {
        // Captured from https://api.rh.lighter.xyz/api/v1/orderBooks on 2026-09-21,
        // trimmed to one entry (extra fields like is_taker_fee_enabled, created_at,
        // and multiplier are present live but intentionally not modeled -- serde
        // ignores them).
        let resp: OrderBooksResponse = serde_json::from_str(
            r#"{"code":200,"order_books":[{"symbol":"QBTS","market_id":51,"market_type":"perp","base_asset_id":0,"quote_asset_id":0,"status":"active","taker_fee":"0.0000","is_taker_fee_enabled":true,"maker_fee":"0.0000","is_maker_fee_enabled":true,"liquidation_fee":"1.0000","min_base_amount":"0.500","min_quote_amount":"10.000000","order_quote_limit":"25000000.000000","supported_size_decimals":3,"supported_price_decimals":3,"supported_quote_decimals":6,"created_at":"1788529452738","multiplier":"1.000000000000000000"}]}"#,
        )
        .unwrap();
        assert_eq!(resp.code, 200);
        assert_eq!(resp.order_books.len(), 1);
        let market = &resp.order_books[0];
        assert_eq!(market.symbol, "QBTS");
        assert_eq!(market.market_id, 51);
        assert_eq!(market.market_type, "perp");
        assert_eq!(market.status, "active");
        assert_eq!(market.supported_price_decimals, 3);
    }
}
