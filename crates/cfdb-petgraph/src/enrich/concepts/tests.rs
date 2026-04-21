use std::path::Path;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::{Node, PropValue, Props};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::PetgraphStore;

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdirs");
    }
    std::fs::write(&path, contents).expect("write");
}

fn store_with_items(workspace: &Path, items: &[(&str, &str)]) -> PetgraphStore {
    let mut store = PetgraphStore::new().with_workspace(workspace);
    let ks = Keyspace::new("test");
    let nodes: Vec<Node> = items
        .iter()
        .map(|(qname, crate_name)| {
            let mut props = Props::new();
            props.insert("qname".into(), PropValue::Str((*qname).into()));
            props.insert("name".into(), PropValue::Str((*qname).into()));
            props.insert("crate".into(), PropValue::Str((*crate_name).into()));
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

fn count_nodes_by_label(store: &PetgraphStore, ks: &Keyspace, label: &str) -> usize {
    let (nodes, _) = store.export(ks).expect("export");
    nodes.iter().filter(|n| n.label.as_str() == label).count()
}

fn count_edges_by_label(store: &PetgraphStore, ks: &Keyspace, label: &str) -> usize {
    let (_, edges) = store.export(ks).expect("export");
    edges.iter().filter(|e| e.label.as_str() == label).count()
}

// ------------------------------------------------------------------
// AC-1: TOML with canonical_crate → :Concept + LABELED_AS + CANONICAL_FOR.
// ------------------------------------------------------------------

#[test]
fn ac1_toml_emits_concept_plus_labeled_as_plus_canonical_for() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/trading.toml",
        r#"
name = "trading"
canonical_crate = "domain-trading"
crates = ["domain-trading", "ports-trading"]
"#,
    );
    let mut store = store_with_items(
        tmp.path(),
        &[
            ("A", "domain-trading"),
            ("B", "domain-trading"),
            ("C", "ports-trading"),
            ("D", "application-x"), // not covered → no edges
        ],
    );
    let ks = Keyspace::new("test");
    let report = store.enrich_concepts(&ks).expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 1, "one declared concept");
    assert_eq!(count_nodes_by_label(&store, &ks, Label::CONCEPT), 1);
    // LABELED_AS: A, B (domain-trading) + C (ports-trading) = 3.
    assert_eq!(
        count_edges_by_label(&store, &ks, EdgeLabel::LABELED_AS),
        3,
        "3 items in TOML-listed crates → 3 LABELED_AS edges"
    );
    // CANONICAL_FOR: A + B (both in domain-trading) = 2.
    assert_eq!(
        count_edges_by_label(&store, &ks, EdgeLabel::CANONICAL_FOR),
        2,
        "2 items in canonical_crate → 2 CANONICAL_FOR edges"
    );
}

// ------------------------------------------------------------------
// AC-2: empty .cfdb/concepts/ → ran=true, zero emissions.
// ------------------------------------------------------------------

#[test]
fn ac2_empty_concepts_dir_is_graceful_noop() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Create the dir but put no .toml files in it.
    std::fs::create_dir_all(tmp.path().join(".cfdb/concepts")).expect("mkdir");
    let mut store = store_with_items(tmp.path(), &[("A", "domain-x")]);
    let ks = Keyspace::new("test");
    let report = store.enrich_concepts(&ks).expect("pass");

    assert!(report.ran, "empty concepts dir is a valid workspace shape");
    assert_eq!(report.facts_scanned, 0);
    assert_eq!(report.attrs_written, 0);
    assert_eq!(report.edges_written, 0);
    assert!(
        report.warnings.is_empty(),
        "no warning expected for empty concepts dir"
    );
}

#[test]
fn no_concepts_dir_is_graceful_noop() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // No .cfdb directory at all.
    let mut store = store_with_items(tmp.path(), &[("A", "domain-x")]);
    let ks = Keyspace::new("test");
    let report = store.enrich_concepts(&ks).expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 0);
    assert_eq!(report.edges_written, 0);
}

// ------------------------------------------------------------------
// AC-3: malformed TOML → ran=false + warning naming the file.
// ------------------------------------------------------------------

#[test]
fn ac3_malformed_toml_returns_ran_false_with_warning() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/broken.toml",
        "this is = not [valid toml",
    );
    let mut store = store_with_items(tmp.path(), &[("A", "domain-x")]);
    let ks = Keyspace::new("test");
    let report = store.enrich_concepts(&ks).expect("pass");

    assert!(!report.ran, "TOML load error → ran=false");
    assert_eq!(report.edges_written, 0);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("concepts") || w.contains("toml") || w.contains("broken")),
        "warning must name the problem file: {:?}",
        report.warnings
    );
}

// ------------------------------------------------------------------
// AC-5: determinism across two runs.
// ------------------------------------------------------------------

#[test]
fn ac5_two_runs_produce_identical_canonical_dumps() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/trading.toml",
        r#"
name = "trading"
canonical_crate = "domain-trading"
crates = ["domain-trading", "ports-trading"]
"#,
    );
    write(
        tmp.path(),
        ".cfdb/concepts/risk.toml",
        r#"
name = "risk"
crates = ["domain-risk"]
"#,
    );

    fn build(root: &Path) -> PetgraphStore {
        let mut store = PetgraphStore::new().with_workspace(root);
        let ks = Keyspace::new("test");
        for (q, c) in [
            ("A", "domain-trading"),
            ("B", "ports-trading"),
            ("C", "domain-risk"),
        ] {
            let mut props = Props::new();
            props.insert("qname".into(), PropValue::Str(q.into()));
            props.insert("name".into(), PropValue::Str(q.into()));
            props.insert("crate".into(), PropValue::Str(c.into()));
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
    s1.enrich_concepts(&ks).expect("run 1");
    let mut s2 = build(tmp.path());
    s2.enrich_concepts(&ks).expect("run 2");
    let d1 = s1.canonical_dump(&ks).expect("dump 1");
    let d2 = s2.canonical_dump(&ks).expect("dump 2");
    assert_eq!(d1, d2, "two runs must be byte-identical (AC-5)");
}

// ------------------------------------------------------------------
// AC-7: integration contract for #101 — 3 concepts → 3 :Concept nodes.
// ------------------------------------------------------------------

#[test]
fn ac7_three_toml_files_emit_three_concept_nodes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/trading.toml",
        "name = \"trading\"\ncrates = [\"domain-trading\"]\n",
    );
    write(
        tmp.path(),
        ".cfdb/concepts/risk.toml",
        "name = \"risk\"\ncrates = [\"domain-risk\"]\n",
    );
    write(
        tmp.path(),
        ".cfdb/concepts/ledger.toml",
        "name = \"ledger\"\ncrates = [\"domain-ledger\"]\n",
    );
    let mut store = store_with_items(
        tmp.path(),
        &[
            ("A", "domain-trading"),
            ("B", "domain-risk"),
            ("C", "domain-ledger"),
        ],
    );
    let ks = Keyspace::new("test");
    store.enrich_concepts(&ks).expect("pass");

    assert_eq!(
        count_nodes_by_label(&store, &ks, Label::CONCEPT),
        3,
        "3 TOML files → 3 :Concept nodes (AC-7)"
    );
}

// ------------------------------------------------------------------
// AC-1 reinforcement: assigned_by = "manual" on emitted :Concept nodes.
// ------------------------------------------------------------------

#[test]
fn concept_nodes_have_assigned_by_manual() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/trading.toml",
        "name = \"trading\"\ncrates = [\"domain-trading\"]\n",
    );
    let mut store = store_with_items(tmp.path(), &[("A", "domain-trading")]);
    let ks = Keyspace::new("test");
    store.enrich_concepts(&ks).expect("pass");

    let (nodes, _) = store.export(&ks).expect("export");
    let concept = nodes
        .iter()
        .find(|n| n.label.as_str() == Label::CONCEPT)
        .expect(":Concept node must exist");
    assert_eq!(
        concept.props.get("assigned_by").and_then(PropValue::as_str),
        Some("manual")
    );
    assert_eq!(
        concept.props.get("name").and_then(PropValue::as_str),
        Some("trading")
    );
}

// ------------------------------------------------------------------
// Degraded paths
// ------------------------------------------------------------------

#[test]
fn unknown_keyspace_returns_err() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let ks = Keyspace::new("never");
    let err = store
        .enrich_concepts(&ks)
        .expect_err("unknown keyspace must err");
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

#[test]
fn no_workspace_root_returns_degraded_report() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str("A".into()));
    props.insert("crate".into(), PropValue::Str("domain-x".into()));
    store
        .ingest_nodes(
            &ks,
            vec![Node {
                id: "item:A".into(),
                label: Label::new(Label::ITEM),
                props,
            }],
        )
        .expect("ingest");
    let report = store.enrich_concepts(&ks).expect("pass");
    assert!(!report.ran);
    assert!(report.warnings.iter().any(|w| w.contains("workspace_root")));
}

#[test]
fn items_without_crate_prop_are_ignored() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        ".cfdb/concepts/trading.toml",
        "name = \"trading\"\ncrates = [\"domain-trading\"]\n",
    );
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let ks = Keyspace::new("test");
    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str("NoCrate".into()));
    // Deliberately omit `crate` prop.
    store
        .ingest_nodes(
            &ks,
            vec![Node {
                id: "item:NoCrate".into(),
                label: Label::new(Label::ITEM),
                props,
            }],
        )
        .expect("ingest");
    let report = store.enrich_concepts(&ks).expect("pass");

    assert!(report.ran);
    // :Concept still emitted, but no edges to the item.
    assert_eq!(report.edges_written, 0);
}
