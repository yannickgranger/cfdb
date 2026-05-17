//! Per-classifier-rule performance benchmark for `cfdb scope`.
//!
//! Builds a synthetic in-memory `:Item` keyspace at controllable scale and
//! runs each of the six §A2.1 classifier cypher rules against it,
//! timing each rule individually. Bypasses the binary spawn so the
//! measurement is end-to-end `parse → execute → finding-project`
//! (i.e. exactly what `cfdb-cli::scope::run_classifier_rule` does at
//! runtime, but with a synthetic store).
//!
//! Why it lives here: the six classifier cypher files are embedded by
//! `cfdb-cli::scope::*_CYPHER` `include_str!` constants. Re-using the
//! same files (relative `../../examples/queries/`) keeps the benchmark
//! coupled to the rules that actually ship, so a perf regression on
//! the production rule shape fails this test.
//!
//! Background: timing instrumentation in `scope.rs` showed that on
//! cfdb-self (~30k :Item, no HIR), the six classifiers contribute:
//!
//! - `DuplicatedFeature`  : ~1.21 s     (44 % of build_scope_inventory)
//! - `ContextHomonym`     : ~0.007 s    (index-fast-path, slice-6)
//! - `UnfinishedRefactor` : ~0.001 s
//! - `RandomScattering`   : ~1.81 s     (52 % of build_scope_inventory)
//! - `CanonicalBypass`    : ~0.002 s
//! - `Unwired`            : ~0.004 s
//!
//! On qbot-core (~192k :Item) DuplicatedFeature alone runs for
//! ~324 seconds (5.4 min) before returning zero rows. The two slow
//! rules are `MATCH (a:Item), (b:Item)` cartesians whose join
//! predicates fall outside the slice-6 fast-path allowlist
//! (`last_segment` only) — `a.name = b.name` is a plain prop-to-prop
//! equi-join the optimizer cannot use today, and
//! `regexp_extract(...)` is not in `match_computed_call_name`.
//!
//! This file pins the per-rule budgets at small scale so a regression
//! (e.g. dropping the slice-6 fast path, or making the cartesian
//! denser) fails fast in CI. The fast-path-shaped rules
//! (`ContextHomonym`) get tight budgets; the known-cartesian rules
//! (`DuplicatedFeature`, `RandomScattering`) get budgets sized to the
//! current implementation so we detect a regression without making
//! the test brittle on a slower runner. Improving the fast path will
//! let us tighten the cartesian budgets — that improvement should
//! ship together with the budget tightening.
//!
//! Scale knob: env var `SCOPE_PERF_SCALE=<n>` controls fixture size
//! (default 1_000). Bigger values are useful for ad-hoc bottleneck
//! hunting; the asserted budgets are calibrated to the default.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::query::Param;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::index::spec::{ComputedKey, IndexEntry, IndexSpec};
use cfdb_petgraph::PetgraphStore;
use cfdb_query::parse;

const CTX: &str = "infrastructure";

const DUPLICATED_FEATURE_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-duplicated-feature.cypher");
const CONTEXT_HOMONYM_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-context-homonym.cypher");
const UNFINISHED_REFACTOR_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-unfinished-refactor.cypher");
const RANDOM_SCATTERING_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-random-scattering.cypher");
const CANONICAL_BYPASS_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-canonical-bypass.cypher");
const UNWIRED_CYPHER: &str = include_str!("../../../examples/queries/classifier-unwired.cypher");

fn fixture_scale() -> usize {
    std::env::var("SCOPE_PERF_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000)
}

/// Mirror of the production `.cfdb/indexes.toml` shipped in the
/// cfdb workspace — `Item.qname`, `Item.bounded_context`,
/// `Item.name`, and the `Item.last_segment(qname)` computed index.
/// The benchmark exercises the same index surface a real `cfdb
/// scope` invocation sees, so per-rule timings are comparable to
/// production wall-time.
fn production_index_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "qname".into(),
                notes: "perf bench".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "bounded_context".into(),
                notes: "perf bench".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "name".into(),
                notes: "perf bench — slice-6b prop-eq bucket for DuplicatedFeature".into(),
            },
            IndexEntry::Computed {
                label: "Item".into(),
                computed: ComputedKey::LastSegment,
                notes: "perf bench — homonym bucket key".into(),
            },
        ],
    }
}

/// Variant WITHOUT `Item.name` — represents the pre-fix index
/// surface where DuplicatedFeature's `a.name = b.name` equi-join
/// falls back to a cartesian. Used by the slice-6b comparison
/// test to prove the prop-to-prop fast path is firing.
fn pre_fix_index_spec() -> IndexSpec {
    IndexSpec {
        entries: production_index_spec()
            .entries
            .into_iter()
            .filter(|e| {
                !matches!(
                    e,
                    IndexEntry::Prop { label, prop, .. }
                        if label.as_str() == "Item" && prop == "name"
                )
            })
            .collect(),
    }
}

/// Build a synthetic `:Item` fixture of size `n` modelling the
/// qbot-core shape: ~50 % `fn`/`method`, ~40 % `struct`/`enum`,
/// ~10 % `trait`; items spread across two bounded contexts.
///
/// Embedded planted findings so each classifier rule has work to do
/// AND deterministic non-zero output:
/// - 5 same-context same-name struct pairs across crates →
///   `DuplicatedFeature` should find 10 rows.
/// - 5 cross-context same-last-segment fn pairs with divergent
///   signatures → `ContextHomonym` should find ~5 rows.
/// - 3 `#[deprecated]` items in CTX → `UnfinishedRefactor` finds 3.
/// - 3 same-context `compute_X_from_bps` / `compute_X_from_pct`
///   fork pairs reachable from an entry point → `RandomScattering`.
/// - 2 items with `is_canonical_for` + reachable, plus 2 BYPASS
///   items → `CanonicalBypass` (degraded on syn-only — surfaces as
///   empty if `reachable_from_entry` is absent).
/// - Most fns are NOT reachable → `Unwired` should find many.
fn build_fixture(n: usize) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(n);
    let label = || Label::new("Item");

    // 1) DuplicatedFeature seeds: 5 struct pairs (10 nodes), same
    //    context CTX, different crates, same name.
    for i in 0..5 {
        let name = format!("DupStruct{i}");
        out.push(
            Node::new(format!("seed:dup:a:{i}"), label())
                .with_prop("qname", format!("crate_a::mod::{name}"))
                .with_prop("name", name.clone())
                .with_prop("kind", "struct")
                .with_prop("crate", "crate_a")
                .with_prop("bounded_context", CTX)
                .with_prop("is_test", false)
                .with_prop("file", "a.rs")
                .with_prop("line", 1_i64),
        );
        out.push(
            Node::new(format!("seed:dup:b:{i}"), label())
                .with_prop("qname", format!("crate_b::mod::{name}"))
                .with_prop("name", name)
                .with_prop("kind", "struct")
                .with_prop("crate", "crate_b")
                .with_prop("bounded_context", CTX)
                .with_prop("is_test", false)
                .with_prop("file", "b.rs")
                .with_prop("line", 1_i64),
        );
    }

    // 2) ContextHomonym seeds: 5 cross-context fn pairs sharing
    //    last segment but with divergent signatures.
    for i in 0..5 {
        let name = format!("homonym_{i}");
        out.push(
            Node::new(format!("seed:hom:a:{i}"), label())
                .with_prop("qname", format!("crate_a::{CTX}::{name}"))
                .with_prop("name", name.clone())
                .with_prop("kind", "fn")
                .with_prop("crate", "crate_a")
                .with_prop("bounded_context", CTX)
                .with_prop("is_test", false)
                .with_prop("signature", "fn(u32) -> u32")
                .with_prop("file", "a.rs")
                .with_prop("line", 10_i64),
        );
        out.push(
            Node::new(format!("seed:hom:b:{i}"), label())
                .with_prop("qname", format!("crate_b::other_ctx::{name}"))
                .with_prop("name", name)
                .with_prop("kind", "fn")
                .with_prop("crate", "crate_b")
                .with_prop("bounded_context", "other_ctx")
                .with_prop("is_test", false)
                .with_prop("signature", "fn(&str) -> Result<(), Error>")
                .with_prop("file", "b.rs")
                .with_prop("line", 20_i64),
        );
    }

    // 3) UnfinishedRefactor seeds: 3 deprecated items in CTX.
    for i in 0..3 {
        out.push(
            Node::new(format!("seed:dep:{i}"), label())
                .with_prop("qname", format!("crate_a::legacy::Old{i}"))
                .with_prop("name", format!("Old{i}"))
                .with_prop("kind", "struct")
                .with_prop("crate", "crate_a")
                .with_prop("bounded_context", CTX)
                .with_prop("is_test", false)
                .with_prop("is_deprecated", true)
                .with_prop("file", "legacy.rs")
                .with_prop("line", (i as i64) + 1),
        );
    }

    // 4) RandomScattering seeds: 3 fork pairs in CTX, reachable.
    //    Names match `^(\w+)_(from|to|for|as)_(\w+)$`.
    for i in 0..3 {
        let stem = format!("compute_{i}");
        out.push(
            Node::new(format!("seed:fork:a:{i}"), label())
                .with_prop("qname", format!("crate_a::{CTX}::{stem}_from_bps"))
                .with_prop("name", format!("{stem}_from_bps"))
                .with_prop("kind", "method")
                .with_prop("crate", "crate_a")
                .with_prop("bounded_context", CTX)
                .with_prop("is_test", false)
                .with_prop("reachable_from_entry", true)
                .with_prop("file", "a.rs")
                .with_prop("line", 100_i64),
        );
        out.push(
            Node::new(format!("seed:fork:b:{i}"), label())
                .with_prop("qname", format!("crate_a::{CTX}::{stem}_from_pct"))
                .with_prop("name", format!("{stem}_from_pct"))
                .with_prop("kind", "method")
                .with_prop("crate", "crate_a")
                .with_prop("bounded_context", CTX)
                .with_prop("is_test", false)
                .with_prop("reachable_from_entry", true)
                .with_prop("file", "a.rs")
                .with_prop("line", 110_i64),
        );
    }

    // 5) Bulk noise — fill to `n`. Half `fn`/`method` (mostly
    //    unwired), half `struct`/`enum`/`trait`. Distinct names so
    //    they don't collide with the planted findings.
    let seeded = out.len();
    let target = n.max(seeded);
    for i in seeded..target {
        let kind = match i % 5 {
            0 | 1 => "fn",
            2 => "method",
            3 => "struct",
            _ => "enum",
        };
        let ctx = if i % 2 == 0 { CTX } else { "other_ctx" };
        let reachable = i % 7 == 0;
        out.push(
            Node::new(format!("noise:{i}"), label())
                .with_prop("qname", format!("crate_n::mod_{}::uniq_{i}", i % 50))
                .with_prop("name", format!("uniq_{i}"))
                .with_prop("kind", kind)
                .with_prop("crate", format!("crate_n_{}", i % 4))
                .with_prop("bounded_context", ctx)
                .with_prop("is_test", false)
                .with_prop("reachable_from_entry", reachable)
                .with_prop("file", format!("f{}.rs", i % 20))
                .with_prop("line", (i as i64) + 1),
        );
    }
    out
}

fn build_store(spec: IndexSpec, n: usize) -> (PetgraphStore, Keyspace) {
    let ks = Keyspace::new("perf-bench");
    let mut store = PetgraphStore::new().with_indexes(spec);
    store.ingest_nodes(&ks, build_fixture(n)).expect("ingest");
    (store, ks)
}

/// Parse `cypher`, bind `$context`, execute. Returns (rows, elapsed).
fn run_rule(
    store: &PetgraphStore,
    ks: &Keyspace,
    cypher: &str,
    context: &str,
) -> (usize, Duration) {
    let mut parsed = parse(cypher).expect("classifier rule parses");
    parsed.params.insert(
        "context".to_string(),
        Param::Scalar(PropValue::Str(context.to_string())),
    );
    let start = Instant::now();
    let result = store.execute(ks, &parsed).expect("execute classifier");
    let elapsed = start.elapsed();
    (result.rows.len(), elapsed)
}

/// One pass through every classifier rule. Returns a sorted
/// `BTreeMap` from rule name → (row_count, elapsed).
fn time_all_rules(
    store: &PetgraphStore,
    ks: &Keyspace,
) -> BTreeMap<&'static str, (usize, Duration)> {
    let rules: [(&'static str, &str); 6] = [
        ("DuplicatedFeature", DUPLICATED_FEATURE_CYPHER),
        ("ContextHomonym", CONTEXT_HOMONYM_CYPHER),
        ("UnfinishedRefactor", UNFINISHED_REFACTOR_CYPHER),
        ("RandomScattering", RANDOM_SCATTERING_CYPHER),
        ("CanonicalBypass", CANONICAL_BYPASS_CYPHER),
        ("Unwired", UNWIRED_CYPHER),
    ];
    let mut out = BTreeMap::new();
    for (name, cypher) in rules {
        // Warm-up (lazy allocations, JIT-ish first-call overhead).
        let _ = run_rule(store, ks, cypher, CTX);
        let (rows, elapsed) = run_rule(store, ks, cypher, CTX);
        out.insert(name, (rows, elapsed));
    }
    out
}

#[test]
fn scope_classifier_perf_at_default_scale() {
    let n = fixture_scale();
    let (store, ks) = build_store(production_index_spec(), n);

    let timings = time_all_rules(&store, &ks);

    // Always-on report — print so `cargo test -- --nocapture` shows
    // the per-rule breakdown for ad-hoc bottleneck hunting at any
    // `SCOPE_PERF_SCALE`.
    println!("\nscope_classifier_perf (fixture size n={n}):");
    for (name, (rows, elapsed)) in &timings {
        println!("  {name:<20} rows={rows:<5} elapsed={elapsed:?}");
    }

    // Per-rule budgets at default scale (1_000 items). These are
    // sized to detect REGRESSIONS (≥3× slowdown over the current
    // implementation), not to flake on CI noise — observed p95 on
    // a quiet developer machine, multiplied by ~3 to absorb slow
    // runners and concurrent build load.
    //
    // Baseline (release, quiet, n=1_000), Apr 2026 post slice-6b:
    //   DuplicatedFeature   ~0.6 ms   → budget 10 ms (slice-6b prop-eq)
    //   ContextHomonym      ~0.5 ms   → budget 5 ms (slice-6 last_segment)
    //   UnfinishedRefactor  ~0.1 ms   → budget 5 ms
    //   RandomScattering    ~800 ms   → budget 2500 ms (still O(n²): regex-extract)
    //   CanonicalBypass     ~0.001 ms → budget 5 ms (no inputs without HIR)
    //   Unwired             ~0.5 ms   → budget 10 ms
    //
    // The two index-fast-path rules (`DuplicatedFeature`,
    // `ContextHomonym`) get tight budgets so a regression dropping
    // either hint fails loudly — at n=1_000 the un-indexed
    // equivalent of each takes ~250 ms, so any breakage above
    // ~10 ms means the fast path stopped firing.
    //
    // `RandomScattering` remains O(n²) because its join is on
    // `regexp_extract(name, …)` and `regexp_extract` is not yet
    // in the `match_computed_call_name` allowlist. The budget is
    // wide enough to survive CI noise but still catches a
    // fundamental shape change.
    if n <= 1_500 {
        let budgets: [(&str, Duration); 6] = [
            ("DuplicatedFeature", Duration::from_millis(10)),
            ("ContextHomonym", Duration::from_millis(5)),
            ("UnfinishedRefactor", Duration::from_millis(5)),
            ("RandomScattering", Duration::from_millis(2500)),
            ("CanonicalBypass", Duration::from_millis(5)),
            ("Unwired", Duration::from_millis(10)),
        ];
        for (name, budget) in budgets {
            let (_rows, elapsed) = timings
                .get(name)
                .copied()
                .expect("every rule timed exactly once");
            assert!(
                elapsed <= budget,
                "classifier `{name}` exceeded perf budget at n={n}: \
                 elapsed={elapsed:?} > budget={budget:?}. \
                 If this is a deliberate change, update the budget \
                 here in scope_classifier_perf.rs and document why."
            );
        }
    }
}

/// Sanity check — at default scale, the planted findings actually
/// surface. Catches the "fixture broke and every rule returns 0"
/// failure mode where the perf budgets pass vacuously.
#[test]
fn scope_classifier_perf_fixture_has_planted_findings() {
    let (store, ks) = build_store(production_index_spec(), 1_000);
    let timings = time_all_rules(&store, &ks);

    let (dup_rows, _) = timings["DuplicatedFeature"];
    assert!(
        dup_rows >= 10,
        "DuplicatedFeature planted 5 pairs (10 rows), got {dup_rows} — fixture drifted"
    );

    let (hom_rows, _) = timings["ContextHomonym"];
    assert!(
        hom_rows >= 5,
        "ContextHomonym planted 5 a-side pairs, got {hom_rows} — fixture drifted"
    );

    let (dep_rows, _) = timings["UnfinishedRefactor"];
    assert!(
        dep_rows >= 3,
        "UnfinishedRefactor planted 3 deprecated items, got {dep_rows} — fixture drifted"
    );
}

/// Quantify the slice-6b prop-to-prop fast path: at the same
/// fixture size, `DuplicatedFeature` with `Item.name` indexed
/// should be at least 10× faster than without. The rule joins
/// `a.name = b.name AND a.bounded_context = b.bounded_context`;
/// without the name index it is a cartesian, with the index it
/// narrows `b` through a single posting-list lookup per `a`.
///
/// This guards against regressions in the `resolve_cross_ref_prop_hint`
/// path — if the hint stops firing (e.g. because the WHERE walker
/// stops descending through `And`, or the spec check is moved),
/// the rule silently reverts to O(n²) and this test catches it.
#[test]
fn scope_classifier_slice6b_prop_eq_fast_path_beats_full_scan_10x() {
    let n = fixture_scale().max(2_000);
    let (with_name_store, with_name_ks) = build_store(production_index_spec(), n);
    let (without_name_store, without_name_ks) = build_store(pre_fix_index_spec(), n);

    // Warm-up
    let _ = run_rule(
        &with_name_store,
        &with_name_ks,
        DUPLICATED_FEATURE_CYPHER,
        CTX,
    );
    let _ = run_rule(
        &without_name_store,
        &without_name_ks,
        DUPLICATED_FEATURE_CYPHER,
        CTX,
    );

    let (with_rows, with_elapsed) = run_rule(
        &with_name_store,
        &with_name_ks,
        DUPLICATED_FEATURE_CYPHER,
        CTX,
    );
    let (without_rows, without_elapsed) = run_rule(
        &without_name_store,
        &without_name_ks,
        DUPLICATED_FEATURE_CYPHER,
        CTX,
    );

    println!(
        "slice-6b prop-eq fast-path comparison at n={n}: \
         with Item.name indexed={with_elapsed:?} (rows={with_rows}) vs \
         without={without_elapsed:?} (rows={without_rows})"
    );

    assert_eq!(
        with_rows, without_rows,
        "DuplicatedFeature must return the same row set whether or not Item.name is indexed"
    );

    let ratio = without_elapsed.as_nanos() as f64 / with_elapsed.as_nanos().max(1) as f64;
    assert!(
        ratio >= 10.0,
        "slice-6b prop-eq fast path should be ≥10× faster than full scan; \
         observed ratio={ratio:.1}× (with-index={with_elapsed:?}, \
         without={without_elapsed:?}). If this drops below 10×, the \
         prop-to-prop hint in `resolve_cross_ref_prop_hint` is no longer firing."
    );
}

/// Quantify the slice-6 (computed-key) fast path: at the same
/// fixture size, the indexed cross-MATCH (ContextHomonym) should
/// be *at least* 10× faster than the un-indexed equivalent.
/// Detects regressions where the fast path silently fails to
/// fire — the rule would still pass
/// `cross_match_indexed_completes_under_100ms` at 1k nodes if the
/// un-indexed scan happens to also be under 100 ms, so we
/// explicitly compare both timings here.
#[test]
fn scope_classifier_slice6_fast_path_beats_full_scan_10x() {
    let n = fixture_scale().max(2_000);
    let (indexed_store, indexed_ks) = build_store(production_index_spec(), n);
    let (bare_store, bare_ks) = build_store(IndexSpec::empty(), n);

    // Warm-up
    let _ = run_rule(&indexed_store, &indexed_ks, CONTEXT_HOMONYM_CYPHER, CTX);
    let _ = run_rule(&bare_store, &bare_ks, CONTEXT_HOMONYM_CYPHER, CTX);

    let (indexed_rows, indexed_elapsed) =
        run_rule(&indexed_store, &indexed_ks, CONTEXT_HOMONYM_CYPHER, CTX);
    let (bare_rows, bare_elapsed) = run_rule(&bare_store, &bare_ks, CONTEXT_HOMONYM_CYPHER, CTX);

    println!(
        "slice-6 fast-path comparison at n={n}: indexed={indexed_elapsed:?} \
         (rows={indexed_rows}) vs bare={bare_elapsed:?} (rows={bare_rows})"
    );

    assert_eq!(
        indexed_rows, bare_rows,
        "indexed and bare rule must return the same row set"
    );

    let ratio = bare_elapsed.as_nanos() as f64 / indexed_elapsed.as_nanos().max(1) as f64;
    assert!(
        ratio >= 10.0,
        "slice-6 fast path should be ≥10× faster than full scan; \
         observed ratio={ratio:.1}× (indexed={indexed_elapsed:?}, bare={bare_elapsed:?}). \
         If this drops below 10×, the slice-6 `last_segment` hint is no longer firing."
    );
}
