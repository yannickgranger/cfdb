// const-table-overlap.cypher — RFC-040 §3.4.
//
// Finds pairs of `:ConstTable` nodes that overlap across distinct
// crates. Each pair is a candidate horizontal split-brain (Pattern A)
// where two crates redefine the same lookup table — the runaway scar
// this rule exists to catch is qbot-core #3656 (Kraken
// `Z_PREFIX_CURRENCIES` ⊂ `FIAT_CURRENCIES`), where a Kraken-adapter-
// local list of "Z-prefixed" currencies was a STRICT SUBSET of the
// canonical fiat list and silently rotted out of sync.
//
// # Verdict ladder (RFC-040 §3.4 precedence)
//
//   1. CONST_TABLE_DUPLICATE         — `entries_hash` equality
//                                      (canonical set-equality key,
//                                      RFC-040 §3.1).
//   2. CONST_TABLE_SUBSET            — one set is a strict subset of
//                                      the other (#3656 reproduction).
//   3. CONST_TABLE_INTERSECTION_HIGH — `entries_jaccard >= 0.5` AND
//                                      not subset.
//
// Verdicts 2 and 3 use the `entries_subset(a, b)` and
// `entries_jaccard(a, b)` UDFs operating over `entries_normalized`
// JSON strings (RFC §3.4). The precedence ordering itself is encoded
// by the `overlap_verdict(a_norm, b_norm, a_hash, b_hash) -> Str`
// UDF, which exists because the v0.1 Cypher subset has no `CASE WHEN`
// or `UNION` (`crates/cfdb-query/src/parser/mod.rs` §316–432) — the
// precedence-decoder MUST live in a UDF for this rule to emit a
// single `verdict` string column. Keeping the precedence semantics
// in one Rust function (rather than reimplemented in every consumer)
// is the canonical-resolver pattern (RFC-035 §3.3).
//
// # What this rule matches
//
// Two `:ConstTable` nodes declared in DIFFERENT crates (both
// non-test-mod), with matching `element_type`, where ANY of the three
// overlap conditions holds. The lexicographic `a.qname < b.qname`
// guard dedupes the symmetric pair so each unordered pair is
// reported once. The `verdict` column carries one of the three
// labels per the precedence ordering above.
//
// # Filters
//
// - Cross-crate only: `a.crate <> b.crate`. Two const tables in the
//   same crate that share entries are usually intentional (a
//   `pub use` re-export OR a deliberate alias); cross-crate is the
//   split-brain signal.
// - `is_test = false` for both: test fixtures legitimately redeclare
//   prod constants for unit-test isolation; flagging them is noise.
// - `element_type` matches: a `&[&str]` table cannot duplicate a
//   `&[u32]` table even with the same `entries_hash` collision (which
//   is a sha256 collision in practice, not a real overlap). The
//   element_type check makes the rule semantically scoped and is
//   load-bearing for the SUBSET / INTERSECTION_HIGH branches too —
//   `entries_subset` / `entries_jaccard` already return false / 0.0
//   on cross-element-type inputs, but the explicit filter avoids
//   computing them in the first place.
// - `a.qname < b.qname`: pair-dedup; report the pair once with `a`
//   as the lex-smaller qname.
// - `verdict <> 'CONST_TABLE_NONE'`: filter out pairs where none of
//   the three overlap conditions holds. The `overlap_verdict` UDF
//   returns `'CONST_TABLE_NONE'` for the no-overlap case so this
//   shape works with the v0.1 Cypher subset (no CASE WHEN); the
//   trailing WITH-WHERE filters those rows out.
//
// # Output columns
//
//   verdict       — one of:
//                     'CONST_TABLE_DUPLICATE'
//                     'CONST_TABLE_SUBSET'
//                     'CONST_TABLE_INTERSECTION_HIGH'
//   a_qname       — :ConstTable.qname of the lex-smaller member
//   a_crate       — :ConstTable.crate of the lex-smaller member
//   b_qname       — :ConstTable.qname of the lex-larger member
//   b_crate       — :ConstTable.crate of the lex-larger member
//   element_type  — shared element_type ("str" | "u32" | "i32" |
//                   "u64" | "i64")
//   entry_count   — :ConstTable.entry_count of `a` (consumer can
//                   compare to `b`'s if needed; same value for the
//                   DUPLICATE branch by definition)
//   sample        — :ConstTable.entries_sample of `a` (declaration-
//                   order JSON, first 8) for triage
//
// # Triage
//
// A row is a candidate `duplicated_feature` (RFC §A2.1 class 1).
// Route per §A2.3 SkillRoutingTable → `/sweep-epic` after manual
// confirmation that the pair is unintentional drift rather than a
// deliberate re-export. The `sample` column is the human-readable
// triage aid; the `verdict` column tells the reviewer how the rule
// classified the pair.
//
// # Determinism
//
// `ORDER BY verdict, a_qname, b_qname` gives a stable row order
// across runs. Validated by the §12.1 determinism CI recipe and the
// `const_table_overlap_rule_output_is_byte_stable` scar in
// `crates/cfdb-cli/tests/const_table_overlap.rs`.
//
// # Why every output column is materialised in WITH
//
// The v0.1 Cypher subset's WITH clause projects every column to a
// scalar `Binding::Value`; node references (`a`, `b`) are NOT carried
// across WITH (`crates/cfdb-petgraph/src/eval/with_clause.rs`). So
// the rule materialises every output column (`a_qname`, `a_crate`,
// `entries_sample`, ...) inside the WITH clause itself, then RETURN
// just renames each scalar. The trailing `WHERE` on WITH consumes
// the materialised `verdict` to drop `CONST_TABLE_NONE` rows.
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
WITH overlap_verdict(a.entries_normalized, b.entries_normalized,
                     a.entries_hash, b.entries_hash) AS verdict,
     a.qname AS a_qname,
     a.crate AS a_crate,
     b.qname AS b_qname,
     b.crate AS b_crate,
     a.element_type AS element_type,
     a.entry_count AS entry_count,
     a.entries_sample AS sample
WHERE verdict <> 'CONST_TABLE_NONE'
RETURN verdict AS verdict,
       a_qname AS a_qname,
       a_crate AS a_crate,
       b_qname AS b_qname,
       b_crate AS b_crate,
       element_type AS element_type,
       entry_count AS entry_count,
       sample AS sample
ORDER BY verdict ASC, a_qname ASC, b_qname ASC
