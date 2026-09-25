//! Shared Rust types for Lighter (Robinhood Chain / zkLighter) exchange
//! wire formats.
//!
//! Consumed by both `mm-rs` (the trading bot, in `LighterConnector`) and
//! `lighter-timescaledb-ingestor` (the market-data collector) so the exact
//! same message shapes are parsed the same way in both places, instead of
//! two independently-guessed copies drifting apart.
//!
//! Every type here must trace to a verified source -- either a live
//! response captured from the real API, or the official Python
//! (`mm-lighter-py`'s vendored `lighter` package) or Go (`lighter-go`)
//! SDKs -- see each module's doc comment for its specific sourcing. Do not
//! add a field or message shape you have not actually observed; an
//! unverified guess here silently propagates into every consumer.
pub mod account;
pub mod market;
pub mod orderbook;
pub mod trade;
pub mod trade_ws;

pub use account::{
    AccountAllWsMessage, AccountPosition, DetailedAccount, DetailedAccountsResponse,
};
pub use market::{OrderBookInfo, OrderBooksResponse};
pub use orderbook::{OrderBookSideLevels, OrderBookWsMessage, PriceLevel};
pub use trade::{Trade, TradesResponse};
pub use trade_ws::TradeWsMessage;
