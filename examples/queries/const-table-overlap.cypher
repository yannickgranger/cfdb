// const-table-overlap.cypher — RFC-040 §3.4.
//
// Finds pairs of `:ConstTable` nodes that hold the same set of literal
// values across distinct crates. Each pair is a candidate horizontal
// split-brain (Pattern A) where two crates redefine the same lookup
// table — the runaway scar this rule exists to catch is qbot-core #3656
// (Kraken `Z_PREFIX_CURRENCIES` ⊂ `FIAT_CURRENCIES`), where a Kraken-
// adapter-local list of "Z-prefixed" currencies was a subset of the
// canonical fiat list and silently rotted out of sync.
//
// # v0.1 scope — DUPLICATE branch only
//
// RFC-040 §3.4 prescribes three verdicts in precedence order:
//
//   1. CONST_TABLE_DUPLICATE        — identical entry sets
//   2. CONST_TABLE_SUBSET           — one set is a strict subset of the other
//   3. CONST_TABLE_INTERSECTION_HIGH — Jaccard ≥ 0.5
//
// Verdicts 2 and 3 require the `entries_subset(a, b)` and
// `entries_jaccard(a, b)` UDFs operating over `entries_normalized` JSON
// strings (RFC §3.4). Those UDFs are NOT yet registered in cfdb-query as
// of this slice's ship time — see follow-up issue. Per the slice-4
// acceptance check (R2 solid-architect N2), the rule MUST NOT silently
// degrade: this file ships ONLY the DUPLICATE branch and the gap is
// recorded in the follow-up. SUBSET / INTERSECTION_HIGH coverage lands
// when the UDFs ship.
//
// **Consequence for #3656 reproduction.** Kraken `Z_PREFIX_CURRENCIES` is
// a STRICT SUBSET of canonical `FIAT_CURRENCIES`, not a duplicate, so
// the DUPLICATE-only rule does NOT reproduce the original scar. The
// SUBSET branch will. The follow-up issue is the tracking signal.
//
// # What this rule matches (DUPLICATE branch)
//
// Two `:ConstTable` nodes whose `entries_hash` is identical and whose
// `element_type` matches, declared in DIFFERENT crates, both
// non-test-mod. The `entries_hash` is sha256 over canonical-sorted
// entries (RFC §3.1) — equality of hashes IS equality of sets regardless
// of declaration order. The lexicographic `a.qname < b.qname` guard
// dedupes the symmetric pair so each unordered pair is reported once.
//
// # Filters
//
// - Cross-crate only: `a.crate <> b.crate`. Two const tables in the same
//   crate that hold the same set are usually intentional (a `pub use`
//   re-export OR a deliberate alias); cross-crate is the split-brain
//   signal.
// - `is_test = false` for both: test fixtures legitimately redeclare
//   prod constants for unit-test isolation; flagging them is noise.
// - `element_type` matches: a `&[&str]` table cannot duplicate a
//   `&[u32]` table even with the same `entries_hash` collision (which is
//   a sha256 collision in practice, not a real overlap). The
//   element_type check makes the rule semantically scoped.
// - `a.qname < b.qname`: pair-dedup; report the pair once with `a` as the
//   lex-smaller qname.
//
// # Output columns
//
//   verdict       — 'CONST_TABLE_DUPLICATE'
//   a_qname       — :ConstTable.qname of the lex-smaller member
//   a_crate       — :ConstTable.crate of the lex-smaller member
//   b_qname       — :ConstTable.qname of the lex-larger member
//   b_crate       — :ConstTable.crate of the lex-larger member
//   element_type  — shared element_type ("str" | "u32" | "i32" | "u64" | "i64")
//   entry_count   — shared entry count
//   sample        — :ConstTable.entries_sample of `a` (declaration-order JSON,
//                   first 8) for triage
//
// # Triage
//
// A row is a candidate `duplicated_feature` (RFC §A2.1 class 1). Route
// per §A2.3 SkillRoutingTable → `/sweep-epic` after manual confirmation
// that the pair is unintentional drift rather than a deliberate
// re-export. The `sample` column is the human-readable triage aid.
//
// # Determinism
//
// `ORDER BY a_qname, b_qname` gives a stable row order across runs.
// Validated by the §12.1 determinism CI recipe.
//
// # Usage
//
//   cfdb query --db <dir> --keyspace <ks> "$(cat const-table-overlap.cypher)"
//
// Expected: empty on a clean tree. Any row is a duplicated_feature
// candidate.

MATCH (a:ConstTable), (b:ConstTable)
WHERE a.crate <> b.crate
  AND a.qname < b.qname
  AND a.is_test = false
  AND b.is_test = false
  AND a.element_type = b.element_type
  AND a.entries_hash = b.entries_hash
RETURN 'CONST_TABLE_DUPLICATE' AS verdict,
       a.qname AS a_qname,
       a.crate AS a_crate,
       b.qname AS b_qname,
       b.crate AS b_crate,
       a.element_type AS element_type,
       a.entry_count AS entry_count,
       a.entries_sample AS sample
ORDER BY a_qname ASC, b_qname ASC
