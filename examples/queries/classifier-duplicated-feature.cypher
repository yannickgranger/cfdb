// smoke-skip: parameterized ($context) — bound by RFC-036 classifier suite
// classifier-duplicated-feature.cypher — §A2.1 class 1 (DuplicatedFeature).
//
// Two independent implementations of the same concept **within the same
// bounded context**, across different crates. The horizontal split-brain
// shape restricted to same-context pairs.
//
// # Inputs
// - `:Item.name`, `:Item.kind`, `:Item.crate` — core extractor props.
// - `:Item.bounded_context` — populated by `enrich_bounded_context`
//   (slice 43-E / issue #108). Required for same-context filtering.
// - `:Item.is_test` — extractor-time filter flag.
//
// # Parameters
// - `$context` — bounded context to scope the finding to.
//
// # Output shape
// Projects one row per item participating in the duplicate set. Columns
// match `cfdb_query::Finding`:
//   qname, name, kind, crate, file, line, bounded_context.
//
// Each duplicate type surfaces one row per definition (not one row per
// pair), so the `ScopeInventory::findings_by_class[DuplicatedFeature]`
// bucket carries every offending `:Item` independently — the CLI
// orchestrator dedups by qname when building the bucket.
//
// # Scope vs §A2.1 signals
// The addendum §A2.1 signals for class 1 include:
//   - same bounded_context (DONE here)
//   - independent git blame — NOT checked here (git_history enrichment
//     is extractor-crate-only; the query surface cannot temporally
//     correlate without an additional enrich pass projecting per-item
//     age buckets into the graph)
//   - signature_hash similarity — NOT checked here (syn-only keyspaces
//     do not carry the `signature_hash` prop; HIR-mode keyspaces do)
//   - no cross-reference comments — NOT checked here
//
// The rule therefore flags CANDIDATES for class 1; the §A2.1 §A2.2
// full classifier joins on those additional signals in a v0.3 form.
// v0.1 ships the same-context + same-name-kind discriminator, which
// is the load-bearing `duplicated_feature ≠ context_homonym` split.
//
// # Why the `struct/enum/trait` kind restriction
// Matches `hsb-by-name.cypher` — fn-name collisions across crates are
// semantic noise (different methods on different types share a name
// trivially). Struct / enum / trait homonyms within one context are
// the split-brain signal.
//
// # Determinism
// `ORDER BY qname` — stable row order required by G1.

MATCH (a:Item), (b:Item)
WHERE a.kind IN ['struct', 'enum', 'trait']
  AND b.kind IN ['struct', 'enum', 'trait']
  AND a.kind = b.kind
  AND a.name = b.name
  AND a.qname <> b.qname
  AND a.crate <> b.crate
  AND a.bounded_context = b.bounded_context
  AND a.bounded_context = $context
  AND a.is_test = false
  AND b.is_test = false
RETURN a.qname AS qname,
       a.name AS name,
       a.kind AS kind,
       a.crate AS crate,
       a.file AS file,
       a.line AS line,
       a.bounded_context AS bounded_context
ORDER BY qname ASC
