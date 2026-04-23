// signature-divergent.cypher — issue #47.
//
// Finds pairs of fn / method `:Item`s that share a last-segment qname
// across distinct bounded contexts but diverge on `:Item.signature`.
// This is the load-bearing discriminator for the DDD Shared-Kernel-vs-
// Homonym decision (RFC-cfdb.md Addendum B-draft §A1.5 gate v0.2-8,
// `council/RATIFIED.md` R1):
//
//   - IDENTICAL signature, cross-context → Shared Kernel (intentional
//     co-ownership of the same semantic contract — leave alone)
//   - DIVERGENT signature, cross-context → Context Homonym (same name,
//     different semantics — route to `/operate-module`, NOT
//     `/sweep-epic`; class 2 in RFC §A2.1)
//
// # What this rule matches
//
// Two fn / method items whose last qname segment matches and whose
// owning crates belong to different bounded contexts, AND whose
// `:Item.signature` strings differ (after whitespace normalization).
// `signature_divergent(a, b)` is the hard-wired UDF that performs the
// comparison — see `docs/udfs.md` for the normalization contract.
//
// # Why the `<>` guard and the UDF together
//
// `a.signature <> b.signature` alone would miss whitespace-only drift
// the extractor may emit as `syn` evolves (AI-dependent changes to
// `render_fn_signature`). The UDF normalizes internal whitespace; the
// `<>` guard is a cheap early-exit so the common equal-prop case skips
// the normalization allocation.
//
// # Output columns
//
// - `last_segment`     — the shared tail segment of both qnames
// - `a_qname`          — qname of the first item (lexicographically smaller)
// - `a_bounded_context` — bounded context of the first item
// - `a_signature`       — raw `:Item.signature` of the first item
// - `b_qname`           — qname of the second item
// - `b_bounded_context` — bounded context of the second item
// - `b_signature`       — raw `:Item.signature` of the second item
//
// # Usage
//
//   cfdb query --db <dir> --keyspace <ks> "$(cat signature-divergent.cypher)"
//
// Expected: empty on a clean tree. Any row is a Context Homonym
// candidate. Route per §A2.3 SkillRoutingTable → `/operate-module`.
//
// # Known motivating bugs (RFC §A2.1 class 2)
//
//   - qbot-core #3618  — `PositionValuation` homonym across
//                        `domain-trading` and `domain-portfolio`
//   - qbot-core #3654  — Context homonym found in 7 split resolution
//                        points for one concept
//
// # Scope (v0.2)
//
// - Only fn / method items carry `:Item.signature`. Struct / enum /
//   trait / const / impl_block / type_alias / static homonyms are NOT
//   surfaced by this rule (they have no signature prop to diverge on).
//   Widening to non-fn kinds is v0.3 scope.
// - The `last_segment` join is syntactic (text match on the trailing
//   qname segment). Cross-context items whose last segments differ but
//   whose semantics are the same (e.g. `compute` vs `evaluate`) are
//   not surfaced — that is the `:Concept` overlay's job (#109).

MATCH (a:Item), (b:Item)
WHERE a.kind IN ['fn', 'method']
  AND b.kind IN ['fn', 'method']
  AND a.qname < b.qname
  AND a.is_test = false
  AND b.is_test = false
  AND a.bounded_context <> b.bounded_context
  AND last_segment(a.qname) = last_segment(b.qname)
  AND a.signature <> b.signature
  AND signature_divergent(a.signature, b.signature) = true
RETURN last_segment(a.qname) AS last_segment,
       a.qname AS a_qname,
       a.bounded_context AS a_bounded_context,
       a.signature AS a_signature,
       b.qname AS b_qname,
       b.bounded_context AS b_bounded_context,
       b.signature AS b_signature
ORDER BY last_segment ASC, a_qname ASC
