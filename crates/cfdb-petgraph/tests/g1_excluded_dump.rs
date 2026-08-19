use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

fn item(id: &str, qname: &str) -> Node {
    Node::new(id, Label::new(Label::ITEM))
        .with_prop("qname", qname)
        .with_prop("kind", "fn")
}

fn dump_of(nodes: Vec<Node>) -> String {
    let ks = Keyspace::new("g1_excluded_486");
    let mut store = PetgraphStore::new();
    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.canonical_dump(&ks).expect("canonical_dump")
}

#[test]
fn cfdb_dump_excludes_test_coverage_and_stays_byte_stable() {
    let plain = dump_of(vec![item("item:demo::f", "demo::f")]);

    let covered = dump_of(vec![item("item:demo::f", "demo::f").with_prop(
        "test_coverage",
        PropValue::Str("{\"lines\":42,\"covered\":7}".into()),
    )]);

    assert!(
        !covered.contains("test_coverage"),
        "cfdb dump must exclude the G1 attr `test_coverage`: {covered}"
    );
    assert_eq!(
        plain, covered,
        "populating `test_coverage` must not change the canonical (G1) dump — \
         this is precisely what keeps ci/determinism-check.sh green after \
         `enrich_metrics --features llvm-cov`"
    );
}
