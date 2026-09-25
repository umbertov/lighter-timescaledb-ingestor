//! Account REST and WebSocket message types.
//!
//! The REST types match the official Python SDK models and a live positioned
//! account response captured from `/api/v1/account` on 2026-09-22.
//!
//! The WebSocket types match the official Python SDK's `account_all/{id}`
//! subscription. Snapshot, position update, and trade update payloads were
//! captured from the Robinhood Chain stream on 2026-09-22.
use std::collections::HashMap;

use serde::Deserialize;

use crate::trade::Trade;

#[derive(Debug, Clone, Deserialize)]
pub struct DetailedAccountsResponse {
    pub code: i64,
    pub message: Option<String>,
    pub accounts: Vec<DetailedAccount>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DetailedAccount {
    pub index: i64,
    pub collateral: String,
    pub available_balance: Option<String>,
    pub positions: Vec<AccountPosition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountPosition {
    pub market_id: i32,
    pub symbol: String,
    /// `1` for long, `-1` for short, and `0` for flat.
    pub sign: i32,
    /// Absolute base-asset position size.
    pub position: String,
    pub avg_entry_price: String,
    pub unrealized_pnl: String,
    pub realized_pnl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum AccountAllWsMessage {
    #[serde(rename = "subscribed/account_all")]
    Subscribed {
        channel: String,
        account: i64,
        positions: HashMap<String, AccountPosition>,
        trades: HashMap<String, Vec<Trade>>,
    },
    #[serde(rename = "update/account_all")]
    Update {
        channel: String,
        account: i64,
        positions: HashMap<String, AccountPosition>,
        trades: HashMap<String, Vec<Trade>>,
    },
}

impl AccountAllWsMessage {
    pub fn account(&self) -> i64 {
        match self {
            Self::Subscribed { account, .. } | Self::Update { account, .. } => *account,
        }
    }

    pub fn positions(&self) -> &HashMap<String, AccountPosition> {
        match self {
            Self::Subscribed { positions, .. } | Self::Update { positions, .. } => positions,
        }
    }

    pub fn trades(&self) -> &HashMap<String, Vec<Trade>> {
        match self {
            Self::Subscribed { trades, .. } | Self::Update { trades, .. } => trades,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_live_positioned_account_response() {
        let response: DetailedAccountsResponse = serde_json::from_str(
            r#"{"code":200,"message":null,"accounts":[{"index":30597,"collateral":"7985.878279","available_balance":"232.447167","positions":[{"market_id":0,"symbol":"ETH","sign":-1,"position":"9.5500","avg_entry_price":"2742.53","unrealized_pnl":"-61.455788","realized_pnl":"0.000000"}]}]}"#,
        )
        .unwrap();
        let position = &response.accounts[0].positions[0];
        assert_eq!(position.market_id, 0);
        assert_eq!(position.sign, -1);
        assert_eq!(position.position, "9.5500");
    }

    #[test]
    fn parses_a_live_account_trade_update() {
        let message: AccountAllWsMessage = serde_json::from_str(
            r#"{"type":"update/account_all","account":39,"channel":"account_all:39","positions":{},"trades":{"25":[{"trade_id":912297287,"tx_hash":"5de60c","type":"trade","market_id":25,"size":"0.4100","price":"742.98","usd_amount":"304.621800","ask_id":7318349410159933,"bid_id":7599824354593016,"ask_client_id":3012762476492,"bid_client_id":1790072395637,"ask_account_id":39,"bid_account_id":4125,"is_maker_ask":true,"block_height":26058484,"timestamp":1790072395752}]}}"#,
        )
        .unwrap();
        assert_eq!(message.account(), 39);
        let trade = &message.trades()["25"][0];
        assert_eq!(trade.ask_client_id, 3012762476492);
        assert!(trade.is_maker_ask);
    }
}
