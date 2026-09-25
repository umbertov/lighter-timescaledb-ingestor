//! `/api/v1/recentTrades` response.
//!
//! A WebSocket push channel for trades does exist, despite not being
//! wired up in the official Python SDK's `WsClient`
//! (`lighter.ws_client.WsClient.handle_connected` only subscribes
//! `order_book/{id}` and `account_all/{id}`) -- confirmed by capturing raw
//! traffic from `wss://api.rh.lighter.xyz/stream?readonly=true` on
//! 2026-09-21. See `trade_ws.rs`, which reuses this module's [`Trade`]
//! type for the WS record shape.
//!
//! Field list cross-checked against the official SDK's
//! `lighter.models.trade.Trade` and a live response from
//! `https://api.rh.lighter.xyz/api/v1/recentTrades` (checked 2026-09-21).
//! Several fields present in both sources are intentionally omitted here as
//! margin/position diagnostics not needed by either consumer today
//! (`taker_position_size_before`, `maker_entry_quote_before`, etc.) --
//! serde ignores unknown fields by default, so adding them later is
//! additive, not breaking.
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TradesResponse {
    pub code: i64,
    pub trades: Vec<Trade>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Trade {
    pub trade_id: i64,
    pub tx_hash: String,
    #[serde(rename = "type")]
    pub trade_type: String,
    pub market_id: i32,
    pub size: String,
    pub price: String,
    pub usd_amount: String,
    pub ask_id: i64,
    pub bid_id: i64,
    pub ask_client_id: i64,
    pub bid_client_id: i64,
    pub ask_account_id: i64,
    pub bid_account_id: i64,
    pub is_maker_ask: bool,
    pub block_height: i64,
    /// Unix milliseconds.
    pub timestamp: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_live_recent_trades_response() {
        // Captured from https://api.rh.lighter.xyz/api/v1/recentTrades?market_id=0&limit=2 on 2026-09-21.
        let resp: TradesResponse = serde_json::from_str(
            r#"{"code":200,"trades":[{"trade_id":886868782,"tx_hash":"00000000ed3647ac000001a0c544d07c000000000000000000000000000000000000000000000000","type":"trade","market_id":0,"size":"0.2500","price":"2751.72","usd_amount":"687.930000","ask_id":281475066754794,"bid_id":562949856726542,"ask_client_id":2501872167,"bid_client_id":439876680208,"ask_account_id":2267,"bid_account_id":9235,"is_maker_ask":true,"block_height":25659845,"timestamp":1790016016508},{"trade_id":886866793,"tx_hash":"00000000ed363043000001a0c544bc7c000000000000000000000000000000000000000000000000","type":"trade","market_id":0,"size":"0.0003","price":"2751.13","usd_amount":"0.825339","ask_id":281475066754769,"bid_id":562949856726646,"ask_client_id":0,"bid_client_id":1786602595555,"ask_account_id":384,"bid_account_id":1183,"is_maker_ask":false,"block_height":25659812,"timestamp":1790016011388}]}"#,
        )
        .unwrap();
        assert_eq!(resp.code, 200);
        assert_eq!(resp.trades.len(), 2);
        assert_eq!(resp.trades[0].trade_id, 886868782);
        assert_eq!(resp.trades[0].trade_type, "trade");
        assert_eq!(resp.trades[0].price, "2751.72");
        assert!(resp.trades[0].is_maker_ask);
    }
}
