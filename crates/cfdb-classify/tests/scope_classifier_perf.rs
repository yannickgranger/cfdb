use std::collections::BTreeMap;
use std::time::Duration;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::query::ParamBinding;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::{QueryBackend, StoreBackend};
use cfdb_eval::QueryEngine;
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
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "reachable_from_entry".into(),
                notes: "perf bench — narrows RandomScattering candidates via slice-5".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "is_test".into(),
                notes: "perf bench — every classifier filters is_test=false".into(),
            },
            IndexEntry::Computed {
                label: "Item".into(),
                computed: ComputedKey::LastSegment,
                notes: "perf bench — homonym bucket key".into(),
            },
            IndexEntry::Computed {
                label: "Item".into(),
                computed: ComputedKey::ConversionPrefix,
                notes: "perf bench — RandomScattering fork-join bucket".into(),
            },
        ],
    }
}

fn without_conversion_prefix_index_spec() -> IndexSpec {
    IndexSpec {
        entries: production_index_spec()
            .entries
            .into_iter()
            .filter(|e| {
                !matches!(
                    e,
                    IndexEntry::Computed { computed, .. }
                        if *computed == ComputedKey::ConversionPrefix
                )
            })
            .collect(),
    }
}

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

fn build_fixture(n: usize) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(n);
    let label = || Label::new("Item");

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

fn thread_cpu_time() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(rc, 0, "clock_gettime(CLOCK_THREAD_CPUTIME_ID) failed");
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

fn run_rule(
    store: &PetgraphStore,
    ks: &Keyspace,
    cypher: &str,
    context: &str,
) -> (usize, Duration) {
    let mut parsed = parse(cypher).expect("classifier rule parses");
    parsed.params.insert(
        "context".to_string(),
        ParamBinding::Scalar(PropValue::Str(context.to_string())),
    );
    let start = thread_cpu_time();
    let result = QueryEngine::new(store)
        .execute(ks, &parsed)
        .expect("execute classifier");
    let elapsed = thread_cpu_time() - start;
    (result.rows.len(), elapsed)
}

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

    println!("\nscope_classifier_perf (fixture size n={n}):");
    for (name, (rows, elapsed)) in &timings {
        println!("  {name:<20} rows={rows:<5} cpu={elapsed:?}");
    }

    if n <= 1_500 {
        let budgets: [(&str, Duration); 6] = [
            ("DuplicatedFeature", Duration::from_millis(50)),
            ("ContextHomonym", Duration::from_millis(50)),
            ("UnfinishedRefactor", Duration::from_millis(25)),
            ("RandomScattering", Duration::from_millis(150)),
            ("CanonicalBypass", Duration::from_millis(10)),
            ("Unwired", Duration::from_millis(50)),
        ];
        for (name, budget) in budgets {
            let (_rows, elapsed) = timings
                .get(name)
                .copied()
                .expect("every rule timed exactly once");
            assert!(
                elapsed <= budget,
                "classifier `{name}` exceeded perf budget at n={n}: \
                 thread-cpu={elapsed:?} > budget={budget:?}. \
                 If this is a deliberate change, update the budget \
                 here in scope_classifier_perf.rs and document why."
            );
        }
    }
}

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

#[test]
fn scope_classifier_slice6b_prop_eq_fast_path_beats_full_scan_10x() {
    let n = fixture_scale().max(2_000);
    let (with_name_store, with_name_ks) = build_store(production_index_spec(), n);
    let (without_name_store, without_name_ks) = build_store(pre_fix_index_spec(), n);

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

#[test]
fn scope_classifier_slice6_fast_path_beats_full_scan_10x() {
    let n = fixture_scale().max(2_000);
    let (indexed_store, indexed_ks) = build_store(production_index_spec(), n);
    let (bare_store, bare_ks) = build_store(IndexSpec::empty(), n);

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

#[test]
fn scope_classifier_slice6c_conversion_prefix_fast_path_beats_full_scan_10x() {
    let n = fixture_scale().max(6_000);
    let (with_store, with_ks) = build_store(production_index_spec(), n);
    let (without_store, without_ks) = build_store(without_conversion_prefix_index_spec(), n);

    let _ = run_rule(&with_store, &with_ks, RANDOM_SCATTERING_CYPHER, CTX);
    let _ = run_rule(&without_store, &without_ks, RANDOM_SCATTERING_CYPHER, CTX);

    let (with_rows, with_elapsed) = run_rule(&with_store, &with_ks, RANDOM_SCATTERING_CYPHER, CTX);
    let (without_rows, without_elapsed) =
        run_rule(&without_store, &without_ks, RANDOM_SCATTERING_CYPHER, CTX);

    println!(
        "slice-6c conversion-prefix fast-path comparison at n={n}: \
         with conversion_prefix indexed={with_elapsed:?} (rows={with_rows}) vs \
         without={without_elapsed:?} (rows={without_rows})"
    );

    assert_eq!(
        with_rows, without_rows,
        "RandomScattering must return the same row set whether or not \
         conversion_prefix(name) is indexed — the fast path is a pure optimisation"
    );

    let ratio = without_elapsed.as_nanos() as f64 / with_elapsed.as_nanos().max(1) as f64;
    assert!(
        ratio >= 10.0,
        "slice-6c conversion-prefix fast path should be ≥10× faster than full scan; \
         observed ratio={ratio:.1}× (with-index={with_elapsed:?}, \
         without={without_elapsed:?}). If this drops below 10×, the conversion-prefix \
         hint in `resolve_cross_ref_computed_hint` is no longer firing (check the \
         byte-for-byte pattern-literal recognition)."
    );
}
