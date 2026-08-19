use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::{Node, PropValue, Props};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

fn synthetic_unrelated_entry_point_node() -> Node {
    let mut props = Props::new();
    props.insert("kind".into(), PropValue::Str("cli_command".into()));
    props.insert("name".into(), PropValue::Str("__test_seed".into()));
    props.insert(
        "handler_qname".into(),
        PropValue::Str("__synthetic::__test_seed".into()),
    );
    props.insert("file".into(), PropValue::Str("__test__.rs".into()));
    props.insert("params".into(), PropValue::Str("[]".into()));
    Node {
        id: "entrypoint:cli_command:__synthetic::__test_seed".into(),
        label: Label::new(Label::ENTRY_POINT),
        props,
    }
}

fn synthetic_unrelated_item_node() -> Node {
    let mut props = Props::new();
    props.insert(
        "qname".into(),
        PropValue::Str("__synthetic::__test_seed".into()),
    );
    props.insert("name".into(), PropValue::Str("__test_seed".into()));
    props.insert("kind".into(), PropValue::Str("fn".into()));
    props.insert("crate".into(), PropValue::Str("__synthetic".into()));
    props.insert("file".into(), PropValue::Str("__test__.rs".into()));
    props.insert("is_test".into(), PropValue::Bool(false));
    Node {
        id: "item:__synthetic::__test_seed".into(),
        label: Label::new(Label::ITEM),
        props,
    }
}

fn exposes_edge_from_seed() -> cfdb_core::fact::Edge {
    cfdb_core::fact::Edge {
        src: "entrypoint:cli_command:__synthetic::__test_seed".into(),
        dst: "item:__synthetic::__test_seed".into(),
        label: EdgeLabel::new(EdgeLabel::EXPOSES),
        props: Props::new(),
    }
}

fn find_item_props<'a>(nodes: &'a [Node], qname: &str) -> Option<&'a Props> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .find(|n| {
            n.props
                .get("qname")
                .and_then(PropValue::as_str)
                .is_some_and(|q| q == qname)
        })
        .map(|n| &n.props)
}

#[test]
fn issue_396_self_dogfood_default_edge_provenance_marked_reachable() {
    let workspace = cfdb_workspace_root();
    let (mut nodes, mut edges) =
        cfdb_extractor::extract_workspace(&workspace).expect("extract cfdb workspace");

    let cfdb_serde_default_callsites: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .filter(|n| {
            n.props.get("kind").and_then(PropValue::as_str) == Some("serde_default")
                && n.props
                    .get("callee_path")
                    .and_then(PropValue::as_str)
                    .is_some_and(|p| p == "default_edge_provenance")
        })
        .collect();
    assert!(
        !cfdb_serde_default_callsites.is_empty(),
        "syn extractor did not emit any :CallSite{{kind=serde_default}} \
         for #[serde(default = \"default_edge_provenance\")] in \
         cfdb-core/src/schema/descriptors.rs — the syn-side emission this \
         post-pass depends on has regressed"
    );

    nodes.push(synthetic_unrelated_entry_point_node());
    nodes.push(synthetic_unrelated_item_node());
    edges.push(exposes_edge_from_seed());

    let mut store = PetgraphStore::new().with_workspace(&workspace);
    let ks = Keyspace::new("selfdog-396");
    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let report = cfdb_enrich::EnrichEngine::new(&mut store)
        .enrich_reachability(&ks)
        .expect("enrich_reachability must run with our synthetic seed");
    assert!(
        report.ran,
        "enrich_reachability should run with the synthetic entry point: {:?}",
        report.warnings
    );

    let (all_nodes, _) = store.export(&ks).expect("export keyspace");
    let target_qname = "cfdb_core::schema::descriptors::default_edge_provenance";
    let props = find_item_props(&all_nodes, target_qname).unwrap_or_else(|| {
        panic!(
            "expected :Item with qname={target_qname} after cfdb extract — the syn \
             extractor must have emitted the fn node"
        )
    });

    let reachable = props
        .get("reachable_from_entry")
        .and_then(|p| match p {
            PropValue::Bool(b) => Some(*b),
            _ => None,
        })
        .expect("reachable_from_entry must be written for every :Item");

    assert!(
        reachable,
        "AC #396: {target_qname} MUST be reachable_from_entry=true after \
         enrich_reachability — the serde_default post-pass should have flipped \
         it. If this fails, either the post-pass did not run, or the \
         resolver failed to match callee_path=\"default_edge_provenance\" \
         (short form) against the same-module candidate \
         item:cfdb_core::schema::descriptors::default_edge_provenance"
    );
}
