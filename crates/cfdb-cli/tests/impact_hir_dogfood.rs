#![cfg(feature = "integration-live")]

use std::collections::BTreeSet;

use cfdb_core::schema::Keyspace;
use cfdb_core::store::QueryBackend;
use cfdb_eval::QueryEngine;
use cfdb_hir_extractor::emit::CallSiteEmitter;
use cfdb_hir_extractor::{build_hir_database, extract_call_sites};
use cfdb_hir_petgraph_adapter::PetgraphAdapter;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::impact_query;

mod common;

const SEED: &str = "cfdb_core::qname::node_id::item_node_id";

const KNOWN_DIRECT_CALLER: &str = "cfdb_enrich::attr_call_resolution::resolve_callee_to_item";

fn resolved_calls_keyspace() -> (PetgraphStore, Keyspace) {
    let root = common::workspace_root();
    let (db, vfs, _proc_macro_client, targets) =
        build_hir_database(&root, true).expect("build HIR database for cfdb-self");
    let (nodes, edges) =
        extract_call_sites(&db, &vfs, &root, &targets).expect("resolve cfdb-self call sites");

    let keyspace = Keyspace::new("impact-hir-dogfood");
    let mut store = PetgraphStore::new();
    let mut adapter = PetgraphAdapter::new(&mut store, keyspace.clone());
    adapter
        .ingest_resolved_call_sites(nodes, edges)
        .expect("ingest resolved call sites into the dogfood keyspace");
    (store, keyspace)
}

fn blast_radius(store: &PetgraphStore, ks: &Keyspace, seed: &str) -> BTreeSet<String> {
    let query = impact_query(&[seed], None);
    QueryEngine::new(store)
        .execute(ks, &query)
        .expect("execute impact query against the HIR keyspace")
        .rows
        .iter()
        .filter_map(|row| {
            row.get("qname")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect()
}

#[test]
fn impact_over_hir_calls_finds_cross_crate_blast_radius() {
    let (store, ks) = resolved_calls_keyspace();
    let blast = blast_radius(&store, &ks, SEED);

    assert!(
        !blast.is_empty(),
        "seed `{SEED}` must have resolved callers in the HIR CALLS graph"
    );

    assert!(
        blast.contains(KNOWN_DIRECT_CALLER),
        "known direct cfdb-enrich caller `{KNOWN_DIRECT_CALLER}` must be in the blast radius; got {blast:?}"
    );

    assert!(
        blast.iter().any(|q| q.starts_with("cfdb_cli::")),
        "the unbounded reverse traversal must reach cfdb-cli callers transitively; got {blast:?}"
    );
}
