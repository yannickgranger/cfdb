//! #486 — G6/G1 exclusion, end-to-end through the production dump path.
//!
//! `specs/concepts/cfdb-core.md` (G6) declares `:Item.test_coverage`
//! excluded from the G1 canonical-dump sha256, but until #486 no code
//! enforced it — the attr was byte-stable only because `enrich_metrics`
//! never populates it without `--features llvm-cov`. This test drives the
//! exact `StoreBackend::canonical_dump` path that `cfdb dump` and
//! `ci/determinism-check.sh` invoke and proves the exclusion is real.
//!
//! **Self-dogfood substitution.** The issue prescribes `enrich_metrics
//! --features llvm-cov` then `cfdb dump`. `llvm-cov` is a heavy,
//! environment-gated toolchain (cargo-llvm-cov + a full coverage run),
//! so this test instead injects the exact prop shape that pass would
//! write — `test_coverage` on an `:Item` — and runs it through the real
//! store dump. Same code path, deterministic, no toolchain dependency.

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
    // Pre-`enrich_metrics --features llvm-cov`: no coverage attr.
    let plain = dump_of(vec![item("item:demo::f", "demo::f")]);

    // Post-llvm-cov shape: the SAME item now carries `test_coverage`.
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
