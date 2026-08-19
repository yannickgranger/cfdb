use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;

fn cfdb_workspace_root() -> &'static Path {
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

    let call_site_count = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .count();
    assert!(
        call_site_count > 0,
        "expected CallSite nodes; got 0 — is visit_item_fn walking bodies?"
    );

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

    let invokes_at_count = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::INVOKES_AT)
        .count();
    assert_eq!(
        invokes_at_count, call_site_count,
        "every CallSite should have exactly one INVOKES_AT edge"
    );

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

    let has_test_callsites = nodes.iter().any(|n| {
        n.label.as_str() == Label::CALL_SITE
            && n.props.get("is_test").and_then(PropValue::as_bool) == Some(true)
    });
    assert!(
        has_test_callsites,
        "expected at least one CallSite tagged is_test=true from cfdb's #[cfg(test)] bodies"
    );
}

#[test]
fn self_workspace_emits_cfdb_context_and_belongs_to_edges() {
    let root = cfdb_workspace_root();
    let (nodes, edges) = extract_workspace(root).expect("extract cfdb");

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

    let json_a = serde_json::to_string(&(&nodes_a, &edges_a)).expect("serialize run 1");
    let json_b = serde_json::to_string(&(&nodes_b, &edges_b)).expect("serialize run 2");
    assert_eq!(
        json_a, json_b,
        "extractor is non-deterministic: two runs produced different outputs"
    );
}

#[test]
fn test_f005_item_line_is_real_not_zero() {
    let root = cfdb_workspace_root();
    let (nodes, _edges) = extract_workspace(root).expect("extract cfdb sub-workspace");

    let item_count = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .count();
    assert!(
        item_count > 0,
        "self-dogfood produced zero :Item nodes — extraction broken upstream of this scar"
    );

    let items_with_real_line = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .filter(|n| {
            n.props
                .get("line")
                .and_then(PropValue::as_i64)
                .map(|line| line > 0)
                .unwrap_or(false)
        })
        .count();

    let percentage = (items_with_real_line * 100) / item_count;
    assert!(
        percentage >= 50,
        "F-005 regression: only {items_with_real_line} of {item_count} :Item nodes \
         ({percentage}%) carry a real line>0 — expected >= 50%. \
         If proc-macro2's `span-locations` feature was disabled or \
         `span_line` reverted to returning 0, every :Item.line collapses \
         to 0 and line-precision queries silently return zero rows."
    );
}

#[test]
fn self_workspace_emits_render_type_inner_deltas() {
    let root = cfdb_workspace_root();
    let (_nodes, edges) = extract_workspace(root).expect("extract cfdb sub-workspace");
    let returns_count = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::RETURNS)
        .count();
    let type_of_count = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::TYPE_OF)
        .count();
    assert!(
        returns_count >= 250,
        "expected >= 250 RETURNS edges after #239 render_type_inner ships (got {returns_count}); baseline at 346eab1 was 131 — a regression below 250 means the third-tier unwrap is not firing"
    );
    assert!(
        type_of_count >= 220,
        "expected >= 220 TYPE_OF edges after #239 render_type_inner ships (got {type_of_count}); baseline at 346eab1 was 182 — a regression below 220 means the Field/Param unwrap wiring broke"
    );
}

#[test]
fn crate_tier_self_dogfood_core_is_zero_cli_is_max() {
    let root = cfdb_workspace_root();
    let (nodes, _edges) = extract_workspace(root).expect("extract cfdb sub-workspace");

    let tier_of = |crate_name: &str| -> i64 {
        nodes
            .iter()
            .find(|n| n.id == format!("crate:{crate_name}"))
            .and_then(|n| n.props.get("crate_tier"))
            .and_then(|v| match v {
                PropValue::Int(i) => Some(*i),
                _ => None,
            })
            .unwrap_or_else(|| panic!("crate:{crate_name} must carry an int crate_tier"))
    };

    let core = tier_of("cfdb-core");
    let cli = tier_of("cfdb-cli");

    assert_eq!(
        core, 0,
        "cfdb-core has zero in-workspace normal deps → tier 0 (got {core})"
    );

    let max_tier = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CRATE)
        .filter_map(|n| match n.props.get("crate_tier") {
            Some(PropValue::Int(i)) => Some(*i),
            _ => None,
        })
        .max()
        .expect("at least one :Crate carries a crate_tier");

    assert!(
        cli > core,
        "cfdb-cli (tier {cli}) must be deeper than cfdb-core (tier {core})"
    );
    assert_eq!(
        cli, max_tier,
        "cfdb-cli is the maximum-tier crate (got {cli}, workspace max {max_tier})"
    );

    for node in nodes.iter().filter(|n| n.label.as_str() == Label::CRATE) {
        assert!(
            matches!(node.props.get("crate_tier"), Some(PropValue::Int(_))),
            "{} is missing an int crate_tier",
            node.id
        );
    }
}

#[test]
fn self_workspace_emits_private_items() {
    let root = cfdb_workspace_root();
    let (nodes, _edges) = extract_workspace(root).expect("extract cfdb sub-workspace");

    let private_items = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("visibility").and_then(PropValue::as_str) == Some("private")
        })
        .count();
    assert!(
        private_items > 0,
        "expected at least one :Item{{visibility:\"private\"}} node from cfdb's own \
         non-pub helper fns/structs — got 0, which would mean either the extractor \
         started filtering private items or visibility tagging broke"
    );

    let pub_items = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("visibility").and_then(PropValue::as_str) == Some("pub")
        })
        .count();
    assert!(
        pub_items > 0,
        "expected at least one :Item{{visibility:\"pub\"}} node from cfdb's own public API"
    );
}
