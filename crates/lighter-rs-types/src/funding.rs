//! `/api/v1/fundings` response: settled funding history for one market.
//!
//! Fields checked against the official OpenAPI definition at
//! `https://apidocs.lighter.xyz/reference/fundings.md` and a live response
//! from `https://api.rh.lighter.xyz/api/v1/fundings` (checked 2026-10-27).
//! `timestamp` is Unix seconds in the live response. The docs do not state
//! the unit. `value` is omitted: the docs do not define its meaning.
//! Serde ignores unknown fields, so adding it later is additive.
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct FundingsResponse {
    pub code: i64,
    /// `"1h"` or `"1d"`. The request selects the resolution.
    pub resolution: String,
    pub fundings: Vec<Funding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Funding {
    /// Unix seconds of the settlement.
    pub timestamp: i64,
    /// Decimal string, e.g. `"0.0004"`.
    pub rate: String,
    /// Direction string from the docs, e.g. `"long"`.
    pub direction: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_live_fundings_response() {
        // Captured from https://api.rh.lighter.xyz/api/v1/fundings?market_id=15&resolution=1h
        // on 2026-10-27, trimmed to two rows.
        let resp: FundingsResponse = serde_json::from_str(
            r#"{"code":200,"resolution":"1h","fundings":[{"timestamp":1790416800,"value":"0.00089832","rate":"0.0004","direction":"long"},{"timestamp":1790420400,"value":"0.00089876","rate":"0.0004","direction":"long"}]}"#,
        )
        .unwrap();
        assert_eq!(resp.code, 200);
        assert_eq!(resp.resolution, "1h");
        assert_eq!(resp.fundings.len(), 2);
        assert_eq!(resp.fundings[0].timestamp, 1790416800);
        assert_eq!(resp.fundings[0].rate, "0.0004");
        assert_eq!(resp.fundings[0].direction, "long");
    }
}
