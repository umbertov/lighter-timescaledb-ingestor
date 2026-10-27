//! In-memory order book for one market, rebuilt from the WS snapshot and
//! deltas. Prices and sizes are exact decimals: each level keeps the
//! original strings from Lighter, keyed by the parsed price.
//!
//! A delta level with a size of zero (in any form, such as "0" or
//! "0.0000") removes the price level. Any other size replaces it.
use bigdecimal::{BigDecimal, Zero};
use color_eyre::eyre::{Result, WrapErr};
use lighter_rs_types::orderbook::PriceLevel;
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Debug, Default, Clone)]
pub struct Book {
    bids: BTreeMap<BigDecimal, PriceLevel>,
    asks: BTreeMap<BigDecimal, PriceLevel>,
}

impl Book {
    /// Builds a book from a full snapshot. A zero-size level in a snapshot
    /// is invalid data and fails.
    pub fn from_snapshot(bids: &[PriceLevel], asks: &[PriceLevel]) -> Result<Self> {
        Ok(Self {
            bids: side_from_snapshot(bids)?,
            asks: side_from_snapshot(asks)?,
        })
    }

    pub fn apply_delta(&mut self, bids: &[PriceLevel], asks: &[PriceLevel]) -> Result<()> {
        apply_side(&mut self.bids, bids)?;
        apply_side(&mut self.asks, asks)
    }

    /// Bids from the highest price down.
    pub fn bids(&self) -> Vec<PriceLevel> {
        self.bids.values().rev().cloned().collect()
    }

    /// Asks from the lowest price up.
    pub fn asks(&self) -> Vec<PriceLevel> {
        self.asks.values().cloned().collect()
    }
}

fn parse(level: &PriceLevel) -> Result<(BigDecimal, BigDecimal)> {
    let price = BigDecimal::from_str(&level.price)
        .wrap_err_with(|| format!("parsing price level price {:?}", level.price))?;
    let size = BigDecimal::from_str(&level.size)
        .wrap_err_with(|| format!("parsing price level size {:?}", level.size))?;
    Ok((price, size))
}

fn side_from_snapshot(levels: &[PriceLevel]) -> Result<BTreeMap<BigDecimal, PriceLevel>> {
    let mut side = BTreeMap::new();
    for level in levels {
        let (price, size) = parse(level)?;
        if size.is_zero() {
            color_eyre::eyre::bail!("snapshot level at price {} has size zero", level.price);
        }
        side.insert(price, level.clone());
    }
    Ok(side)
}

fn apply_side(side: &mut BTreeMap<BigDecimal, PriceLevel>, levels: &[PriceLevel]) -> Result<()> {
    for level in levels {
        let (price, size) = parse(level)?;
        if size.is_zero() {
            side.remove(&price);
        } else {
            side.insert(price, level.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lv(price: &str, size: &str) -> PriceLevel {
        PriceLevel {
            price: price.to_string(),
            size: size.to_string(),
        }
    }

    fn pairs(levels: &[PriceLevel]) -> Vec<(&str, &str)> {
        levels
            .iter()
            .map(|l| (l.price.as_str(), l.size.as_str()))
            .collect()
    }

    #[test]
    fn snapshot_orders_sides() {
        let book = Book::from_snapshot(
            &[lv("217.69", "1"), lv("217.70", "2")],
            &[lv("217.75", "3"), lv("217.72", "4")],
        )
        .unwrap();
        assert_eq!(pairs(&book.bids()), [("217.70", "2"), ("217.69", "1")]);
        assert_eq!(pairs(&book.asks()), [("217.72", "4"), ("217.75", "3")]);
    }

    #[test]
    fn delta_replaces_adds_and_removes_levels() {
        let mut book =
            Book::from_snapshot(&[lv("217.71", "1.0000"), lv("217.69", "1")], &[]).unwrap();
        // Captured shape from the NVDA perp table: a zero size removes the level.
        book.apply_delta(&[lv("217.71", "0.0000"), lv("217.70", "0.0460")], &[])
            .unwrap();
        book.apply_delta(&[lv("217.69", "5")], &[lv("218", "2")])
            .unwrap();
        assert_eq!(pairs(&book.bids()), [("217.70", "0.0460"), ("217.69", "5")]);
        assert_eq!(pairs(&book.asks()), [("218", "2")]);
    }

    #[test]
    fn zero_in_any_form_removes_and_prices_match_by_value() {
        let mut book = Book::from_snapshot(&[lv("1.50", "1"), lv("2", "1")], &[]).unwrap();
        book.apply_delta(&[lv("1.5", "0"), lv("2.000", "0e0")], &[])
            .unwrap();
        assert!(book.bids().is_empty());
    }

    #[test]
    fn removing_an_absent_level_is_a_no_op() {
        let mut book = Book::from_snapshot(&[lv("1", "1")], &[]).unwrap();
        book.apply_delta(&[lv("9", "0")], &[]).unwrap();
        assert_eq!(pairs(&book.bids()), [("1", "1")]);
    }

    #[test]
    fn rejects_zero_size_in_snapshot_and_bad_decimals() {
        assert!(Book::from_snapshot(&[lv("1", "0")], &[]).is_err());
        let mut book = Book::default();
        assert!(book.apply_delta(&[lv("x", "1")], &[]).is_err());
    }

    #[test]
    fn new_snapshot_resets_the_book() {
        let mut book = Book::from_snapshot(&[lv("1", "1")], &[]).unwrap();
        book.apply_delta(&[lv("2", "1")], &[]).unwrap();
        book = Book::from_snapshot(&[lv("3", "1")], &[]).unwrap();
        assert_eq!(pairs(&book.bids()), [("3", "1")]);
    }
}
