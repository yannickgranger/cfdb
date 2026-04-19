// ledger-canonical-bypass.cypher — Pattern C instance for issue #3525.
//
// Finds any prod method on LedgerService that calls `.append(...)` as
// a method invocation instead of the canonical `.append_idempotent(...)`.
// The idempotent variant is the write path's safety barrier: without
// it, a double-submitted fill double-posts its journal entries.
//
// Backlog evidence:
//   #3525 fix(ledger): LedgerService::record_trade calls append()
//         not append_idempotent()
//
// Catches the filed bug AND any other LedgerService method that shares
// the pattern. When run against qbot-core HEAD on 2026-04-13 this
// surfaces two prod call sites: `record_trade` (#3525 itself) and
// `record_fill` (same bug in a sibling method the issue body may not
// have enumerated).
//
// Why this works without :Concept / CANONICAL_FOR edges:
//   The v0.1 cfdb schema already carries enough to express this
//   specific Pattern C instance:
//     - :CallSite with `callee_path` + `kind` tells us which method
//       is being invoked
//     - :Item `qname` tells us which method contains the call
//     - `is_test` filter drops test harness code cleanly
//   A general "for any concept C, find callers of non-canonical
//   implementations" rule needs the Layer 2 concept overlay edges
//   (:LABELED_AS, :CANONICAL_FOR) which are v0.2 per RFC §14 Q7.
//
// Usage:
//   cfdb violations --db <dir> --keyspace <ks> --rule ledger-canonical-bypass.cypher
//
// Expected: empty on a clean tree. Any row is a prod write-path
// call site that must be migrated to `.append_idempotent(...)`.

MATCH (caller:Item)-[:INVOKES_AT]->(cs:CallSite)
WHERE caller.qname =~ '.*::LedgerService::.*'
  AND cs.callee_path = 'append'
  AND cs.kind = 'method'
  AND caller.is_test = false
  AND cs.is_test = false
RETURN caller.qname, caller.file
