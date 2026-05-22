//! Oanda pricing fixture — declares the same fiat-currencies set as
//! `kraken_normalize::FIAT` but in alphabetical order (different from
//! the kraken declaration order). Pins the `entries_hash` order-
//! invariance contract — the DUPLICATE rule MUST pair the two consts.

pub const FIAT: &[&str] = &["GBP", "USD", "EUR"];
