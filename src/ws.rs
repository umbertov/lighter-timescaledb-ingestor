//! `order_book/{market_id}` and `trade/{market_id}` WS ingestion.
//!
//! One connection subscribes to every market's `order_book` and `trade`
//! channels (168 subscriptions for 84 markets today -- no documented
//! per-connection subscription limit, and sharding across connections
//! later only needs a different `market_ids` slice). On any order-book
//! nonce gap, [`run_ws_ingestion`] returns an error: the caller
//! reconnects and resubscribes everything, the same coarse-grained
//! "kill the connection and restart" recovery
//! `bybit-timescaledb-ingestor`'s `crawler_thread` uses, rather than an
//! in-stream resync.
use crate::book::Book;
use crate::models::{OrderbookMessageRow, TradeRow};
use crate::{insert_orderbook_messages, insert_trades};
use chrono::{DateTime, Utc};
use color_eyre::eyre::{eyre, Result, WrapErr};
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::PgConnection;
use futures::{SinkExt, StreamExt};
use lighter_rs_types::orderbook::{OrderBookSideLevels, OrderBookWsMessage};
use lighter_rs_types::trade::Trade;
use lighter_rs_types::trade_ws::TradeWsMessage;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

const BATCH_CAPACITY: usize = 128;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
/// The server's reverse proxy enforces a read deadline on the
/// client->server direction: confirmed by capturing repeated
/// disconnects at almost exactly 120s after subscribing, each closed
/// normally with a `read tcp ...: i/o timeout` reason naming the
/// proxy's own read of our connection, with zero server-sent `ping`
/// messages observed in between (ruling out the reference Python SDK's
/// `ping`/`pong` JSON exchange as the actual mechanism here -- that
/// code path is defensive, not what this deployment relies on). A
/// client-initiated WS ping frame well under that window keeps the
/// proxy's read timer from firing.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

pub enum WriteMsg {
    Trade(TradeRow),
    Orderbook(OrderbookMessageRow),
}

/// Batches incoming rows and flushes them to Postgres on a capacity or
/// time trigger. Exits the process on a write failure: a silently
/// dropped batch is a correctness bug, not a condition to paper over.
pub async fn writer_task(
    mut rx: mpsc::Receiver<WriteMsg>,
    pool: Pool<ConnectionManager<PgConnection>>,
) {
    let mut batch: Vec<WriteMsg> = Vec::with_capacity(BATCH_CAPACITY);
    let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
    ticker.tick().await; // first tick fires immediately; discard it

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(msg) => {
                        batch.push(msg);
                        if batch.len() >= BATCH_CAPACITY {
                            flush(&mut batch, &pool).await;
                        }
                    }
                    None => {
                        flush(&mut batch, &pool).await;
                        info!("writer_task: channel closed, exiting");
                        return;
                    }
                }
            }
            _ = ticker.tick() => {
                flush(&mut batch, &pool).await;
            }
        }
    }
}

async fn flush(batch: &mut Vec<WriteMsg>, pool: &Pool<ConnectionManager<PgConnection>>) {
    if batch.is_empty() {
        return;
    }
    let drained: Vec<WriteMsg> = std::mem::take(batch);
    let mut trade_rows = Vec::new();
    let mut orderbook_rows = Vec::new();
    for msg in drained {
        match msg {
            WriteMsg::Trade(row) => trade_rows.push(row),
            WriteMsg::Orderbook(row) => orderbook_rows.push(row),
        }
    }

    let pool = pool.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut conn = pool.get().wrap_err("getting a pooled connection")?;
        if !trade_rows.is_empty() {
            insert_trades(&trade_rows, &mut conn).wrap_err("inserting trade rows")?;
        }
        if !orderbook_rows.is_empty() {
            insert_orderbook_messages(&orderbook_rows, &mut conn)
                .wrap_err("inserting orderbook rows")?;
        }
        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            error!("writer_task: flush failed, exiting: {e}");
            std::process::exit(1);
        }
        Err(e) => {
            error!("writer_task: flush task panicked, exiting: {e}");
            std::process::exit(1);
        }
    }
}

/// Runs one WS connection end to end: subscribes to every market's
/// `order_book` and `trade` channels, then reads until an error (a
/// nonce gap, a parse failure, or the connection closing) ends the
/// loop. Returns `Err` in every such case -- the caller is expected to
/// reconnect from scratch.
pub async fn run_ws_ingestion(
    ws_url: &str,
    market_ids: &[i32],
    symbol_cache: &HashMap<i32, i32>,
    snapshot_interval: Duration,
    tx: &mpsc::Sender<WriteMsg>,
) -> Result<()> {
    let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .wrap_err("connecting to the market-data WebSocket")?;
    info!("connected to the market-data WebSocket");
    let (mut write, mut read) = ws_stream.split();

    // The official Python client sends subscriptions without a delay.
    // Per-channel pacing added about 17 seconds to each reconnect for 84 markets.
    for market_id in market_ids {
        for channel in [
            format!("order_book/{market_id}"),
            format!("trade/{market_id}"),
        ] {
            let sub = serde_json::json!({"type": "subscribe", "channel": channel});
            write
                .send(Message::Text(sub.to_string()))
                .await
                .wrap_err_with(|| format!("subscribing to {channel}"))?;
        }
    }
    info!(
        "sent order_book and trade subscriptions for {} markets",
        market_ids.len()
    );

    // Fresh on every reconnect: a gap detected against a nonce from a
    // previous connection would be meaningless, since the server just
    // sent a brand-new snapshot for every market.
    let mut markets: HashMap<i32, MarketBook> = HashMap::new();

    let mut snapshot_timer = tokio::time::interval(snapshot_interval);
    snapshot_timer.tick().await; // first tick fires immediately; discard it

    let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive.tick().await; // first tick fires immediately; discard it

    loop {
        tokio::select! {
            frame = read.next() => {
                let Some(frame) = frame else {
                    return Err(eyre!("ws stream ended"));
                };
                let frame = frame.wrap_err("ws read error")?;
                let text = match frame {
                    Message::Text(t) => t,
                    Message::Close(reason) => return Err(eyre!("ws closed by server: {reason:?}")),
                    _ => continue,
                };

                let value: serde_json::Value = serde_json::from_str(&text)
                    .wrap_err_with(|| format!("parsing ws frame: {text}"))?;
                let ty = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

                if ty.ends_with("order_book") {
                    handle_orderbook_frame(value, symbol_cache, &mut markets, tx).await?;
                } else if ty.ends_with("trade") {
                    handle_trade_frame(value, symbol_cache, tx).await?;
                } else if ty == "ping" {
                    // Defensive: matches the reference Python SDK's
                    // `WsClient.on_message` ping/pong handling, though
                    // this deployment was never observed to send one --
                    // see KEEPALIVE_INTERVAL for the mechanism that
                    // actually keeps the connection alive.
                    write
                        .send(Message::Text(serde_json::json!({"type": "pong"}).to_string()))
                        .await
                        .wrap_err("sending pong")?;
                } else {
                    debug!("unhandled ws frame type {ty:?}: {text}");
                }
            }
            _ = snapshot_timer.tick() => {
                write_periodic_snapshots(&markets, symbol_cache, tx).await?;
            }
            _ = keepalive.tick() => {
                write.send(Message::Ping(Vec::new())).await.wrap_err("sending keepalive ping")?;
            }
        }
    }
}

/// The book and nonce of one market in the current connection.
pub struct MarketBook {
    pub nonce: i64,
    pub book: Book,
}

/// Writes one `snapshot` row for each market with a book, from the book
/// after the last applied delta. Its `time` is the write time; readers
/// must order rows of one connection by `sequence`.
async fn write_periodic_snapshots(
    markets: &HashMap<i32, MarketBook>,
    symbol_cache: &HashMap<i32, i32>,
    tx: &mpsc::Sender<WriteMsg>,
) -> Result<()> {
    let now = Utc::now();
    for (market_id, market) in markets {
        let symbol = *symbol_cache
            .get(market_id)
            .ok_or_else(|| eyre!("book for unknown market_id {market_id}"))?;
        let row = book_snapshot_row(symbol, now, market)?;
        tx.send(WriteMsg::Orderbook(row))
            .await
            .map_err(|_| eyre!("writer_task channel closed"))?;
    }
    debug!("wrote periodic snapshots for {} markets", markets.len());
    Ok(())
}

pub fn book_snapshot_row(
    symbol: i32,
    time: DateTime<Utc>,
    market: &MarketBook,
) -> Result<OrderbookMessageRow> {
    Ok(OrderbookMessageRow {
        time,
        symbol,
        message_type: "snapshot".to_string(),
        sequence: Some(market.nonce),
        bids: serde_json::to_value(market.book.bids()).wrap_err("serializing bids")?,
        asks: serde_json::to_value(market.book.asks()).wrap_err("serializing asks")?,
        begin_nonce: None,
    })
}

async fn handle_orderbook_frame(
    value: serde_json::Value,
    symbol_cache: &HashMap<i32, i32>,
    markets: &mut HashMap<i32, MarketBook>,
    tx: &mpsc::Sender<WriteMsg>,
) -> Result<()> {
    let parsed: OrderBookWsMessage =
        serde_json::from_value(value).wrap_err("deserializing order_book message")?;
    let market_id = parsed
        .market_id()
        .ok_or_else(|| eyre!("order_book message missing a parseable market_id"))?;
    let Some(&symbol) = symbol_cache.get(&market_id) else {
        warn!("order_book message for unknown market_id {market_id}, skipping");
        return Ok(());
    };

    let row = apply_orderbook_message(symbol, market_id, parsed, markets)?;
    tx.send(WriteMsg::Orderbook(row))
        .await
        .map_err(|_| eyre!("writer_task channel closed"))
}

/// Applies one order_book message to the market's book and returns the row
/// to store. A snapshot replaces the book. A delta must continue the nonce
/// chain, or the call fails.
pub fn apply_orderbook_message(
    symbol: i32,
    market_id: i32,
    message: OrderBookWsMessage,
    markets: &mut HashMap<i32, MarketBook>,
) -> Result<OrderbookMessageRow> {
    match message {
        OrderBookWsMessage::Subscribed {
            last_updated_at,
            order_book,
            ..
        } => {
            let book = Book::from_snapshot(&order_book.bids, &order_book.asks)
                .wrap_err_with(|| format!("building book for market_id {market_id}"))?;
            markets.insert(
                market_id,
                MarketBook {
                    nonce: order_book.nonce,
                    book,
                },
            );
            orderbook_row(symbol, "snapshot", None, last_updated_at, &order_book)
        }
        OrderBookWsMessage::Update {
            last_updated_at,
            order_book,
            ..
        } => {
            let Some(market) = markets.get_mut(&market_id) else {
                return Err(eyre!(
                    "order_book delta arrived before snapshot for market_id {market_id}"
                ));
            };
            if order_book.begin_nonce != market.nonce {
                return Err(eyre!(
                    "order_book gap for market_id {market_id}: begin_nonce {} != last nonce {}",
                    order_book.begin_nonce,
                    market.nonce
                ));
            }
            market
                .book
                .apply_delta(&order_book.bids, &order_book.asks)
                .wrap_err_with(|| format!("applying delta for market_id {market_id}"))?;
            market.nonce = order_book.nonce;
            orderbook_row(
                symbol,
                "delta",
                Some(order_book.begin_nonce),
                last_updated_at,
                &order_book,
            )
        }
    }
}

fn orderbook_row(
    symbol: i32,
    message_type: &str,
    begin_nonce: Option<i64>,
    last_updated_at: i64,
    order_book: &OrderBookSideLevels,
) -> Result<OrderbookMessageRow> {
    Ok(OrderbookMessageRow {
        time: micros_to_datetime(last_updated_at)?,
        symbol,
        message_type: message_type.to_string(),
        sequence: Some(order_book.nonce),
        bids: serde_json::to_value(&order_book.bids).wrap_err("serializing bids")?,
        asks: serde_json::to_value(&order_book.asks).wrap_err("serializing asks")?,
        begin_nonce,
    })
}

async fn handle_trade_frame(
    value: serde_json::Value,
    symbol_cache: &HashMap<i32, i32>,
    tx: &mpsc::Sender<WriteMsg>,
) -> Result<()> {
    let parsed: TradeWsMessage =
        serde_json::from_value(value).wrap_err("deserializing trade message")?;
    let market_id = parsed
        .market_id()
        .ok_or_else(|| eyre!("trade message missing a parseable market_id"))?;
    let Some(&symbol) = symbol_cache.get(&market_id) else {
        warn!("trade message for unknown market_id {market_id}, skipping");
        return Ok(());
    };

    let (trades, liquidation_trades) = match &parsed {
        TradeWsMessage::Subscribed {
            trades,
            liquidation_trades,
            ..
        }
        | TradeWsMessage::Update {
            trades,
            liquidation_trades,
            ..
        } => (trades, liquidation_trades),
    };

    for trade in trades.iter().chain(liquidation_trades.iter()) {
        let row = trade_row(trade, symbol)?;
        tx.send(WriteMsg::Trade(row))
            .await
            .map_err(|_| eyre!("writer_task channel closed"))?;
    }
    Ok(())
}

fn trade_row(trade: &Trade, symbol: i32) -> Result<TradeRow> {
    Ok(TradeRow {
        time: millis_to_datetime(trade.timestamp)?,
        symbol,
        lighter_trade_id: trade.trade_id,
        price: trade
            .price
            .parse()
            .wrap_err_with(|| format!("parsing trade price {:?}", trade.price))?,
        size: trade
            .size
            .parse()
            .wrap_err_with(|| format!("parsing trade size {:?}", trade.size))?,
        is_maker_ask: trade.is_maker_ask,
        trade_type: trade.trade_type.clone(),
    })
}

fn micros_to_datetime(micros: i64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp_micros(micros)
        .ok_or_else(|| eyre!("invalid timestamp (unix micros): {micros}"))
}

fn millis_to_datetime(millis: i64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp_millis(millis)
        .ok_or_else(|| eyre!("invalid timestamp (unix millis): {millis}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(json: &str) -> OrderBookWsMessage {
        serde_json::from_str(json).unwrap()
    }

    fn snapshot(nonce: i64, bids: &str, asks: &str) -> OrderBookWsMessage {
        msg(&format!(
            r#"{{"type":"subscribed/order_book","channel":"order_book:7","offset":1,"last_updated_at":1790015233367788,"order_book":{{"code":0,"offset":1,"nonce":{nonce},"begin_nonce":0,"asks":{asks},"bids":{bids}}}}}"#
        ))
    }

    fn delta(begin: i64, nonce: i64, bids: &str, asks: &str) -> OrderBookWsMessage {
        msg(&format!(
            r#"{{"type":"update/order_book","channel":"order_book:7","offset":2,"last_updated_at":1790015233367999,"order_book":{{"code":0,"offset":2,"nonce":{nonce},"begin_nonce":{begin},"asks":{asks},"bids":{bids}}}}}"#
        ))
    }

    #[test]
    fn rows_store_begin_nonce_and_snapshot_equals_book_after_last_delta() {
        let mut markets = HashMap::new();
        let snap = apply_orderbook_message(
            3,
            7,
            snapshot(
                10,
                r#"[{"price":"217.71","size":"1.0000"},{"price":"217.69","size":"1"}]"#,
                r#"[{"price":"217.80","size":"2"}]"#,
            ),
            &mut markets,
        )
        .unwrap();
        assert_eq!(snap.begin_nonce, None);
        assert_eq!(snap.sequence, Some(10));

        let d = apply_orderbook_message(
            3,
            7,
            delta(
                10,
                15,
                r#"[{"price":"217.71","size":"0.0000"},{"price":"217.70","size":"0.0460"}]"#,
                r#"[{"price":"217.80","size":"0"},{"price":"217.85","size":"4"}]"#,
            ),
            &mut markets,
        )
        .unwrap();
        assert_eq!(d.message_type, "delta");
        assert_eq!(d.begin_nonce, Some(10));
        assert_eq!(d.sequence, Some(15));

        let periodic = book_snapshot_row(3, Utc::now(), &markets[&7]).unwrap();
        assert_eq!(periodic.message_type, "snapshot");
        assert_eq!(periodic.sequence, Some(15));
        assert_eq!(periodic.begin_nonce, None);
        assert_eq!(
            periodic.bids,
            serde_json::json!([{"price":"217.70","size":"0.0460"},{"price":"217.69","size":"1"}])
        );
        assert_eq!(
            periodic.asks,
            serde_json::json!([{"price":"217.85","size":"4"}])
        );
    }

    #[test]
    fn gap_fails_and_leaves_the_book_unchanged() {
        let mut markets = HashMap::new();
        apply_orderbook_message(
            3,
            7,
            snapshot(10, r#"[{"price":"1","size":"1"}]"#, "[]"),
            &mut markets,
        )
        .unwrap();
        let err = apply_orderbook_message(
            3,
            7,
            delta(11, 12, r#"[{"price":"1","size":"0"}]"#, "[]"),
            &mut markets,
        );
        assert!(err.is_err());
        assert_eq!(markets[&7].nonce, 10);
        assert_eq!(markets[&7].book.bids().len(), 1);
    }

    #[test]
    fn delta_before_snapshot_fails() {
        let mut markets = HashMap::new();
        assert!(apply_orderbook_message(3, 7, delta(0, 1, "[]", "[]"), &mut markets).is_err());
    }
}
