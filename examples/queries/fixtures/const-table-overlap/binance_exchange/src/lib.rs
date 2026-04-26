//! Binance exchange fixture — same `element_type="str"` as the FIAT
//! pair but a non-overlapping entry set. Pins that the rule does not
//! cross-pair on element_type alone — `entries_hash` equality is the
//! join key.

pub const STABLES: &[&str] = &["USDC", "USDT"];
