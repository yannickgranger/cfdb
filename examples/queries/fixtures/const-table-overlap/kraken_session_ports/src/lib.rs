//! Kraken session ports fixture — RFC-040 §3.4 SUBSET branch
//! reproduction. Together with `oanda_session_ports::PORTS` and
//! `mt5_jaccard_ports::PORTS` this pins the
//! `entries_subset` / `overlap_verdict` UDFs:
//!
//! - `oanda_session_ports::PORTS = [10, 20]` is a STRICT SUBSET of
//!   `kraken_session_ports::PORTS = [10, 20, 30]` → CONST_TABLE_SUBSET.
//! - `mt5_jaccard_ports::PORTS = [20, 30, 40]` shares two of three
//!   elements with this set (jaccard 2/4 = 0.5, neither subset of
//!   the other) → CONST_TABLE_INTERSECTION_HIGH.
//!
//! Reproduction of qbot-core #3656 in numeric form: a "small lookup"
//! and an "extended lookup" diverging in a way that
//! `entries_hash`-equality alone (the v0.1 DUPLICATE branch) cannot
//! detect. The `entries_subset` UDF closes the gap.

pub const PORTS: [u32; 3] = [10, 20, 30];
