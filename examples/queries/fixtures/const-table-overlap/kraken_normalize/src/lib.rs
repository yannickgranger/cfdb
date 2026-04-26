//! Kraken normalize fixture — declares the same fiat-currencies set as
//! `oanda_pricing` but in a different declaration order. The
//! `entries_hash` MUST match (sha256 over canonical-sorted entries).

pub const FIAT: &[&str] = &["USD", "EUR", "GBP"];
