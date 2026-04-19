// arch-ban-utc-now.cypher — RFC §13 headline acceptance item (Q1=(b), Pattern D).
//
// Replaces the handwritten Rust architecture test that enforces RFC-026 S3
// (`test architecture_test_banning_utc_now`). Surfaces every call site in
// a `domain-*` or `ports-*` crate whose callee path ends in `Utc::now` —
// both fully-qualified (`chrono::Utc::now()`) and use-statement-shortened
// (`Utc::now()`).
//
// The extractor (cfdb-extractor v0.1) performs name-based, unresolved call
// extraction via `syn`: a CallSite node is emitted per call expression in
// every fn and impl-method body, with a `callee_path` property carrying the
// textual path the author wrote. That is sufficient here because a ban rule
// cares about the *appearance* of a symbol in source, not type resolution.
//
// Method calls (`.now()`) do NOT match this rule — `Utc::now()` is a path
// call, not a method call. If a future regression writes `use chrono::Utc;
// let now = Utc::now();` it is still a path call and will be caught.
//
// Filters:
// - Only domain-* and ports-* crates (hexagonal inner ring).
// - Only prod code — items and call sites inside `#[cfg(test)]` modules
//   are tagged `is_test=true` by the extractor and excluded here. The
//   test exemption is load-bearing: tests legitimately need a clock to
//   build fixtures, but prod code must receive time from a port.
//
// Usage:
//   cfdb query --db <dir> --keyspace <ks> "$(cat arch-ban-utc-now.cypher)"
//
// Expected: empty result on a clean tree. Any row is a violation.

MATCH (caller:Item)-[:INVOKES_AT]->(cs:CallSite)
WHERE caller.crate =~ '(domain|ports).*'
  AND cs.callee_path =~ '.*Utc::now'
  AND caller.is_test = false
  AND cs.is_test = false
RETURN caller.qname, caller.crate, cs.file, cs.callee_path
