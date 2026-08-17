use std::path::Path;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::{Node, PropValue, Props};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

use crate::EnrichEngine;

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdirs");
    }
    std::fs::write(&path, contents).expect("write");
}

/// Build a store containing `:Item` nodes with given `(qname, name, crate,
/// bounded_context)` tuples. Useful for simulating a previously-extracted
/// keyspace.
fn store_with_items(workspace: &Path, items: &[(&str, &str, &str, &str)]) -> PetgraphStore {
    let mut store = PetgraphStore::new().with_workspace(workspace);
    let ks = Keyspace::new("test");
    let nodes: Vec<Node> = items
        .iter()
        .map(|(qname, name, crate_name, ctx)| {
            let mut props = Props::new();
            props.insert("qname".into(), PropValue::Str((*qname).into()));
            props.insert("name".into(), PropValue::Str((*name).into()));
            props.insert("crate".into(), PropValue::Str((*crate_name).into()));
            props.insert("bounded_context".into(), PropValue::Str((*ctx).into()));
            props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
            Node {
                id: format!("item:{qname}"),
                label: Label::new(Label::ITEM),
                props,
            }
        })
        .collect();
    store.ingest_nodes(&ks, nodes).expect("ingest");
    store
}

fn get_bounded_context(store: &PetgraphStore, keyspace: &Keyspace, qname: &str) -> String {
    let (nodes, _) = store.export(keyspace).expect("export");
    nodes
        .iter()
        .find(|n| {
            n.props
                .get("qname")
                .and_then(PropValue::as_str)
                .is_some_and(|q| q == qname)
        })
        .and_then(|n| n.props.get("bounded_context").and_then(PropValue::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| panic!("item {qname} or its bounded_context prop missing"))
}

// ------------------------------------------------------------------
// AC-1: TOML override patches mismatched items.
// ------------------------------------------------------------------

#[test]
fn ac1_toml_override_patches_mismatched_items() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Declare that `domain-trading` belongs to context `"trading"` (which
    // happens to match the heuristic — so we instead use a non-heuristic
    // mapping to prove the override wins).
    write(
        tmp.path(),
        ".cfdb/concepts/trading.toml",
        "name = \"custom-trading\"\ncrates = [\"domain-trading\"]\n",
    );
    // Extractor-time values: stale heuristic ("trading" from stripping
    // "domain-"). TOML now says "custom-trading" — re-enrichment must
    // patch.
    let mut store = store_with_items(
        tmp.path(),
        &[
            ("crate::A", "A", "domain-trading", "trading"),
            ("crate::B", "B", "domain-trading", "trading"),
        ],
    );
    let ks = Keyspace::new("test");
    let report = EnrichEngine::new(&mut store)
        .enrich_bounded_context(&ks)
        .expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 2);
    assert_eq!(report.attrs_written, 2, "both items patched");
    assert_eq!(
        get_bounded_context(&store, &ks, "crate::A"),
        "custom-trading"
    );
    assert_eq!(
        get_bounded_context(&store, &ks, "crate::B"),
        "custom-trading"
    );
}

// ------------------------------------------------------------------
// AC-2: no TOML changes (or no TOML at all) → no-op, ran=true.
// ------------------------------------------------------------------

#[test]
fn ac2_no_toml_is_noop_on_items_that_match_heuristic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // No `.cfdb/concepts/` directory. Heuristic applies:
    // `domain-trading` → "trading". Stored value already matches.
    let mut store = store_with_items(
        tmp.path(),
        &[("crate::A", "A", "domain-trading", "trading")],
    );
    let ks = Keyspace::new("test");
    let report = EnrichEngine::new(&mut store)
        .enrich_bounded_context(&ks)
        .expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 1);
    assert_eq!(report.attrs_written, 0, "no-op on fresh-extract values");
}

// ------------------------------------------------------------------
// AC-3: modified TOML → mismatched crates patched, matching ones untouched.
// ------------------------------------------------------------------

#[test]
fn ac3_only_mismatched_crates_patched() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/trading.toml",
        "name = \"trading-v2\"\ncrates = [\"domain-trading\"]\n",
    );
    // Two items in `domain-trading` (out of sync) + one in
    // `ports-trading` (where stored value "trading" already matches the
    // heuristic output for that crate; no override for ports-trading, so
    // it stays unchanged).
    let mut store = store_with_items(
        tmp.path(),
        &[
            ("crate::A", "A", "domain-trading", "trading"),
            ("crate::B", "B", "domain-trading", "trading"),
            ("crate::C", "C", "ports-trading", "trading"),
        ],
    );
    let ks = Keyspace::new("test");
    let report = EnrichEngine::new(&mut store)
        .enrich_bounded_context(&ks)
        .expect("pass");

    assert!(report.ran);
    assert_eq!(report.attrs_written, 2, "two domain-trading items patched");
    assert_eq!(get_bounded_context(&store, &ks, "crate::A"), "trading-v2");
    assert_eq!(get_bounded_context(&store, &ks, "crate::B"), "trading-v2");
    assert_eq!(get_bounded_context(&store, &ks, "crate::C"), "trading");
}

// ------------------------------------------------------------------
// AC-7: two runs on identical workspace + TOML produce byte-identical
// canonical dumps.
// ------------------------------------------------------------------

#[test]
fn ac7_two_runs_produce_identical_canonical_dumps() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/trading.toml",
        "name = \"custom-trading\"\ncrates = [\"domain-trading\"]\n",
    );

    fn build(root: &Path) -> PetgraphStore {
        let mut store = PetgraphStore::new().with_workspace(root);
        let ks = Keyspace::new("test");
        for (q, n, c, ctx) in [
            ("crate::A", "A", "domain-trading", "trading"),
            ("crate::B", "B", "ports-trading", "trading"),
        ] {
            let mut props = Props::new();
            props.insert("qname".into(), PropValue::Str(q.into()));
            props.insert("name".into(), PropValue::Str(n.into()));
            props.insert("crate".into(), PropValue::Str(c.into()));
            props.insert("bounded_context".into(), PropValue::Str(ctx.into()));
            store
                .ingest_nodes(
                    &ks,
                    vec![Node {
                        id: format!("item:{q}"),
                        label: Label::new(Label::ITEM),
                        props,
                    }],
                )
                .expect("ingest");
        }
        store
    }

    let ks = Keyspace::new("test");
    let mut s1 = build(tmp.path());
    EnrichEngine::new(&mut s1)
        .enrich_bounded_context(&ks)
        .expect("run 1");
    let mut s2 = build(tmp.path());
    EnrichEngine::new(&mut s2)
        .enrich_bounded_context(&ks)
        .expect("run 2");
    let d1 = s1.canonical_dump(&ks).expect("dump 1");
    let d2 = s2.canonical_dump(&ks).expect("dump 2");
    assert_eq!(d1, d2, "two runs must be byte-identical (AC-7)");
}

// ------------------------------------------------------------------
// Degraded paths
// ------------------------------------------------------------------

#[test]
fn malformed_toml_returns_ran_false_with_warning() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/broken.toml",
        "this is = not [valid toml",
    );
    let mut store = store_with_items(
        tmp.path(),
        &[("crate::A", "A", "domain-trading", "trading")],
    );
    let ks = Keyspace::new("test");
    let report = EnrichEngine::new(&mut store)
        .enrich_bounded_context(&ks)
        .expect("pass");

    assert!(!report.ran, "TOML load error → ran=false");
    assert_eq!(report.attrs_written, 0);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("concepts") || w.contains("toml")),
        "warning must name the load failure: {:?}",
        report.warnings
    );
}

#[test]
fn empty_keyspace_returns_ran_true_with_zero_counters() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let ks = Keyspace::new("test");
    store.ingest_nodes(&ks, Vec::new()).expect("ingest empty");
    let report = EnrichEngine::new(&mut store)
        .enrich_bounded_context(&ks)
        .expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 0);
    assert_eq!(report.attrs_written, 0);
}

#[test]
fn unknown_keyspace_returns_err() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let ks = Keyspace::new("never");
    let err = EnrichEngine::new(&mut store)
        .enrich_bounded_context(&ks)
        .expect_err("unknown keyspace must err");
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

#[test]
fn no_workspace_root_returns_degraded_report() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str("crate::A".into()));
    props.insert("name".into(), PropValue::Str("A".into()));
    props.insert("crate".into(), PropValue::Str("domain-x".into()));
    props.insert("bounded_context".into(), PropValue::Str("x".into()));
    store
        .ingest_nodes(
            &ks,
            vec![Node {
                id: "item:crate::A".into(),
                label: Label::new(Label::ITEM),
                props,
            }],
        )
        .expect("ingest");
    let report = EnrichEngine::new(&mut store)
        .enrich_bounded_context(&ks)
        .expect("pass");
    assert!(!report.ran);
    assert!(report.warnings.iter().any(|w| w.contains("workspace_root")));
}

#[test]
fn unknown_keyspace_errs_even_when_workspace_root_is_also_missing() {
    // The keyspace guard wins when both fail — never the degraded report.
    let mut store = PetgraphStore::new(); // no workspace root
    let ks = Keyspace::new("never"); // and no such keyspace
    let err = EnrichEngine::new(&mut store)
        .enrich_bounded_context(&ks)
        .expect_err("keyspace guard must win over the workspace guard");
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}
