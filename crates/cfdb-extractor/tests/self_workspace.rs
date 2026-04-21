//! Integration tests that extract cfdb's own sub-workspace and assert
//! invariants on the emitted node/edge set. Moved out of `lib.rs` as
//! part of the #3718 god-module split so `lib.rs` can drop below 200 LOC.
//!
//! These tests use only the public API (`extract_workspace` + the re-exported
//! `cfdb_core` facts), so they compile cleanly against the crate boundary.

use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;

/// Resolve the cfdb sub-workspace root — this crate's grandparent directory.
fn cfdb_workspace_root() -> &'static Path {
    // The cfdb sub-workspace itself — this crate's parent Cargo.toml.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-extractor crate dir has parent (crates/)")
        .parent()
        .expect("crates/ has parent (cfdb sub-workspace root)")
}

#[test]
fn extracts_self_workspace() {
    let root = cfdb_workspace_root();
    let (nodes, edges) = extract_workspace(root).expect("extract cfdb sub-workspace");

    assert!(
        nodes.iter().any(|n| n.id == "crate:cfdb-core"),
        "expected cfdb-core crate node, got: {:?}",
        nodes.iter().map(|n| &n.id).take(5).collect::<Vec<_>>()
    );

    // cfdb-core defines a StoreBackend trait; we should see it as an Item.
    assert!(
        nodes.iter().any(|n| {
            n.label.as_str() == Label::ITEM
                && n.props
                    .get("name")
                    .and_then(PropValue::as_str)
                    .map(|s| s == "StoreBackend")
                    .unwrap_or(false)
        }),
        "expected StoreBackend trait item"
    );

    // Every Item should have an IN_CRATE edge to its crate node.
    let item_count = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .count();
    let in_crate_count = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IN_CRATE)
        .count();
    assert!(
        in_crate_count >= item_count,
        "IN_CRATE edges ({in_crate_count}) should cover all Items ({item_count})"
    );
}

#[test]
fn emits_call_sites_and_methods() {
    let root = cfdb_workspace_root();
    let (nodes, edges) = extract_workspace(root).expect("extract cfdb");

    // At least one CallSite node exists (cfdb's own source calls
    // `BTreeMap::new`, `format!`, `Vec::new`, etc. everywhere).
    let call_site_count = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .count();
    assert!(
        call_site_count > 0,
        "expected CallSite nodes; got 0 — is visit_item_fn walking bodies?"
    );

    // At least one method Item exists (cfdb has plenty of impl methods).
    let method_count = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && n.props
                    .get("kind")
                    .and_then(PropValue::as_str)
                    .map(|s| s == "method")
                    .unwrap_or(false)
        })
        .count();
    assert!(
        method_count > 0,
        "expected method Items; got 0 — is visit_impl_item_fn wired?"
    );

    // Every CallSite has an incoming INVOKES_AT edge.
    let invokes_at_count = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::INVOKES_AT)
        .count();
    assert_eq!(
        invokes_at_count, call_site_count,
        "every CallSite should have exactly one INVOKES_AT edge"
    );

    // SchemaVersion v0.1.3+ self-dogfood (issue #83, RFC-029 §A1.2):
    // every :CallSite emitted while extracting cfdb's own source tree
    // carries `resolver="syn"` + `callee_resolved=false`. No exceptions.
    // The syn extractor must never claim HIR resolution.
    for cs in nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
    {
        assert_eq!(
            cs.props.get("resolver").and_then(PropValue::as_str),
            Some("syn"),
            "{}: self-dogfood must see `resolver=\"syn\"` on every :CallSite",
            cs.id,
        );
        assert_eq!(
            cs.props.get("callee_resolved"),
            Some(&PropValue::Bool(false)),
            "{}: self-dogfood must see `callee_resolved=false` on every :CallSite",
            cs.id,
        );
    }
}

#[test]
fn tags_cfg_test_modules_with_is_test() {
    let root = cfdb_workspace_root();
    let (nodes, _edges) = extract_workspace(root).expect("extract cfdb");

    // cfdb's own source has `#[cfg(test)] mod tests {}` in several
    // crates. At least one Item should be tagged as test.
    let test_items = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && n.props
                    .get("is_test")
                    .and_then(PropValue::as_bool)
                    .unwrap_or(false)
        })
        .count();
    assert!(
        test_items > 0,
        "expected at least one is_test=true Item from cfdb's own #[cfg(test)] blocks"
    );

    // And at least one Item should NOT be tagged — cfdb's prod code
    // (every lib.rs, every visit_* method) is prod.
    let prod_items = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("is_test").and_then(PropValue::as_bool) == Some(false)
        })
        .count();
    assert!(
        prod_items > test_items,
        "expected more prod Items than test Items in cfdb, got prod={prod_items} test={test_items}"
    );

    // CallSite nodes should carry the same flag.
    let has_test_callsites = nodes.iter().any(|n| {
        n.label.as_str() == Label::CALL_SITE
            && n.props.get("is_test").and_then(PropValue::as_bool) == Some(true)
    });
    assert!(
        has_test_callsites,
        "expected at least one CallSite tagged is_test=true from cfdb's #[cfg(test)] bodies"
    );
}

/// AC: `context_node_emitted_for_each_declared_context` +
/// `belongs_to_edge_connects_crate_to_context` (issue #3727,
/// council-cfdb-wiring §B.1.3). Running `extract_workspace` against the
/// cfdb sub-workspace (with the committed `.cfdb/concepts/cfdb.toml`
/// override fixture) must emit exactly one `:Context` node named `"cfdb"`
/// and one `BELONGS_TO` edge per cfdb crate pointing to it.
#[test]
fn self_workspace_emits_cfdb_context_and_belongs_to_edges() {
    let root = cfdb_workspace_root();
    let (nodes, edges) = extract_workspace(root).expect("extract cfdb");

    // The committed override fixture declares ALL 6 cfdb crates under
    // a single "cfdb" bounded context. There must be exactly one
    // :Context{name="cfdb"} node, and it must carry the override metadata
    // (canonical_crate, owning_rfc).
    let cfdb_context_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::CONTEXT
                && n.props.get("name").and_then(PropValue::as_str) == Some("cfdb")
        })
        .collect();
    assert_eq!(
        cfdb_context_nodes.len(),
        1,
        "expected exactly one :Context{{name=cfdb}} node, got {}",
        cfdb_context_nodes.len()
    );
    let cfdb_ctx = cfdb_context_nodes[0];
    assert_eq!(
        cfdb_ctx
            .props
            .get("canonical_crate")
            .and_then(PropValue::as_str),
        Some("cfdb-core"),
        ":Context{{cfdb}} should carry canonical_crate=cfdb-core from override"
    );
    assert_eq!(
        cfdb_ctx.props.get("owning_rfc").and_then(PropValue::as_str),
        Some("RFC-029"),
        ":Context{{cfdb}} should carry owning_rfc=RFC-029 from override"
    );

    // Every workspace-member Crate node must have exactly one BELONGS_TO
    // edge, and (since the override maps them all to cfdb) every edge
    // targets `context:cfdb`.
    let crate_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CRATE)
        .collect();
    assert!(
        crate_nodes.len() >= 6,
        "expected at least 6 cfdb workspace crates, got {}",
        crate_nodes.len()
    );

    for crate_node in &crate_nodes {
        let belongs: Vec<_> = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::BELONGS_TO && e.src == crate_node.id)
            .collect();
        assert_eq!(
            belongs.len(),
            1,
            "{} must have exactly one BELONGS_TO edge, got {}",
            crate_node.id,
            belongs.len()
        );
        assert_eq!(
            belongs[0].dst, "context:cfdb",
            "{} BELONGS_TO must target context:cfdb per override",
            crate_node.id
        );
    }

    // And every Item emitted from a cfdb crate must carry
    // `bounded_context=cfdb` — the per-Item stamping side of §B.1.2.
    let sample_item = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("crate").and_then(PropValue::as_str) == Some("cfdb-core")
        })
        .expect("at least one cfdb-core Item");
    assert_eq!(
        sample_item
            .props
            .get("bounded_context")
            .and_then(PropValue::as_str),
        Some("cfdb"),
        "cfdb-core Items must carry bounded_context=cfdb from the override"
    );
}

/// G1 determinism (RFC §12.1): two consecutive extractions against the
/// same workspace SHA must produce byte-identical (nodes, edges) output.
/// Any slip into `HashMap`, `sort_unstable`, or system-clock reads in the
/// extractor breaks this silently — the gate exists to catch it.
#[test]
fn extractor_is_deterministic_across_two_runs() {
    let root = cfdb_workspace_root();

    let (nodes_a, edges_a) = extract_workspace(root).expect("run 1");
    let (nodes_b, edges_b) = extract_workspace(root).expect("run 2");

    assert_eq!(
        nodes_a.len(),
        nodes_b.len(),
        "node count drifted between runs"
    );
    assert_eq!(
        edges_a.len(),
        edges_b.len(),
        "edge count drifted between runs"
    );

    // Compare by canonical JSON serialization — catches any property
    // reordering that Vec-equality would miss (PropValue is BTreeMap-keyed
    // so this should be stable, but the test asserts the guarantee).
    let json_a = serde_json::to_string(&(&nodes_a, &edges_a)).expect("serialize run 1");
    let json_b = serde_json::to_string(&(&nodes_b, &edges_b)).expect("serialize run 2");
    assert_eq!(
        json_a, json_b,
        "extractor is non-deterministic: two runs produced different outputs"
    );
}
