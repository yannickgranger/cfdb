// hsb-cluster.cypher — RFC-036 §3.4 HSB detector (v2).
//
// Detects Horizontal Split-Brain: the same *concept* is implemented
// twice in the codebase under distinct qnames, usually in sibling
// crates. The canonical HSB failure mode is two fn items that share a
// structural signature and were independently authored — classic
// "we had no shared kernel so both teams shipped `stop_from_bps`"
// shape (RFC-cfdb.md §A1.2).
//
// # v2 signal scheme
//
// RFC-036 §3.4 specifies a four-signal detector with ≥2-signal
// acceptance. The full signal set:
//
//   S1: same `signature_hash` (RFC-035-indexed, produced by extractor)
//   S2: same normalized name — exact-match in v2; edit-distance-1
//       deferred to v2.1 as an extension point
//   S3: neighbour-set Jaccard ≥ 0.6 over `TYPE_OF` neighbours
//   S4: shared conversion target — both items implement
//       `From<T>` / `Into<T>` / `TryFrom<T>` for the same `T`
//
// ## v2 reduction — why this query ships only S1
//
// S3 and S4 aren't expressible in the v0.3 cfdb-query subset:
//
//   - S3 (Jaccard) needs set intersection + cardinality over the
//     `TYPE_OF` neighbours of each side, plus division with a float
//     threshold. The subset has `collect()` + `size()` but no
//     per-pair set-difference operator.
//   - S4 (conversion target) needs a positive `EXISTS { MATCH ... }`
//     to assert "both sides share an IMPLEMENTS_FOR target". The
//     subset only supports `NOT EXISTS` (the inverse).
//
// Both are v2.1 extension points — documented here rather than
// silently omitted.
//
// Step 3 (#203) already materialises signal 1 as the
// `:Item.dup_cluster_id` attribute: `sha256(lex_sorted(member_qnames))`
// keyed by shared `signature_hash` on `:Item{kind:"fn"}` groups of
// size ≥ 2. Emitting pairs that share a `dup_cluster_id` IS the v2
// HSB detector — each such pair necessarily has matching
// `signature_hash` (signal 1) and was grouped by the step-3 pass.
//
// Name equality (signal 2) is NOT required as a gating predicate —
// the issue's Tests row explicitly asks that a **synonym-renamed**
// duplicate also clusters, so only S1 gates acceptance in v2.
// Callers compare `a_name` vs `b_name` in the result columns to
// distinguish same-name clusters (highest-priority, consolidate) from
// synonym-renamed clusters (triage).
//
// ## Carve-out (RFC-036 §3.4) — already implicit
//
// Unit structs and empty types are excluded from HSB per RFC §3.4 to
// avoid `struct Marker;` false-clustering storms. Step 3 populates
// `dup_cluster_id` only on `:Item{kind:"fn"}`, so structs / enums /
// traits / impl_blocks are silently ineligible for v2 clustering —
// the carve-out is enforced by the producer-side kind filter, not by
// this query. Extending HSB to types is a v2.1 concern (needs
// extractor-side type-signature hashing).
//
// ## Test-scope filter
//
// `is_test = false` on both sides excludes `#[cfg(test)]` and `#[test]`
// fns from clustering. A test helper `make_stop` in crate A and a
// production `make_stop` in crate B are not HSB — the test helper
// isn't shipped. The filter is load-bearing for zero-false-positive
// dogfood.
//
// # Output columns
//
//   cluster_id  — `:Item.dup_cluster_id` = sha256(lex_sorted members)
//   a_qname     — lex-first member of the pair
//   a_name      — unqualified name of a
//   b_qname     — lex-second member of the pair
//   b_name      — unqualified name of b
//
// # Usage
//
//   cfdb query --db <dir> --keyspace <ks> "$(cat .cfdb/queries/hsb-cluster.cypher)"
//
// Expected: empty on a clean tree. Any row is an HSB candidate.
// Reviewer triage using the returned columns:
//   - `a_name == b_name` → highest priority, same concept / same
//                          name duplicated — consolidate
//   - `a_name != b_name` → synonym-rename — either consolidate or
//                          document the intentional divergence in
//                          KNOWN_GAPS.md

MATCH (a:Item), (b:Item)
WHERE a.kind = 'fn'
  AND b.kind = 'fn'
  AND a.qname < b.qname
  AND a.dup_cluster_id = b.dup_cluster_id
  AND a.is_test = false
  AND b.is_test = false
RETURN a.dup_cluster_id AS cluster_id,
       a.qname AS a_qname,
       a.name AS a_name,
       b.qname AS b_qname,
       b.name AS b_name
ORDER BY cluster_id ASC, a_qname ASC, b_qname ASC
