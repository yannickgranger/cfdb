// arch-ban-f64-in-domain.cypher — RFC §13 Phase C ban rule (issue #3637).
//
// Bans f64 construction and conversion inside the financial-path domain
// crates. Surfaces every call site in `domain-market`, `domain-ledger`,
// `domain-portfolio`, or `domain-trading-core` whose callee path routes
// through an f64 conversion (`from_f64`, `to_f64`, `as_f64`, `f64::from`).
//
// The financial-path domains use `rust_decimal::Decimal` exclusively
// (CLAUDE.md §Financial Precision). Any call that coerces money, prices,
// fees, or quantity values through f64 is a precision-loss bug.
//
// # Scope carve-out — domain-trading is NOT covered
//
// `domain-trading` legitimately uses f64 for pure-math helpers
// (`pair_math.rs` statistics, `statistical.rs` p-value / t-distribution,
// `hedge_sizing.rs` mean/variance window aggregation, hull sqrt per
// CLAUDE.md §Audit Status). Widening this rule to match all of `domain-*`
// would false-positive on those math helpers — forbidden by issue #3637
// context package Forbidden Move #6.
//
// # Why conversion-only, not all f64 mentions
//
// The extractor tracks `CallSite` via `callee_path` regex — it does not
// index field types or parameter types. A broad `cs.callee_path =~ '.*f64.*'`
// would match `f64::NAN`, `f64::sqrt`, arithmetic method calls, etc. —
// all legitimate math operations. The layer-violation signal is crossing
// the `Decimal` <-> f64 boundary: `from_f64`, `to_f64`, `as_f64`,
// `f64::from`. Matching only conversion/construction paths catches the
// bug without false-positives on arithmetic.
//
// Usage:
//   cfdb violations --db <dir> --keyspace <ks> --rule arch-ban-f64-in-domain.cypher
//
// Expected: empty result on a clean tree. Any row is a precision-loss risk
// in the financial-path domain.

MATCH (caller:Item)-[:INVOKES_AT]->(cs:CallSite)
WHERE caller.crate =~ '(domain-market|domain-ledger|domain-portfolio|domain-trading-core).*'
  AND cs.callee_path =~ '^.*(from_f64|to_f64|as_f64|f64::from)$'
  AND caller.is_test = false
  AND cs.is_test = false
RETURN caller.qname, caller.crate, cs.file, cs.callee_path
