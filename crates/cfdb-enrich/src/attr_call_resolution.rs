use std::collections::BTreeMap;

use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphView;
use cfdb_core::qname::{item_node_id, item_node_id_for_target, TargetDiscriminator};
use cfdb_core::schema::{Direction, EdgeLabel, Label};

const KIND_SERDE_DEFAULT: &str = "serde_default";

pub(crate) fn mark_serde_default_callees_reachable(
    view: &mut dyn GraphView,
    reach_attr: &str,
) -> u64 {
    let resolutions = collect_resolutions(view);
    apply_resolutions(view, &resolutions, reach_attr)
}

fn collect_resolutions(view: &dyn GraphView) -> BTreeMap<String, String> {
    let callsites = view.nodes_with_label(&Label::new(Label::CALL_SITE));
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for cs_id in callsites {
        let Some(cs_node) = view.node_by_id(&cs_id) else {
            continue;
        };
        if cs_node.props.get("kind").and_then(PropValue::as_str) != Some(KIND_SERDE_DEFAULT) {
            continue;
        }
        let Some(callee_path) = cs_node.props.get("callee_path").and_then(PropValue::as_str) else {
            continue;
        };
        let Some(caller_qname) = cs_node
            .props
            .get("caller_qname")
            .and_then(PropValue::as_str)
        else {
            continue;
        };
        let caller_target = caller_target_via_invokes_at(view, &cs_id);
        if let Some(item_id) =
            resolve_callee_to_item(view, callee_path, caller_qname, caller_target.as_ref())
        {
            out.insert(cs_id, item_id);
        }
    }
    out
}

fn caller_target_via_invokes_at(view: &dyn GraphView, cs_id: &str) -> Option<TargetDiscriminator> {
    view.neighbors(cs_id, Direction::In)
        .into_iter()
        .find(|(label, _)| label.as_str() == EdgeLabel::INVOKES_AT)
        .and_then(|(_, owner_id)| view.node_by_id(&owner_id))
        .and_then(|owner| owner.props.get("target").and_then(PropValue::as_str))
        .and_then(TargetDiscriminator::from_wire_str)
}

fn apply_resolutions(
    view: &mut dyn GraphView,
    resolutions: &BTreeMap<String, String>,
    reach_attr: &str,
) -> u64 {
    let mut count: u64 = 0;
    for item_id in resolutions.values() {
        if view.set_attr(item_id, reach_attr, PropValue::Bool(true)) {
            count += 1;
        }
    }
    count
}

fn resolve_callee_to_item(
    view: &dyn GraphView,
    callee_path: &str,
    caller_qname: &str,
    caller_target: Option<&TargetDiscriminator>,
) -> Option<String> {
    let lookup = |candidate: &str| -> Option<String> {
        if let Some(target) = caller_target {
            let discriminated = item_node_id_for_target(candidate, target);
            if view.node_by_id(&discriminated).is_some() {
                return Some(discriminated);
            }
        }
        let bare = item_node_id(candidate);
        view.node_by_id(&bare).is_some().then_some(bare)
    };
    if let Some(id) = lookup(callee_path) {
        return Some(id);
    }
    if let Some((module_path, _last)) = caller_qname.rsplit_once("::") {
        if let Some(id) = lookup(&format!("{module_path}::{callee_path}")) {
            return Some(id);
        }
    }
    if let Some((crate_name, _rest)) = caller_qname.split_once("::") {
        if let Some(id) = lookup(&format!("{crate_name}::{callee_path}")) {
            return Some(id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::{Node, Props};
    use cfdb_core::graph::GraphBackend;
    use cfdb_core::schema::Keyspace;
    use cfdb_core::store::StoreBackend;
    use cfdb_petgraph::PetgraphStore;

    fn make_item(qname: &str) -> Node {
        let mut props = Props::new();
        props.insert("qname".into(), PropValue::Str(qname.into()));
        Node {
            id: format!("item:{qname}"),
            label: Label::new(Label::ITEM),
            props,
        }
    }

    fn store_with(nodes: Vec<Node>) -> (PetgraphStore, Keyspace) {
        let ks = Keyspace::new("test");
        let mut store = PetgraphStore::new();
        store.ingest_nodes(&ks, nodes).expect("ingest");
        (store, ks)
    }

    #[test]
    fn resolver_prefers_exact_over_same_module() {
        let (mut store, ks) = store_with(vec![
            make_item("myapp::config::default_url"),
            make_item("myapp::other::config::default_url"),
        ]);
        let view = store.graph_view(&ks).expect("keyspace");
        let resolved = resolve_callee_to_item(
            view,
            "myapp::config::default_url",
            "myapp::other::config::AppConfig",
            None,
        );
        let id = resolved.expect("exact match must win");
        let node = view.node_by_id(&id).expect("node");
        assert_eq!(
            node.props.get("qname").and_then(PropValue::as_str),
            Some("myapp::config::default_url")
        );
    }

    #[test]
    fn resolver_falls_back_to_same_module() {
        let (mut store, ks) = store_with(vec![make_item("myapp::config::default_url")]);
        let view = store.graph_view(&ks).expect("keyspace");
        let resolved =
            resolve_callee_to_item(view, "default_url", "myapp::config::AppConfig", None);
        assert!(resolved.is_some(), "same-module resolution must succeed");
    }

    #[test]
    fn resolver_falls_back_to_same_crate() {
        let (mut store, ks) = store_with(vec![make_item("myapp::config::default_url")]);
        let view = store.graph_view(&ks).expect("keyspace");
        let resolved =
            resolve_callee_to_item(view, "config::default_url", "myapp::main::AppConfig", None);
        assert!(resolved.is_some(), "same-crate resolution must succeed");
    }

    #[test]
    fn resolver_returns_none_for_unknown_path() {
        let (mut store, ks) = store_with(vec![]);
        let view = store.graph_view(&ks).expect("keyspace");
        assert!(resolve_callee_to_item(view, "nowhere::fn", "myapp::AppConfig", None).is_none());
    }

    #[test]
    fn bin_target_caller_resolves_bin_local_callee_same_target_first() {
        let mut callee = make_item("tif::defaults::seed");
        callee.id = format!("{}#bin:alpha", callee.id);
        let (mut store, ks) = store_with(vec![callee]);
        let view = store.graph_view(&ks).expect("keyspace");
        let alpha = TargetDiscriminator::Bin {
            name: "alpha".to_string(),
        };
        let resolved =
            resolve_callee_to_item(view, "tif::defaults::seed", "tif::main", Some(&alpha));
        assert!(
            resolved.is_some(),
            "bin-target callee must resolve within the caller's namespace"
        );
        let beta = TargetDiscriminator::Bin {
            name: "beta".to_string(),
        };
        let foreign = resolve_callee_to_item(view, "tif::defaults::seed", "tif::main", Some(&beta));
        assert!(
            foreign.is_none(),
            "a foreign bin's namespace must not capture the candidate"
        );
    }
}
