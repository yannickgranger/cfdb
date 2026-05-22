//! Oanda session ports fixture — RFC-040 §3.4 SUBSET branch
//! reproduction. `PORTS = [10, 20]` is a STRICT SUBSET of
//! `kraken_session_ports::PORTS = [10, 20, 30]`.
//!
//! Pins the `entries_subset` UDF: the rule MUST surface this pair
//! under `CONST_TABLE_SUBSET` (verdict precedence: not duplicate
//! because hashes differ; subset because every element is present).
//! Declaration order is intentionally reversed (`[20, 10]`) to
//! pin that `entries_normalized` is canonical-sorted at extract
//! time (RFC-040 §3.4) and `entries_subset` operates on the
//! normalized form.

pub const PORTS: [u32; 2] = [20, 10];
