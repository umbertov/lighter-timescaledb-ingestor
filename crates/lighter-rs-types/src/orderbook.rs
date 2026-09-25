//! `order_book/{market_id}` WebSocket channel.
//!
//! Verified against the official Python SDK's `lighter.ws_client.WsClient`
//! (`mm-lighter-py/.venv/.../site-packages/lighter/ws_client.py` on
//! umbertov's machine): subscribe with
//! `{"type": "subscribe", "channel": "order_book/{market_id}"}`; the server
//! replies with one `subscribed/order_book` message (full snapshot), then a
//! stream of `update/order_book` messages (deltas). Both carry an
//! `order_book: {asks: [...], bids: [...]}` payload of the same shape.
//!
//! A level's `size == "0"` means "remove this price level" -- there is no
//! separate delete message type, per the reference SDK's own reconciliation
//! logic (`WsClient.update_orders`, a naive price-keyed merge).
//!
//! Gap detection (see `./todos/002-confirm-orderbook-sequence-numbers.md`):
//! confirmed by capturing raw traffic from
//! `wss://api.rh.lighter.xyz/stream?readonly=true` on 2026-09-21 (not
//! documented in either SDK). Each `order_book` payload carries `nonce` and
//! `begin_nonce`. Every observed message's `begin_nonce` equals the
//! previous message's `nonce` for that market, forming an unbroken chain:
//! a client must hold the last-seen `nonce` and reject (drop-and-resubscribe)
//! any message whose `begin_nonce` does not match it. `offset` is also
//! present and strictly increasing, but observed increments are uneven, so
//! it is exposed as a supplementary ordering signal, not a substitute for
//! the nonce chain. The `code` field (always `0` in captures) has unknown
//! semantics and is deliberately omitted.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum OrderBookWsMessage {
    #[serde(rename = "subscribed/order_book")]
    Subscribed {
        /// `"order_book:{market_id}"` -- use [`OrderBookWsMessage::market_id`]
        /// rather than parsing this directly.
        channel: String,
        /// Unix microseconds, exchange-side. Not in either SDK -- confirmed
        /// present on every captured payload.
        last_updated_at: i64,
        order_book: OrderBookSideLevels,
    },
    #[serde(rename = "update/order_book")]
    Update {
        channel: String,
        /// Unix microseconds, exchange-side. Not in either SDK -- confirmed
        /// present on every captured payload.
        last_updated_at: i64,
        order_book: OrderBookSideLevels,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookSideLevels {
    /// Strictly increasing per market, but not necessarily by a fixed
    /// step -- see the module doc comment. Prefer `nonce`/`begin_nonce` for
    /// gap detection.
    pub offset: i64,
    /// This message's sequence position. The next message for this market
    /// must carry this value as its `begin_nonce`.
    pub nonce: i64,
    /// Must equal the previous message's `nonce` for this market. A
    /// mismatch means at least one message was dropped.
    pub begin_nonce: i64,
    pub asks: Vec<PriceLevel>,
    pub bids: Vec<PriceLevel>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PriceLevel {
    pub price: String,
    pub size: String,
}

impl OrderBookWsMessage {
    /// Parses `channel.split(':')[1]`, e.g. `"order_book:4095"` -> `4095`,
    /// the same way the reference SDK does.
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
        let msg: OrderBookWsMessage = serde_json::from_str(
            r#"{"type":"subscribed/order_book","channel":"order_book:0","offset":30563230,"last_updated_at":1790015233367788,"order_book":{"code":0,"offset":30563230,"nonce":2180232924,"begin_nonce":0,"asks":[{"price":"2750.29","size":"0.3778"}],"bids":[{"price":"2750.03","size":"0.3778"}]}}"#,
        )
        .unwrap();
        assert_eq!(msg.market_id(), Some(0));
        let OrderBookWsMessage::Subscribed {
            order_book,
            last_updated_at,
            ..
        } = &msg
        else {
            panic!("expected Subscribed");
        };
        assert_eq!(order_book.asks[0].price, "2750.29");
        assert_eq!(order_book.nonce, 2180232924);
        assert_eq!(*last_updated_at, 1790015233367788);
    }

    #[test]
    fn parses_an_update_message_with_a_zero_size_removal() {
        // Captured from wss://api.rh.lighter.xyz/stream?readonly=true on 2026-09-21.
        let msg: OrderBookWsMessage = serde_json::from_str(
            r#"{"type":"update/order_book","channel":"order_book:0","offset":30563238,"last_updated_at":1790015233665075,"order_book":{"code":0,"offset":30563238,"nonce":2180233131,"begin_nonce":2180233095,"asks":[{"price":"2755.52","size":"0"}],"bids":[]}}"#,
        )
        .unwrap();
        assert_eq!(msg.market_id(), Some(0));
        let OrderBookWsMessage::Update {
            order_book,
            last_updated_at,
            ..
        } = &msg
        else {
            panic!("expected Update");
        };
        assert_eq!(order_book.asks[0].size, "0");
        assert_eq!(order_book.begin_nonce, 2180233095);
        assert_eq!(*last_updated_at, 1790015233665075);
    }

    #[test]
    fn consecutive_updates_chain_by_nonce() {
        // begin_nonce of one message must equal nonce of the previous one for
        // the same market -- the basis of gap detection.
        let first: OrderBookWsMessage = serde_json::from_str(
            r#"{"type":"update/order_book","channel":"order_book:0","offset":30563234,"last_updated_at":0,"order_book":{"code":0,"offset":30563234,"nonce":2180233095,"begin_nonce":2180233011,"asks":[],"bids":[]}}"#,
        )
        .unwrap();
        let second: OrderBookWsMessage = serde_json::from_str(
            r#"{"type":"update/order_book","channel":"order_book:0","offset":30563238,"last_updated_at":0,"order_book":{"code":0,"offset":30563238,"nonce":2180233131,"begin_nonce":2180233095,"asks":[],"bids":[]}}"#,
        )
        .unwrap();
        let (
            OrderBookWsMessage::Update {
                order_book: first, ..
            },
            OrderBookWsMessage::Update {
                order_book: second, ..
            },
        ) = (&first, &second)
        else {
            panic!("expected Update");
        };
        assert_eq!(first.nonce, second.begin_nonce);
    }
}
