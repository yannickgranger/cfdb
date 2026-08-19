use std::collections::BTreeMap;

use cfdb_classify::{CheckReport, ClassifyEngine, TriggerId};
use cfdb_core::fact::{Node, PropValue};
use cfdb_core::result::{Row, RowValue, WarningKind};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

fn ctx(name: &str, canonical_crate: &str, owning_rfc: &str) -> Node {
    Node::new(format!("ctx:{name}"), Label::new(Label::CONTEXT))
        .with_prop("name", name)
        .with_prop("canonical_crate", canonical_crate)
        .with_prop("owning_rfc", owning_rfc)
}

fn krate(name: &str) -> Node {
    Node::new(format!("crate:{name}"), Label::new(Label::CRATE)).with_prop("name", name)
}

fn item(qname: &str, name: &str, kind: &str, crate_name: &str, bc: &str) -> Node {
    Node::new(qname, Label::new(Label::ITEM))
        .with_prop("qname", qname)
        .with_prop("name", name)
        .with_prop("kind", kind)
        .with_prop("crate", crate_name)
        .with_prop("bounded_context", bc)
        .with_prop("file", format!("{crate_name}/src/lib.rs"))
        .with_prop("is_test", false)
}

fn rfc_doc(path: &str, title: &str) -> Node {
    Node::new(format!("rfc:{path}"), Label::new(Label::RFC_DOC))
        .with_prop("path", path)
        .with_prop("title", title)
}

fn fixture(with_rfc_docs: bool) -> Vec<Node> {
    let mut nodes = vec![
        ctx("ctx-a", "crate-a", "RFC-001"),
        ctx("ctx-b", "ghost-crate", "RFC-001"),
        krate("crate-a"),
        krate("crate-b"),
        item("crate_a::Widget", "Widget", "struct", "crate-a", "ctx-a"),
        item("crate_b::Widget", "Widget", "struct", "crate-b", "ctx-b"),
        item("crate_a::Gadget", "Gadget", "struct", "crate-a", "ctx-a"),
    ];
    if with_rfc_docs {
        nodes.push(rfc_doc("docs/RFC-001-seed.md", "RFC-001 — seed"));
    }
    nodes
}

fn run(trigger: TriggerId, with_rfc_docs: bool) -> CheckReport {
    let ks = Keyspace::new("golden");
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks, fixture(with_rfc_docs))
        .expect("ingest");
    ClassifyEngine::new(&store)
        .check(&ks, trigger)
        .expect("check runs")
}

fn s(v: &str) -> RowValue {
    RowValue::Scalar(PropValue::Str(v.to_string()))
}

fn i(v: i64) -> RowValue {
    RowValue::Scalar(PropValue::Int(v))
}

fn list(vs: &[&str]) -> RowValue {
    RowValue::List(vs.iter().map(|v| PropValue::Str(v.to_string())).collect())
}

#[test]
fn t1_projects_one_missing_canonical_crate_row_in_the_five_column_shape() {
    let report = run(TriggerId::T1, true);
    assert_eq!(report.trigger, TriggerId::T1);
    assert!(
        report.warnings.is_empty(),
        "an `:RfcDoc` set present ⇒ no warning, got {:?}",
        report.warnings
    );

    let expected: Row = BTreeMap::from([
        ("verdict".to_string(), s("MISSING_CANONICAL_CRATE")),
        ("context_name".to_string(), s("ctx-b")),
        ("canonical_crate".to_string(), s("ghost-crate")),
        ("owning_rfc".to_string(), s("RFC-001")),
        ("evidence".to_string(), s("ghost-crate")),
    ]);
    assert_eq!(report.rows, vec![expected]);
    assert_eq!(report.row_count(), 1);
}

#[test]
fn t1_null_fills_absent_context_props_and_reports_every_sub_verdict() {
    let ks = Keyspace::new("golden-null");
    let mut store = PetgraphStore::new();
    let mut nodes = fixture(true);
    nodes.push(Node::new("ctx:bare", Label::new(Label::CONTEXT)).with_prop("name", "ctx-bare"));
    store.ingest_nodes(&ks, nodes).expect("ingest");
    let report = ClassifyEngine::new(&store)
        .check(&ks, TriggerId::T1)
        .expect("check runs");

    let verdicts: Vec<(&str, &str)> = report
        .rows
        .iter()
        .map(|r| {
            let col = |k: &str| match r.get(k) {
                Some(RowValue::Scalar(PropValue::Str(v))) => v.as_str(),
                other => panic!("{k}: expected string, got {other:?}"),
            };
            (col("context_name"), col("verdict"))
        })
        .collect();
    assert_eq!(
        verdicts,
        vec![
            ("ctx-b", "MISSING_CANONICAL_CRATE"),
            ("ctx-bare", "CONCEPT_UNWIRED")
        ]
    );
    let bare = &report.rows[1];
    assert_eq!(
        bare.get("canonical_crate"),
        Some(&RowValue::Scalar(PropValue::Null))
    );
    assert_eq!(
        bare.get("owning_rfc"),
        Some(&RowValue::Scalar(PropValue::Null))
    );
    assert_eq!(bare.get("evidence"), Some(&s("ctx-bare")));
}

#[test]
fn t1_without_rfc_docs_warns_empty_result_and_marks_every_rfc_tag_stale() {
    let report = run(TriggerId::T1, false);
    let verdicts: Vec<&str> = report
        .rows
        .iter()
        .map(|r| match r.get("verdict") {
            Some(RowValue::Scalar(PropValue::Str(v))) => v.as_str(),
            other => panic!("verdict: expected string, got {other:?}"),
        })
        .collect();
    assert_eq!(
        verdicts,
        vec![
            "STALE_RFC_REFERENCE",
            "MISSING_CANONICAL_CRATE",
            "STALE_RFC_REFERENCE"
        ]
    );
    assert_eq!(report.warnings.len(), 1);
    let w = &report.warnings[0];
    assert_eq!(w.kind, WarningKind::EmptyResult);
    assert!(
        w.message.starts_with("no :RfcDoc nodes in keyspace"),
        "warning message drifted: {}",
        w.message
    );
    assert_eq!(
        w.suggestion.as_deref(),
        Some("cfdb enrich-rfc-docs --db <db> --keyspace <ks> --workspace <path>")
    );
}

#[test]
fn t3_projects_one_cross_context_row_in_the_eleven_column_shape() {
    let report = run(TriggerId::T3, true);
    assert_eq!(report.trigger, TriggerId::T3);
    assert!(report.warnings.is_empty());

    let expected: Row = BTreeMap::from([
        ("name".to_string(), s("Widget")),
        ("kind".to_string(), s("struct")),
        ("n".to_string(), i(2)),
        ("n_crates".to_string(), i(2)),
        ("n_contexts".to_string(), i(2)),
        ("crates".to_string(), list(&["crate-a", "crate-b"])),
        ("bounded_contexts".to_string(), list(&["ctx-a", "ctx-b"])),
        (
            "qnames".to_string(),
            list(&["crate_a::Widget", "crate_b::Widget"]),
        ),
        (
            "files".to_string(),
            list(&["crate-a/src/lib.rs", "crate-b/src/lib.rs"]),
        ),
        (
            "is_cross_context".to_string(),
            RowValue::Scalar(PropValue::Bool(true)),
        ),
        ("canonical_candidate".to_string(), s("crate-a")),
    ]);
    assert_eq!(report.rows, vec![expected]);
    assert_eq!(report.row_count(), 1);
}

#[test]
fn t3_without_a_declared_canonical_crate_projects_null_candidate() {
    let ks = Keyspace::new("golden-no-canonical");
    let mut store = PetgraphStore::new();
    let nodes: Vec<Node> = fixture(true)
        .into_iter()
        .filter(|n| n.label.as_str() != Label::CONTEXT)
        .collect();
    store.ingest_nodes(&ks, nodes).expect("ingest");
    let report = ClassifyEngine::new(&store)
        .check(&ks, TriggerId::T3)
        .expect("check runs");
    assert_eq!(report.rows.len(), 1);
    assert_eq!(
        report.rows[0].get("canonical_candidate"),
        Some(&RowValue::Scalar(PropValue::Null))
    );
    assert_eq!(
        report.rows[0].get("is_cross_context"),
        Some(&RowValue::Scalar(PropValue::Bool(true)))
    );
}
