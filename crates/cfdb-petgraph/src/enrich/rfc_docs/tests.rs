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
    std::fs::write(&path, contents).expect("write file");
}

fn store_with_item(workspace: &Path, item_name: &str, qname: &str) -> PetgraphStore {
    let mut store = PetgraphStore::new().with_workspace(workspace);
    let ks = Keyspace::new("test");
    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str(qname.to_string()));
    props.insert("name".into(), PropValue::Str(item_name.to_string()));
    props.insert("file".into(), PropValue::Str("src/lib.rs".to_string()));
    let node = Node {
        id: format!("item:{qname}"),
        label: Label::new(Label::ITEM),
        props,
    };
    store.ingest_nodes(&ks, vec![node]).expect("ingest");
    store
}

fn count_nodes(store: &PetgraphStore, ks: &Keyspace, label: &str) -> usize {
    let (nodes, _) = store.export(ks).expect("export");
    nodes.iter().filter(|n| n.label.as_str() == label).count()
}

fn count_edges(store: &PetgraphStore, ks: &Keyspace, label: &str) -> usize {
    let (_, edges) = store.export(ks).expect("export");
    edges.iter().filter(|e| e.label.as_str() == label).count()
}

// ------------------------------------------------------------------
// AC-1: synthetic RFC with known item name → exactly 1 :RfcDoc + 1
// REFERENCED_BY edge.
// ------------------------------------------------------------------

#[test]
fn ac1_match_emits_one_rfc_doc_and_one_edge() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "docs/RFC-example.md",
        "# Example RFC\n\nRefers to FooBarService here.\n",
    );
    let mut store = store_with_item(tmp.path(), "FooBarService", "crate::FooBarService");
    let ks = Keyspace::new("test");
    let report = store.enrich_rfc_docs(&ks).expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 1, "one RFC file scanned");
    assert_eq!(report.edges_written, 1, "one REFERENCED_BY edge");
    assert_eq!(count_nodes(&store, &ks, Label::RFC_DOC), 1);
    assert_eq!(count_edges(&store, &ks, EdgeLabel::REFERENCED_BY), 1);
}

// ------------------------------------------------------------------
// AC-2: no RFC files → ran=true, all counters zero, no panic.
// ------------------------------------------------------------------

#[test]
fn ac2_no_rfc_files_returns_zeroed_report() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = store_with_item(tmp.path(), "FooBarService", "crate::FooBarService");
    let ks = Keyspace::new("test");
    let report = store.enrich_rfc_docs(&ks).expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 0);
    assert_eq!(report.attrs_written, 0);
    assert_eq!(report.edges_written, 0);
}

// ------------------------------------------------------------------
// AC-6: no panic on malformed markdown.
// ------------------------------------------------------------------

#[test]
fn ac6_empty_file_and_no_heading_do_not_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(tmp.path(), "docs/empty.md", "");
    write(
        tmp.path(),
        "docs/no-heading.md",
        "just text, no heading\nand another line\n",
    );
    write(
        tmp.path(),
        "docs/has-heading.md",
        "# Real Heading\n\nMentions FooBarService.\n",
    );
    let mut store = store_with_item(tmp.path(), "FooBarService", "crate::FooBarService");
    let ks = Keyspace::new("test");
    let report = store.enrich_rfc_docs(&ks).expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 3);
    // Only has-heading.md and no-heading.md (if the name matches) would
    // match — but FooBarService is only in has-heading.md.
    assert_eq!(report.edges_written, 1, "only has-heading.md has the match");
}

#[test]
fn whole_word_matching_rejects_substring_matches() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Fixture contains Timer as a substring (`Timers`, `preTimer`,
    // `TimerService`, `TimerXyz`) but never as a standalone word — no
    // match should register.
    write(
        tmp.path(),
        "docs/RFC-example.md",
        "# Example\n\nMentions Timers and preTimer and TimerService and TimerXyz.\n",
    );
    let mut store = store_with_item(tmp.path(), "Timer", "crate::Timer");
    let ks = Keyspace::new("test");
    let report = store.enrich_rfc_docs(&ks).expect("pass");

    assert_eq!(report.edges_written, 0, "substring matches rejected");
}

#[test]
fn whole_word_matching_accepts_punctuation_neighbours() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "docs/RFC-example.md",
        "# Example\n\nUse `Timer`, then Timer. Also Timer's behavior.\n",
    );
    let mut store = store_with_item(tmp.path(), "Timer", "crate::Timer");
    let ks = Keyspace::new("test");
    let report = store.enrich_rfc_docs(&ks).expect("pass");

    assert_eq!(
        report.edges_written, 1,
        "backticks/commas/apostrophes are word boundaries"
    );
}

#[test]
fn qname_match_triggers_reference_when_name_absent() {
    // File mentions the qname but not the bare name — qname match alone
    // should be sufficient.
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "docs/RFC-example.md",
        "# Example\n\nThe cfdb_core::enrich::EnrichBackend trait.\n",
    );
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let ks = Keyspace::new("test");
    let mut props = Props::new();
    props.insert(
        "qname".into(),
        PropValue::Str("cfdb_core::enrich::EnrichBackend".into()),
    );
    props.insert("name".into(), PropValue::Str("NotMentionedHere".into()));
    props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
    let node = Node {
        id: "item:cfdb_core::enrich::EnrichBackend".into(),
        label: Label::new(Label::ITEM),
        props,
    };
    store.ingest_nodes(&ks, vec![node]).expect("ingest");
    let report = store.enrich_rfc_docs(&ks).expect("pass");

    assert_eq!(report.edges_written, 1);
}

// ------------------------------------------------------------------
// AC-5: determinism across two runs.
// ------------------------------------------------------------------

#[test]
fn ac5_two_runs_produce_identical_canonical_dumps() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "docs/RFC-a.md",
        "# RFC A\n\nFooBarService and BazQuuxService.\n",
    );
    write(
        tmp.path(),
        "docs/RFC-b.md",
        "# RFC B\n\nOnly BazQuuxService.\n",
    );

    fn build(root: &Path) -> PetgraphStore {
        let mut store = PetgraphStore::new().with_workspace(root);
        let ks = Keyspace::new("test");
        for (n, q) in [
            ("FooBarService", "crate::FooBarService"),
            ("BazQuuxService", "crate::BazQuuxService"),
        ] {
            let mut props = Props::new();
            props.insert("qname".into(), PropValue::Str(q.to_string()));
            props.insert("name".into(), PropValue::Str(n.to_string()));
            props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
            let node = Node {
                id: format!("item:{q}"),
                label: Label::new(Label::ITEM),
                props,
            };
            store.ingest_nodes(&ks, vec![node]).expect("ingest");
        }
        store
    }

    let ks = Keyspace::new("test");
    let mut s1 = build(tmp.path());
    s1.enrich_rfc_docs(&ks).expect("run 1");
    let mut s2 = build(tmp.path());
    s2.enrich_rfc_docs(&ks).expect("run 2");
    let d1 = s1.canonical_dump(&ks).expect("dump 1");
    let d2 = s2.canonical_dump(&ks).expect("dump 2");
    assert_eq!(d1, d2, "two runs must produce byte-identical dumps (AC-5)");
}

#[test]
fn unknown_keyspace_returns_err() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let ks = Keyspace::new("never");
    let err = store
        .enrich_rfc_docs(&ks)
        .expect_err("unknown keyspace must err");
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

#[test]
fn no_workspace_root_returns_degraded_report() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str("crate::x".into()));
    props.insert("name".into(), PropValue::Str("x".into()));
    props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
    let node = Node {
        id: "item:crate::x".into(),
        label: Label::new(Label::ITEM),
        props,
    };
    store.ingest_nodes(&ks, vec![node]).expect("ingest");
    let report = store.enrich_rfc_docs(&ks).expect("pass");
    assert!(!report.ran, "no workspace_root → ran=false");
    assert!(
        report.warnings.iter().any(|w| w.contains("workspace_root")),
        "warning must name the missing root"
    );
}

#[test]
fn rfc_file_with_no_matches_is_not_emitted_as_node() {
    // RFC file exists but doesn't reference any known item — no node,
    // no edge, no wasted data.
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "docs/unrelated.md",
        "# Meta\n\nNothing to see.\n",
    );
    let mut store = store_with_item(tmp.path(), "FooBarService", "crate::FooBarService");
    let ks = Keyspace::new("test");
    let report = store.enrich_rfc_docs(&ks).expect("pass");

    assert_eq!(report.facts_scanned, 1);
    assert_eq!(
        report.attrs_written, 0,
        "no :RfcDoc emitted for orphan file"
    );
    assert_eq!(report.edges_written, 0);
    assert_eq!(count_nodes(&store, &ks, Label::RFC_DOC), 0);
}
