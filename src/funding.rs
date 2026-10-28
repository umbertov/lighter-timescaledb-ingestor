//! Settled funding collection from `/api/v1/fundings`.
//!
//! The endpoint returns settled fundings only. Predicted rates from the
//! `market_stats` WebSocket channel never reach this module. Each request
//! names one market, the `1h` resolution, and a start instant. The API
//! includes the start instant: `start_timestamp=T` returns the row at `T`
//! (live probe, 2026-10-28). Each page holds at most 750 rows, so a full
//! page means more rows may follow.
use crate::models::FundingRateRow;
use crate::{insert_funding_rates, newest_funding_time};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use color_eyre::eyre::{bail, Result, WrapErr};
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::PgConnection;
use lighter_rs_types::FundingsResponse;
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

/// Lighter settles funding hourly. The response must echo this resolution.
pub const FUNDING_RESOLUTION: &str = "1h";
/// Maximum rows per `/api/v1/fundings` call, from the endpoint docs.
pub const MAX_FUNDINGS_PER_CALL: usize = 750;
/// Standard accounts get 60 REST requests per rolling minute. One request
/// per 1.1 seconds stays below that limit, from the Lighter rate-limits docs.
const REQUEST_SPACING: Duration = Duration::from_millis(1100);

/// A perp market from the `symbols` table.
#[derive(Debug, Clone)]
pub struct PerpMarket {
    pub symbol_id: i32,
    pub name: String,
    pub lighter_market_id: i32,
}

/// One settled funding row, parsed from the API response.
#[derive(Debug, Clone, PartialEq)]
pub struct SettledFunding {
    pub time: DateTime<Utc>,
    pub rate: BigDecimal,
    pub direction: String,
}

/// Parses one `/api/v1/fundings` body. Rejects any other code or resolution,
/// any missing field, and any rate that is not an exact decimal.
pub fn parse_fundings_response(body: &str) -> Result<Vec<SettledFunding>> {
    let resp: FundingsResponse =
        serde_json::from_str(body).wrap_err("parsing fundings response")?;
    if resp.code != 200 {
        bail!("fundings returned code {}", resp.code);
    }
    if resp.resolution != FUNDING_RESOLUTION {
        bail!(
            "fundings returned resolution {:?}, expected {FUNDING_RESOLUTION:?}",
            resp.resolution
        );
    }
    resp.fundings
        .into_iter()
        .map(|funding| {
            let time = DateTime::from_timestamp(funding.timestamp, 0).ok_or_else(|| {
                color_eyre::eyre::eyre!("funding timestamp {} is out of range", funding.timestamp)
            })?;
            let rate = BigDecimal::from_str(&funding.rate).wrap_err_with(|| {
                format!("funding rate {:?} at {time} is not a decimal", funding.rate)
            })?;
            if funding.direction.is_empty() {
                bail!("funding at {time} has an empty direction");
            }
            Ok(SettledFunding {
                time,
                rate,
                direction: funding.direction,
            })
        })
        .collect()
}

/// Checks that a page holds rows in strictly increasing time order, none
/// before the inclusive `start` and none after `now`. A settlement cannot lie in the future, so a
/// future time signals a wrong unit, for example milliseconds read as seconds.
pub fn validate_page(
    start: DateTime<Utc>,
    now: DateTime<Utc>,
    rows: &[SettledFunding],
) -> Result<()> {
    let mut previous: Option<DateTime<Utc>> = None;
    for row in rows {
        if row.time < start {
            bail!(
                "funding at {} is before the requested start {start}",
                row.time
            );
        }
        if let Some(previous) = previous.filter(|previous| row.time <= *previous) {
            bail!(
                "funding at {} does not follow {previous}; the page is not strictly increasing",
                row.time
            );
        }
        if row.time > now {
            bail!("funding at {} is after the current time {now}", row.time);
        }
        previous = Some(row.time);
    }
    Ok(())
}

/// Returns the inclusive start of the next request: one second after the
/// newest stored settlement, or the configured backfill instant when the
/// market has no stored settlement.
pub fn next_request_start(
    newest_stored: Option<DateTime<Utc>>,
    backfill_start: DateTime<Utc>,
) -> DateTime<Utc> {
    match newest_stored {
        Some(time) => after(time),
        None => backfill_start,
    }
}

/// The API start is inclusive, so a request that must skip `time` starts one
/// second later. Settlements are whole hours, so no settlement is skipped.
fn after(time: DateTime<Utc>) -> DateTime<Utc> {
    time + ChronoDuration::seconds(1)
}

/// Polls settled funding for every perp market, forever.
///
/// Any error stops the poller, so the caller exits the process. The process
/// restarts through the container restart policy.
pub async fn run_funding_poller(
    api_url: String,
    markets: Vec<PerpMarket>,
    pool: Pool<ConnectionManager<PgConnection>>,
    backfill_start: DateTime<Utc>,
    poll_interval: Duration,
) -> Result<()> {
    if markets.is_empty() {
        bail!("no perp markets to poll for funding");
    }
    let http = reqwest::Client::new();
    loop {
        for market in &markets {
            poll_market(&http, &api_url, &pool, market, backfill_start).await?;
        }
        info!("polled settled funding for {} perp markets", markets.len());
        tokio::time::sleep(poll_interval).await;
    }
}

async fn poll_market(
    http: &reqwest::Client,
    api_url: &str,
    pool: &Pool<ConnectionManager<PgConnection>>,
    market: &PerpMarket,
    backfill_start: DateTime<Utc>,
) -> Result<()> {
    let symbol_id = market.symbol_id;
    let newest = with_conn(pool, move |conn| newest_funding_time(symbol_id, conn)).await?;
    let mut start = next_request_start(newest, backfill_start);

    loop {
        let rows = fetch_page(http, api_url, market.lighter_market_id, start).await?;
        validate_page(start, Utc::now(), &rows)?;
        let Some(last_time) = rows.last().map(|row| row.time) else {
            return Ok(());
        };
        let fetched = rows.len();
        let db_rows: Vec<FundingRateRow> = rows
            .into_iter()
            .map(|row| FundingRateRow {
                time: row.time,
                symbol: symbol_id,
                rate: row.rate,
                direction: row.direction,
            })
            .collect();
        let inserted = with_conn(pool, move |conn| insert_funding_rates(&db_rows, conn)).await?;
        info!(
            market = %market.name,
            fetched,
            inserted,
            "stored settled funding rows"
        );

        if fetched < MAX_FUNDINGS_PER_CALL {
            return Ok(());
        }
        start = after(last_time);
    }
}

/// Requests one page of settled fundings from `start` (inclusive) up to now.
/// Waits `REQUEST_SPACING` after every request, to respect the rate limit.
async fn fetch_page(
    http: &reqwest::Client,
    api_url: &str,
    lighter_market_id: i32,
    start: DateTime<Utc>,
) -> Result<Vec<SettledFunding>> {
    let end = Utc::now();
    let body = http
        .get(format!("{api_url}/api/v1/fundings"))
        .query(&[
            ("market_id", lighter_market_id.to_string()),
            ("resolution", FUNDING_RESOLUTION.to_string()),
            ("start_timestamp", start.timestamp().to_string()),
            ("end_timestamp", end.timestamp().to_string()),
            // count_back 0 returns all fundings in the window, capped at 750 per call.
            ("count_back", "0".to_string()),
        ])
        .send()
        .await
        .wrap_err("requesting fundings")?
        .error_for_status()
        .wrap_err("fundings request failed")?
        .text()
        .await
        .wrap_err("reading fundings response")?;
    tokio::time::sleep(REQUEST_SPACING).await;
    parse_fundings_response(&body)
}

/// Runs a blocking database closure on a pooled connection.
async fn with_conn<T, F>(pool: &Pool<ConnectionManager<PgConnection>>, f: F) -> Result<T>
where
    F: FnOnce(&mut PgConnection) -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    let pool = pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().wrap_err("getting a pooled connection")?;
        f(&mut conn)
    })
    .await
    .wrap_err("joining database task")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn parses_exact_decimal_rates_and_unix_second_times() {
        let rows = parse_fundings_response(
            r#"{"code":200,"resolution":"1h","fundings":[{"timestamp":1790416800,"value":"0.00089832","rate":"0.0004","direction":"long"},{"timestamp":1790420400,"value":"0.00089876","rate":"-0.00012500","direction":"short"}]}"#,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].time, at(1790416800));
        assert_eq!(rows[0].rate, BigDecimal::from_str("0.0004").unwrap());
        assert_eq!(rows[0].rate.to_string(), "0.0004");
        assert_eq!(rows[0].direction, "long");
        // Trailing zeros and the sign survive parsing: no float rounding.
        assert_eq!(rows[1].rate.to_string(), "-0.00012500");
        assert_eq!(rows[1].direction, "short");
    }

    #[test]
    fn accepts_an_empty_window() {
        let rows =
            parse_fundings_response(r#"{"code":200,"resolution":"1h","fundings":[]}"#).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn rejects_an_error_code() {
        let err =
            parse_fundings_response(r#"{"code":429,"resolution":"1h","fundings":[]}"#).unwrap_err();
        assert!(err.to_string().contains("code 429"), "{err}");
    }

    #[test]
    fn rejects_a_different_resolution() {
        let err =
            parse_fundings_response(r#"{"code":200,"resolution":"1d","fundings":[]}"#).unwrap_err();
        assert!(err.to_string().contains("resolution"), "{err}");
    }

    #[test]
    fn rejects_a_missing_rate() {
        assert!(parse_fundings_response(
            r#"{"code":200,"resolution":"1h","fundings":[{"timestamp":1790416800,"direction":"long"}]}"#,
        )
        .is_err());
    }

    #[test]
    fn rejects_a_numeric_rate_instead_of_a_string() {
        // A float rate would lose precision, so the shape must be a string.
        assert!(parse_fundings_response(
            r#"{"code":200,"resolution":"1h","fundings":[{"timestamp":1790416800,"rate":0.0004,"direction":"long"}]}"#,
        )
        .is_err());
    }

    #[test]
    fn rejects_a_rate_that_is_not_a_decimal() {
        let err = parse_fundings_response(
            r#"{"code":200,"resolution":"1h","fundings":[{"timestamp":1790416800,"rate":"abc","direction":"long"}]}"#,
        )
        .unwrap_err();
        assert!(format!("{err:?}").contains("not a decimal"), "{err:?}");
    }

    #[test]
    fn rejects_a_future_settlement_such_as_milliseconds_read_as_seconds() {
        // Milliseconds parse as a valid but far-future instant. The page check rejects it.
        let rows = parse_fundings_response(
            r#"{"code":200,"resolution":"1h","fundings":[{"timestamp":1790416800000,"rate":"0.0004","direction":"long"}]}"#,
        )
        .unwrap();
        let err = validate_page(at(0), at(1790420400), &rows).unwrap_err();
        assert!(err.to_string().contains("after the current time"), "{err}");
    }

    #[test]
    fn rejects_an_empty_direction() {
        assert!(parse_fundings_response(
            r#"{"code":200,"resolution":"1h","fundings":[{"timestamp":1790416800,"rate":"0.0004","direction":""}]}"#,
        )
        .is_err());
    }

    #[test]
    fn page_must_be_strictly_increasing_from_start() {
        let row = |secs: i64| SettledFunding {
            time: at(secs),
            rate: BigDecimal::from_str("0.0004").unwrap(),
            direction: "long".to_string(),
        };
        let start = at(1000);
        let now = at(2000);
        assert!(validate_page(start, now, &[row(1001), row(1002)]).is_ok());
        assert!(validate_page(start, now, &[]).is_ok());
        // The API includes the start instant.
        assert!(validate_page(start, now, &[row(1000), row(1001)]).is_ok());
        assert!(validate_page(start, now, &[row(999)]).is_err());
        assert!(validate_page(start, now, &[row(1002), row(1001)]).is_err());
        assert!(validate_page(start, now, &[row(1001), row(1001)]).is_err());
        // A row after now is rejected.
        assert!(validate_page(start, now, &[row(2001)]).is_err());
    }

    #[test]
    fn starts_one_second_after_the_newest_stored_settlement() {
        let newest = Utc.with_ymd_and_hms(2026, 10, 27, 12, 0, 0).unwrap();
        let configured = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(
            next_request_start(Some(newest), configured),
            Utc.with_ymd_and_hms(2026, 10, 27, 12, 0, 1).unwrap()
        );
    }

    #[test]
    fn starts_at_the_backfill_start_when_nothing_is_stored() {
        let configured = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(next_request_start(None, configured), configured);
    }
}
