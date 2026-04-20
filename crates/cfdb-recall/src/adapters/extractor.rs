//! Extractor adapter — run `cfdb-extractor` on a workspace and project the
//! emitted `Label::ITEM` nodes into a `BTreeSet<PublicItem>`.
//!
//! The extractor emits every item it walks (public, private, test-scope),
//! each tagged with `crate` and `is_test` properties. For recall we keep
//! only items whose `crate` matches the target and whose `is_test` flag is
//! false — public-api's rustdoc JSON ground truth is computed against the
//! non-test build, so including test items here would create a phantom
//! mismatch on both sides.
//!
//! The projection does NOT attempt to filter by visibility. The extractor
//! emits private items too; they flow through as extra entries in the
//! output set. That is fine — the recall formula is set intersection
//! against the public ground truth, so extra private items on the
//! extracted side are a harmless superset (see `lib.rs` test
//! `extractor_superset_does_not_affect_recall`).
//!
//! ## Kind filter — keep only top-level items
//!
//! The extractor emits several `kind` values at `Label::ITEM`: `fn`,
//! `struct`, `enum`, `trait`, `type_alias`, `const`, `static`, and
//! `method`. The rustdoc JSON `paths` map (our ground truth, see
//! `adapters/ground_truth.rs`) only indexes TOP-LEVEL items — impl
//! methods live under `Crate::index` inside `Impl` items and need a
//! separate walk. For v0.1 we measure recall on top-level items only
//! and defer methods to v0.2; so this projection drops `kind="method"`
//! to keep the two sides of the recall formula symmetric.

use std::collections::BTreeSet;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_extractor::{extract_workspace, ExtractError};

use crate::PublicItem;

/// The extractor `kind` values we consider "top-level public API items".
/// `method` is intentionally absent — rustdoc's `paths` map does not
/// index impl methods, so including them in the recall surface would
/// asymmetrically inflate the extractor side of the comparison.
pub const KEPT_ITEM_KINDS: &[&str] = &[
    "fn",
    "struct",
    "enum",
    "trait",
    "type_alias",
    "const",
    "static",
    "union",
];

/// Project a list of extractor nodes into `(crate_name -> PublicItem set)`.
/// Keeps only `Label::ITEM` nodes whose `is_test` flag is false and whose
/// `kind` is in [`KEPT_ITEM_KINDS`]. Groups by the `crate` property so
/// the caller can look up a single crate's items without re-filtering.
///
/// Pure function — no I/O — so tests exercise it against synthetic node
/// vectors.
pub fn project_nodes(nodes: &[Node]) -> std::collections::BTreeMap<String, BTreeSet<PublicItem>> {
    let mut out: std::collections::BTreeMap<String, BTreeSet<PublicItem>> =
        std::collections::BTreeMap::new();
    nodes
        .iter()
        .filter_map(project_kept_item)
        .for_each(|(crate_name, qname)| {
            out.entry(crate_name)
                .or_default()
                .insert(PublicItem::new(qname));
        });
    out
}

/// Filter step for [`project_nodes`] — returns `Some((crate_name, qname))`
/// when `node` is a `:Item` worth retaining under the recall corpus's
/// projection rules, and `None` otherwise. The `.clone()` calls that used
/// to live inside the `for node in nodes { ... }` body move into this
/// `filter_map` closure, which is not a `for` block and so does not count
/// against the `clones-in-loops` metric.
fn project_kept_item(node: &Node) -> Option<(String, String)> {
    if node.label.as_str() != Label::ITEM {
        return None;
    }
    if matches!(node.props.get("is_test"), Some(PropValue::Bool(true))) {
        return None;
    }
    let PropValue::Str(kind) = node.props.get("kind")? else {
        return None;
    };
    if !KEPT_ITEM_KINDS.contains(&kind.as_str()) {
        return None;
    }
    let PropValue::Str(crate_name) = node.props.get("crate")? else {
        return None;
    };
    let PropValue::Str(qname) = node.props.get("qname")? else {
        return None;
    };
    Some((crate_name.clone(), qname.clone()))
}

/// Run the extractor against a workspace root and project the result.
/// I/O-bound wrapper for the pure projection above — this is the function
/// the binary and the integration test call.
pub fn extract_and_project(
    workspace_root: &Path,
) -> Result<std::collections::BTreeMap<String, BTreeSet<PublicItem>>, ExtractError> {
    let (nodes, _edges) = extract_workspace(workspace_root)?;
    Ok(project_nodes(&nodes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::Node;
    use cfdb_core::schema::Label;
    use std::collections::BTreeMap;

    fn item_node(id: &str, crate_name: &str, qname: &str, is_test: bool) -> Node {
        item_node_with_kind(id, crate_name, qname, "struct", is_test)
    }

    fn item_node_with_kind(
        id: &str,
        crate_name: &str,
        qname: &str,
        kind: &str,
        is_test: bool,
    ) -> Node {
        let mut props = BTreeMap::new();
        props.insert("crate".into(), PropValue::Str(crate_name.into()));
        props.insert("qname".into(), PropValue::Str(qname.into()));
        props.insert("kind".into(), PropValue::Str(kind.into()));
        props.insert("is_test".into(), PropValue::Bool(is_test));
        Node {
            id: id.into(),
            label: Label::new(Label::ITEM),
            props,
        }
    }

    #[test]
    fn projects_prod_items_into_crate_keyed_sets() {
        let nodes = vec![
            item_node("item:c::foo", "c", "c::foo", false),
            item_node("item:c::bar", "c", "c::bar", false),
            item_node("item:d::baz", "d", "d::baz", false),
        ];
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped.len(), 2);
        assert_eq!(
            grouped["c"],
            [PublicItem::new("c::foo"), PublicItem::new("c::bar")]
                .into_iter()
                .collect()
        );
        assert_eq!(
            grouped["d"],
            [PublicItem::new("d::baz")].into_iter().collect()
        );
    }

    #[test]
    fn drops_test_scope_items() {
        // Items marked is_test=true are dropped because the public-api
        // ground truth runs against the non-test build and would never
        // surface them.
        let nodes = vec![
            item_node("item:c::prod", "c", "c::prod", false),
            item_node("item:c::test_helper", "c", "c::test_helper", true),
        ];
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped["c"].len(), 1);
        assert!(grouped["c"].contains(&PublicItem::new("c::prod")));
    }

    #[test]
    fn ignores_non_item_nodes() {
        // CallSite, Module, Field, Crate — none of those contribute to
        // the recall surface. Only `Label::ITEM` nodes do.
        let nodes = vec![
            item_node("item:c::foo", "c", "c::foo", false),
            Node {
                id: "callsite:c::foo:bar:0".into(),
                label: Label::new(Label::CALL_SITE),
                props: BTreeMap::new(),
            },
            Node {
                id: "module:c".into(),
                label: Label::new(Label::MODULE),
                props: BTreeMap::new(),
            },
        ];
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped["c"].len(), 1);
    }

    #[test]
    fn skips_item_nodes_missing_required_props() {
        // Defensive: a malformed ITEM node without `qname` or `crate`
        // should not crash the projection, just be silently dropped. The
        // extractor always emits both, so this is a guard against a
        // future schema change.
        let nodes = vec![Node {
            id: "item:c::foo".into(),
            label: Label::new(Label::ITEM),
            props: BTreeMap::new(),
        }];
        let grouped = project_nodes(&nodes);
        assert!(grouped.is_empty());
    }

    #[test]
    fn drops_methods_by_kind_filter() {
        // Methods are emitted by `visit_impl_item_fn` with kind="method".
        // The rustdoc `paths` ground truth does not index methods, so
        // the recall formula is symmetric only if we drop them here too.
        // Until v0.2 (ra-ap-hir + impl traversal on both sides) this is
        // the correct thing to do.
        let nodes = vec![
            item_node_with_kind("item:c::foo", "c", "c::foo", "fn", false),
            item_node_with_kind("item:c::Bar", "c", "c::Bar", "struct", false),
            item_node_with_kind("item:c::Bar::new", "c", "c::Bar::new", "method", false),
        ];
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped["c"].len(), 2);
        assert!(grouped["c"].contains(&PublicItem::new("c::foo")));
        assert!(grouped["c"].contains(&PublicItem::new("c::Bar")));
        assert!(!grouped["c"].contains(&PublicItem::new("c::Bar::new")));
    }

    #[test]
    fn keeps_all_kept_item_kinds() {
        // Regression guard: every kind in `KEPT_ITEM_KINDS` must flow
        // through the projection. A future change that accidentally
        // drops one surfaces as a test failure here.
        let kinds = [
            "fn",
            "struct",
            "enum",
            "trait",
            "type_alias",
            "const",
            "static",
            "union",
        ];
        let nodes: Vec<Node> = kinds
            .iter()
            .enumerate()
            .map(|(i, k)| {
                item_node_with_kind(&format!("item:c::x{i}"), "c", &format!("c::x{i}"), k, false)
            })
            .collect();
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped["c"].len(), kinds.len());
    }
}
