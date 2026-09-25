//! `trade/{market_id}` WebSocket channel.
//!
//! Confirmed to exist by capturing raw traffic from
//! `wss://api.rh.lighter.xyz/stream?readonly=true` on 2026-09-21 (see
//! `./todos/001-confirm-trade-ws-channel.md`) -- NOT documented in the
//! official Python SDK's `lighter.ws_client.WsClient`, which only wires up
//! `order_book/{id}` and `account_all/{id}` subscriptions. Subscribe with
//! `{"type": "subscribe", "channel": "trade/{market_id}"}`; the server
//! replies with one `subscribed/trade` message (recent history), then a
//! stream of `update/trade` messages (new trades as they happen).
//!
//! Both message types carry `trades` (regular fills) and
//! `liquidation_trades` (forced liquidations) arrays. Every field of
//! [`crate::trade::Trade`] (the REST `/api/v1/recentTrades` record) is
//! present, under the same names, on both arrays' entries -- confirmed by
//! comparing captured WS records against the REST response -- so this
//! module reuses that type rather than duplicating it. Liquidation records
//! additionally carry margin/fee diagnostics (`taker_fee`,
//! `taker_position_sign_changed`, ...) that serde silently ignores here,
//! the same as `trade.rs`'s own omissions.
//!
//! Each message also carries a `nonce`. Unlike the `order_book` channel
//! (see `orderbook.rs`), there is no paired `begin_nonce` to chain
//! against, so it cannot be used for gap detection by itself -- only for
//! detecting that two messages arrived out of order.
use crate::trade::Trade;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum TradeWsMessage {
    #[serde(rename = "subscribed/trade")]
    Subscribed {
        /// `"trade:{market_id}"` -- use [`TradeWsMessage::market_id`]
        /// rather than parsing this directly.
        channel: String,
        nonce: i64,
        trades: Vec<Trade>,
        liquidation_trades: Vec<Trade>,
    },
    #[serde(rename = "update/trade")]
    Update {
        channel: String,
        nonce: i64,
        trades: Vec<Trade>,
        liquidation_trades: Vec<Trade>,
    },
}

impl TradeWsMessage {
    /// Parses `channel.split(':')[1]`, e.g. `"trade:0"` -> `0`, the same
    /// convention [`crate::orderbook::OrderBookWsMessage::market_id`] uses.
    pub fn market_id(&self) -> Option<i32> {
        let channel = match self {
            Self::Subscribed { channel, .. } | Self::Update { channel, .. } => channel,
        };
        channel.split(':').nth(1)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_subscribed_message() {
        // Captured from wss://api.rh.lighter.xyz/stream?readonly=true on 2026-09-21.
        let msg: TradeWsMessage = serde_json::from_str(
            r#"{"type":"subscribed/trade","channel":"trade:0","nonce":2180216777,"trades":[{"trade_id":886492467,"tx_hash":"3d86ca74de66d8dc2c15ff2ee71d8204d871d74b03994cefeacd790f372bf2c8e7871a3153db402f","type":"trade","market_id":0,"size":"0.0500","price":"2749.83","usd_amount":"137.491500","ask_id":281475066736111,"bid_id":562949856754209,"ask_client_id":667509946090,"bid_client_id":36284421607817,"ask_account_id":30597,"bid_account_id":27160,"is_maker_ask":false,"block_height":25653864,"timestamp":1790015210729}],"liquidation_trades":[{"trade_id":882932008,"tx_hash":"00000000ec885d57000001a0c4cf786c000000000000000000000000000000000000000000000000","type":"liquidation","market_id":0,"size":"1.0034","price":"2769.77","usd_amount":"2779.187218","ask_id":281475066529203,"bid_id":562949857055916,"ask_client_id":238695887555499,"bid_client_id":0,"ask_account_id":7806,"bid_account_id":3220,"is_maker_ask":true,"block_height":25598797,"timestamp":1790008326252,"taker_fee":10000,"taker_position_sign_changed":true}]}"#,
        )
        .unwrap();
        assert_eq!(msg.market_id(), Some(0));
        let TradeWsMessage::Subscribed {
            trades,
            liquidation_trades,
            ..
        } = &msg
        else {
            panic!("expected Subscribed");
        };
        assert_eq!(trades[0].trade_type, "trade");
        assert_eq!(liquidation_trades[0].trade_type, "liquidation");
    }

    #[test]
    fn parses_an_update_message() {
        // Captured from wss://api.rh.lighter.xyz/stream?readonly=true on 2026-09-21.
        let msg: TradeWsMessage = serde_json::from_str(
            r#"{"type":"update/trade","channel":"trade:0","nonce":2180216806,"trades":[{"trade_id":886492488,"tx_hash":"2ee776a51acfcf70676ef6f0ac837f8498eb5414b950795d539f2ecd358602e86e07842442845218","type":"trade","market_id":0,"size":"0.2350","price":"2750.40","usd_amount":"646.344000","ask_id":281475066736107,"bid_id":562949856754201,"ask_client_id":8301417026,"bid_client_id":3231440947,"ask_account_id":2304,"bid_account_id":27972,"is_maker_ask":true,"block_height":25653865,"timestamp":1790015211009}],"liquidation_trades":[]}"#,
        )
        .unwrap();
        assert_eq!(msg.market_id(), Some(0));
        let TradeWsMessage::Update {
            nonce,
            trades,
            liquidation_trades,
            ..
        } = &msg
        else {
            panic!("expected Update");
        };
        assert_eq!(*nonce, 2180216806);
        assert_eq!(trades[0].price, "2750.40");
        assert!(liquidation_trades.is_empty());
    }
}
