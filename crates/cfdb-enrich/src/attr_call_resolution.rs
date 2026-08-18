//! `attr_call_resolution` — post-pass that flips `:Item.reachable_from_entry`
//! to `true` for fn items invoked by an attribute-driven derived impl that
//! cfdb cannot trace via the normal call graph.
//!
//! # The recall gap this closes
//!
//! `#[serde(default = "fn")]` on a struct field references a callable that
//! serde's derived `Deserialize` impl invokes when the field is missing.
//! The derive expansion is invisible to cfdb (the proc-macro server is
//! disabled), so the BFS in [`crate::reachability`] never reaches the
//! callee through a CALLS chain. Without this post-pass, every
//! `#[serde(default = "fn")]` callee is flagged `unwired` even when
//! the owning struct is actively deserialised.
//!
//! The syn-side extractor (`cfdb-extractor::item_visitor::visits`) DOES
//! emit a `:CallSite{kind="serde_default", callee_path="..."}` node + an
//! `INVOKES_AT(struct, callsite)` edge for each such attribute. The
//! callsite is dangling for reachability purposes because:
//!
//! 1. Structs are types, not call-graph nodes — nothing reaches the struct
//!    via CALLS, so BFS never visits its outgoing INVOKES_AT.
//! 2. The callsite has no outgoing CALLS edge to the callee `:Item` (the
//!    schema disallows `CallSite -[:CALLS]-> Item` per `EdgeLabelDescriptor`
//!    at `cfdb-core/src/schema/describe/edges.rs`; CALLS is
//!    `Item -> Item` only).
//!
//! # What this pass does
//!
//! For every `:CallSite{kind="serde_default"}`:
//!
//! 1. Read `callee_path` (the literal string the author wrote in the attr).
//! 2. Resolve `callee_path` to an `:Item` node id using three candidate
//!    strategies in order — exact, same-module, same-crate. The first
//!    match wins.
//! 3. If a candidate matches, set `:Item.reachable_from_entry = true` on
//!    the resolved node. `:Item.reachable_entry_count` is intentionally
//!    NOT incremented — the count semantic is preserved as "distinct
//!    BFS-reaching entry points" and the attr-call resolution does not
//!    correspond to a specific entry point.
//!
//! Unresolvable `callee_path`s (e.g. cross-crate paths to a dep not in the
//! workspace) are skipped silently — the recall gain is bounded by the
//! callee being in the same workspace, which is the common case.
//!
//! # Why a post-pass, not a new enrichment verb
//!
//! Adding a top-level `enrich_attr_call_resolution` verb would expand the
//! `EnrichBackend` trait surface, the CLI verb table, and the
//! `:SchemaDescribe` output for what is in essence a single targeted
//! recall fix. Folding it into [`crate::reachability::run`] keeps the
//! cost local to the one consumer (the unwired classifier) that cares.

use std::collections::BTreeMap;

use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphView;
use cfdb_core::qname::{item_node_id, item_node_id_for_target, TargetDiscriminator};
use cfdb_core::schema::{Direction, EdgeLabel, Label};

/// `:CallSite.kind` value scoped by this post-pass. Any callsite with a
/// different kind (e.g. the regular `"call"` / `"method"` / `"fn"`
/// emissions from the syn or HIR extractor) is ignored.
const KIND_SERDE_DEFAULT: &str = "serde_default";

/// Walk every `:CallSite{kind="serde_default"}` and mark its resolved
/// callee `:Item` as `<reach_attr> = true`. Returns the number of attrs
/// written (one per successful resolution).
///
/// `reach_attr` is the `:Item` boolean prop name the post-pass writes
/// — `"reachable_from_entry"` for the All-filter pass, or
/// `"reachable_from_production_entry"` for the ProductionOnly-filter
/// pass. Serde deserialize callbacks are production code, so the
/// post-pass is invoked once per filter and writes the corresponding
/// attr each time.
///
/// Resolution strategies attempted in order:
///
/// 1. **Exact** — `item:<callee_path>` already exists. The author wrote
///    a fully-qualified path (e.g. `chrono::Utc::now`) AND the crate is
///    in the workspace.
/// 2. **Same-module** — `item:<caller_module>::<callee_path>` exists.
///    The author wrote the short form (`default_url`) and the fn lives
///    in the same module as the owning struct.
/// 3. **Same-crate** — `item:<caller_crate>::<callee_path>` exists.
///    The author wrote a crate-relative path (`config::default_url`) and
///    the fn is reachable from the crate root.
///
/// Misses are silent — the caller logs nothing because a missing
/// callee_path resolution simply means the recall improvement does not
/// apply to this particular attr. This is a graceful degradation in absence
/// of a definitive resolution.
pub(crate) fn mark_serde_default_callees_reachable(
    view: &mut dyn GraphView,
    reach_attr: &str,
) -> u64 {
    let resolutions = collect_resolutions(view);
    apply_resolutions(view, &resolutions, reach_attr)
}

/// Pure-data scan — walk every `:CallSite` once, project the
/// `(callsite_id → resolved_item_id)` mapping. Split out from
/// [`apply_resolutions`] so the borrow-checker is satisfied: the scan
/// takes `&view`, the apply takes `&mut view`.
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
        // The caller's target comes from the graph's own structure — the
        // incoming INVOKES_AT edge names the exact owning `:Item` (emitted
        // with the discriminated src), so no value-join on display props
        // exists to be wrong, and nothing is scanned when the keyspace
        // carries no serde-default sites. Keyspaces without this targeting
        // have no `target` prop ⇒ `None` ⇒ bare-id candidates only.
        let caller_target = caller_target_via_invokes_at(view, &cs_id);
        if let Some(item_id) =
            resolve_callee_to_item(view, callee_path, caller_qname, caller_target.as_ref())
        {
            out.insert(cs_id, item_id);
        }
    }
    out
}

/// The owning `:Item`'s target discriminator, read off the incoming
/// `INVOKES_AT` edge. One hop, no ambiguity; wire→discriminator parsing
/// stays in cfdb-core.
fn caller_target_via_invokes_at(view: &dyn GraphView, cs_id: &str) -> Option<TargetDiscriminator> {
    view.neighbors(cs_id, Direction::In)
        .into_iter()
        .find(|(label, _)| label.as_str() == EdgeLabel::INVOKES_AT)
        .and_then(|(_, owner_id)| view.node_by_id(&owner_id))
        .and_then(|owner| owner.props.get("target").and_then(PropValue::as_str))
        .and_then(TargetDiscriminator::from_wire_str)
}

/// Write `<reach_attr> = true` on each resolved item. Returns the number
/// of attrs written.
///
/// Items already marked `true` by the BFS are still written — idempotent
/// `insert` is cheaper than a per-item read-modify-write check, and the
/// count is the number of *resolutions*, not the number of *flips*.
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

/// Try the three candidate qname forms against the graph, return the
/// first matching `:Item` id. See [`mark_serde_default_callees_reachable`]
/// for the strategy ordering rationale.
fn resolve_callee_to_item(
    view: &dyn GraphView,
    callee_path: &str,
    caller_qname: &str,
    caller_target: Option<&TargetDiscriminator>,
) -> Option<String> {
    // Each strategy tries the caller's own identity namespace first
    // (same target, else the bare/lib id — a foreign bin's id is never
    // constructed).
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
    // Strategy 1 — exact match. Author wrote the fully-qualified path
    // and the callee lives in the workspace.
    if let Some(id) = lookup(callee_path) {
        return Some(id);
    }
    // Strategy 2 — same-module. Strip the last `::` segment from
    // caller_qname to recover the module path, then prepend it.
    if let Some((module_path, _last)) = caller_qname.rsplit_once("::") {
        if let Some(id) = lookup(&format!("{module_path}::{callee_path}")) {
            return Some(id);
        }
    }
    // Strategy 3 — same-crate. First `::` segment of caller_qname is
    // the crate name (cfdb's qname convention, see
    // `cfdb-core::qname::item_qname`). Prepend it.
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

    /// A store holding exactly `nodes` in one keyspace; the resolver under
    /// test only ever sees the keyspace's `GraphView`.
    fn store_with(nodes: Vec<Node>) -> (PetgraphStore, Keyspace) {
        let ks = Keyspace::new("test");
        let mut store = PetgraphStore::new();
        store.ingest_nodes(&ks, nodes).expect("ingest");
        (store, ks)
    }

    /// Smoke test on the resolver in isolation — exact match wins
    /// before same-module.
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

    /// Same-module fallback when no exact match exists.
    #[test]
    fn resolver_falls_back_to_same_module() {
        let (mut store, ks) = store_with(vec![make_item("myapp::config::default_url")]);
        let view = store.graph_view(&ks).expect("keyspace");
        let resolved =
            resolve_callee_to_item(view, "default_url", "myapp::config::AppConfig", None);
        assert!(resolved.is_some(), "same-module resolution must succeed");
    }

    /// Same-crate fallback for crate-relative paths.
    #[test]
    fn resolver_falls_back_to_same_crate() {
        let (mut store, ks) = store_with(vec![make_item("myapp::config::default_url")]);
        let view = store.graph_view(&ks).expect("keyspace");
        let resolved =
            resolve_callee_to_item(view, "config::default_url", "myapp::main::AppConfig", None);
        assert!(resolved.is_some(), "same-crate resolution must succeed");
    }

    /// Unresolvable returns None, never panics.
    #[test]
    fn resolver_returns_none_for_unknown_path() {
        let (mut store, ks) = store_with(vec![]);
        let view = store.graph_view(&ks).expect("keyspace");
        assert!(resolve_callee_to_item(view, "nowhere::fn", "myapp::AppConfig", None).is_none());
    }

    #[test]
    fn bin_target_caller_resolves_bin_local_callee_same_target_first() {
        // A #[serde(default = "...")] callee defined in a bin target lives
        // at a discriminated id; the caller's target context must route the
        // candidate there, with the bare (lib) id as fallback — never a
        // foreign bin.
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
        // A foreign-bin suffix must NOT resolve to alpha's item (and there
        // is no lib fallback here), mirroring rustc visibility.
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
