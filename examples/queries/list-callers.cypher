// smoke-skip: parameterized ($qname) — caller binds via `cfdb list-callers --qname` or `cfdb query --params`
// list-callers.cypher — generic discovery query (RFC §13 v0.1 AC item 5).
//
// "Where is this function / method used?" — the canonical user story for
// ad-hoc agent queries against cfdb. Not a ban rule, not a gate; a
// discovery query that returns structured facts an agent can reason
// about without re-reading Rust source.
//
// Parameterized by $qname — a regex-compatible pattern for the callee
// path. Callers bind $qname either via the `list-callers` typed verb
// (`cfdb list-callers --qname <regex>`) or via the generic query path
// (`cfdb query --params '{"qname":"<regex>"}' "$(cat list-callers.cypher)"`).
// Both paths must produce identical results — that is the contract.
//
// Shape: find every function or method that contains a call expression
// whose callee path matches $qname. Returns the caller qname + crate +
// file + exact callee path so a follow-up agent can jump to the call site.
//
// Design notes:
// - $qname is a Rust regex pattern. For substring/case-insensitive match
//   pass `(?i).*kalman.*`. For exact match pass `^qbot_indicators::kalman$`.
// - Not filtered to prod-only on purpose — test code that exercises the
//   target is useful context for "where is this used", even if it would
//   be noise in an arch-ban rule.
// - No `cs.kind` filter — catches both path calls (`kalman::filter(...)`)
//   and method calls (`x.apply_kalman()`).
//
// Usage:
//   cfdb list-callers --db <dir> --keyspace <ks> --qname '(?i).*kalman.*'
//   cfdb query --db <dir> --keyspace <ks> \
//       --params '{"qname":"(?i).*kalman.*"}' "$(cat list-callers.cypher)"
//
// Expected: any row is a use site. Zero rows means the target is defined
// but never called (dead code), which is itself interesting information.

MATCH (caller:Item)-[:INVOKES_AT]->(cs:CallSite)
WHERE cs.callee_path =~ $qname
RETURN caller.qname, caller.crate, cs.file, cs.callee_path
