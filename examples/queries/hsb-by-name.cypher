// hsb-by-name.cypher — Pattern A horizontal split-brain detector, enriched.
// RFC-029 §3.1 + §13 acceptance gate Item 8 (promoted from example → v0.1 gate
// via issue #3675).
//
// Finds items (`struct` / `enum` / `trait`) whose `name` + `kind` is defined in
// more than one crate, emitting complete triage context in one pass so that
// the human reader does not have to run a follow-up `MATCH` per candidate.
// The original "just a count" rule required 58 manual lookups on develop to
// separate real split-brain from name collision; this enriched form carries
// `crates`, `qnames`, and `files` arrays on every row.
//
// **Visibility column intentionally omitted.** The v0.1 extractor only emits
// `:Item` nodes whose visibility is already `pub` / `pub(crate)` (per
// RFC-029 §7 — private items are not surfaced), so a `visibilities[]` column
// would be uniform and carry no triage signal. The original AC #3 in issue
// #3675 listed the column; the extractor constraint was discovered during
// implementation and the column dropped accordingly.
//
// ⚠️ TRIAGE NOTE — read BEFORE routing any candidate to a fix skill.
//
// The `crates` column is NECESSARY but NOT SUFFICIENT to classify findings.
//
//  - Two crates in the SAME bounded context → likely `duplicated_feature`
//    class, routes to /sweep-epic (pick one head, `pub use` the rest).
//  - Two crates in DIFFERENT bounded contexts → likely `context_homonym`
//    class, routes to /operate-module + council. Context Mapping decision
//    required — ACL / Shared Kernel / Conformist / Published Language.
//    Mechanical renaming destroys legitimate context isolation.
//
// The "distinct-concept renames" cluster in EPIC #3519 Cluster C (#3548) is
// the warning sign: distinct concepts across bounded contexts ARE homonyms
// by definition, and mechanical /sweep-epic on them is a Pattern 1 regression.
//
// Bounded-context assignment is v0.2 work (`cfdb-hir-extractor` +
// `enrich_bounded_context` pass per RFC-029 v0.2 addendum §A2.2). This v0.1
// query cannot make the distinction — it only surfaces candidates.
//
// 🚨 DO NOT route cross-context candidates to /sweep-epic based on this
//    query alone during the v0.1 → v0.2 window. When in doubt, hold the
//    finding until the classifier ships and can assign `bounded_context`.
//
// **Scope:** Structs, enums, and traits only. Fn items are excluded because
// method-name collisions across unrelated types are semantic noise, not
// split-brain. A later v0.2 rule can layer `signature_hash` Jaccard clustering
// on top to catch synonym-renamed duplicates (`OrderStatus` vs `OrderState`)
// that this rule misses — deferred to RFC-029 v0.2 addendum §A2.2.
//
// **Test exemption:** Items under `#[cfg(test)]` are tagged `is_test=true`
// by the extractor and excluded — tests legitimately declare helper types
// (`MockFoo`, `FixtureBar`, `TestCase`) with overlapping names across crates.
//
// **Same-crate filter:** `count(DISTINCT a.crate) > 1` eliminates candidates
// where the same name appears in multiple inner modules of a single crate.
// Legitimate inner-module reuse is not split-brain — cross-crate is the signal.
//
// **Determinism:** `ORDER BY n DESC, name ASC` gives a stable row order across
// runs (n first for triage priority, name for ties). Validated by the §12.1
// determinism CI recipe — any two-run dump hash divergence on this file is a
// determinism regression.
//
// **Reading a row:**
//   name     — the offending type name (e.g., "OrderStatus")
//   kind     — struct | enum | trait
//   n        — how many distinct Item nodes carry this name+kind
//   crates[] — the crates holding each definition (size > 1 by filter)
//   qnames[] — full qname per definition
//   files[]  — source file per definition
//
// Usage:
//   cfdb query --db <dir> --keyspace <ks> "$(cat hsb-by-name.cypher)"
//
// Expected: non-empty result on a tree with accumulated drift; empty result on
// a clean tree. Every row is a candidate for a #3616-class dedup PR OR a
// #3548-class homonym-requires-council decision — the triage note above
// governs which.

MATCH (a:Item)
WHERE a.kind IN ['struct', 'enum', 'trait']
  AND a.is_test = false
WITH a.name AS name,
     a.kind AS kind,
     count(a) AS n,
     count(DISTINCT a.crate) AS n_crates,
     collect(DISTINCT a.crate) AS crates,
     collect(a.qname) AS qnames,
     collect(a.file) AS files
WHERE n > 1
  AND n_crates > 1
RETURN name, kind, n, crates, qnames, files
ORDER BY n DESC, name ASC
