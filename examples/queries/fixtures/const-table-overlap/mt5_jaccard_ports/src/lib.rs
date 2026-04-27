//! MT5 jaccard ports fixture — RFC-040 §3.4 INTERSECTION_HIGH
//! branch reproduction. `PORTS = [20, 30, 40]` shares two elements
//! with `kraken_session_ports::PORTS = [10, 20, 30]` (intersection
//! `{20, 30}` of size 2, union `{10, 20, 30, 40}` of size 4,
//! jaccard 0.5).
//!
//! Pins the `entries_jaccard` UDF and the verdict-precedence
//! tie-break:
//! - jaccard 0.5 (≥ 0.5 threshold) AND
//! - neither set is a subset of the other (kraken has 10 missing
//!   from mt5; mt5 has 40 missing from kraken)
//! ⇒ `CONST_TABLE_INTERSECTION_HIGH` per RFC §3.4 precedence (rank 3,
//! after DUPLICATE rank 1 and SUBSET rank 2).
//!
//! Pair with `oanda_session_ports::PORTS = [10, 20]` evaluates to
//! `CONST_TABLE_NONE` (intersection `{20}` of size 1, union
//! `{10, 20, 30, 40}` of size 4, jaccard 0.25 — below threshold)
//! and is filtered out by the rule's trailing
//! `WHERE verdict <> 'CONST_TABLE_NONE'`.

pub const PORTS: [u32; 3] = [20, 30, 40];
