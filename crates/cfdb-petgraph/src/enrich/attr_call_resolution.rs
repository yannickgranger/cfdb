//! `attr_call_resolution` — post-pass that flips `:Item.reachable_from_entry`
//! to `true` for fn items invoked by an attribute-driven derived impl that
//! cfdb cannot trace via the normal call graph (issue #396).
//!
//! # The recall gap this closes
//!
//! `#[serde(default = "fn")]` on a struct field references a callable that
//! serde's derived `Deserialize` impl invokes when the field is missing.
//! The derive expansion is invisible to cfdb (the proc-macro server is
//! disabled — see issue #398), so the BFS in [`super::reachability`]
//! never reaches the callee through a CALLS chain. Without this post-pass,
//! every `#[serde(default = "fn")]` callee is flagged `unwired` even when
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
//!    at `cfdb-core/src/schema/describe/edges.rs:114-115`; CALLS is
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
//! recall fix. Folding it into [`super::reachability::run`] keeps the
//! cost local to the one consumer (the unwired classifier) that cares.

use std::collections::BTreeMap;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;

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
/// pass (RFC-042 042-B). Serde deserialize callbacks are production
/// code, so the post-pass is invoked once per filter and writes the
/// corresponding attr each time.
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
/// apply to this particular attr. This matches the rest of cfdb's
/// degraded-pass discipline (RFC §6 graceful degradation).
pub(crate) fn mark_serde_default_callees_reachable(
    state: &mut KeyspaceState,
    reach_attr: &str,
) -> u64 {
    let resolutions = collect_resolutions(state);
    apply_resolutions(state, &resolutions, reach_attr)
}

/// Pure-data scan — walk every `:CallSite` once, project the
/// `(callsite_idx → resolved_item_idx)` mapping. Split out from
/// [`apply_resolutions`] so the borrow-checker is satisfied: the scan
/// takes `&state`, the apply takes `&mut state`.
fn collect_resolutions(state: &KeyspaceState) -> BTreeMap<NodeIndex, NodeIndex> {
    let callsites = state.nodes_with_label(&Label::new(Label::CALL_SITE));
    let mut out: BTreeMap<NodeIndex, NodeIndex> = BTreeMap::new();
    for cs_idx in callsites {
        let Some(cs_node) = state.graph.node_weight(cs_idx) else {
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
        if let Some(item_idx) = resolve_callee_to_item(state, callee_path, caller_qname) {
            out.insert(cs_idx, item_idx);
        }
    }
    out
}

/// Write `<reach_attr> = true` on each resolved item. Returns the number
/// of attrs written.
///
/// Items already marked `true` by the BFS are still written — idempotent
/// `insert` is cheaper than a per-item read-modify-write check, and the
/// count is the number of *resolutions*, not the number of *flips*.
fn apply_resolutions(
    state: &mut KeyspaceState,
    resolutions: &BTreeMap<NodeIndex, NodeIndex>,
    reach_attr: &str,
) -> u64 {
    let mut count: u64 = 0;
    for &item_idx in resolutions.values() {
        if let Some(node) = state.graph.node_weight_mut(item_idx) {
            node.props
                .insert(reach_attr.to_string(), PropValue::Bool(true));
            count += 1;
        }
    }
    count
}

/// Try the three candidate qname forms against the id index, return the
/// first matching `:Item` NodeIndex. See [`mark_serde_default_callees_reachable`]
/// for the strategy ordering rationale.
fn resolve_callee_to_item(
    state: &KeyspaceState,
    callee_path: &str,
    caller_qname: &str,
) -> Option<NodeIndex> {
    // Strategy 1 — exact match. Author wrote the fully-qualified path
    // and the callee lives in the workspace.
    let exact_id = format!("item:{callee_path}");
    if let Some(&idx) = state.id_to_idx.get(&exact_id) {
        return Some(idx);
    }
    // Strategy 2 — same-module. Strip the last `::` segment from
    // caller_qname to recover the module path, then prepend it.
    if let Some((module_path, _last)) = caller_qname.rsplit_once("::") {
        let candidate = format!("item:{module_path}::{callee_path}");
        if let Some(&idx) = state.id_to_idx.get(&candidate) {
            return Some(idx);
        }
    }
    // Strategy 3 — same-crate. First `::` segment of caller_qname is
    // the crate name (cfdb's qname convention, see
    // `cfdb-core::qname::item_qname`). Prepend it.
    if let Some((crate_name, _rest)) = caller_qname.split_once("::") {
        let candidate = format!("item:{crate_name}::{callee_path}");
        if let Some(&idx) = state.id_to_idx.get(&candidate) {
            return Some(idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::{Node, Props};

    fn make_item(qname: &str) -> Node {
        let mut props = Props::new();
        props.insert("qname".into(), PropValue::Str(qname.into()));
        Node {
            id: format!("item:{qname}"),
            label: Label::new(Label::ITEM),
            props,
        }
    }

    /// Smoke test on the resolver in isolation — exact match wins
    /// before same-module.
    #[test]
    fn resolver_prefers_exact_over_same_module() {
        let mut state = KeyspaceState::new();
        state.ingest_nodes(vec![
            make_item("myapp::config::default_url"),
            make_item("myapp::other::config::default_url"),
        ]);
        let resolved = resolve_callee_to_item(
            &state,
            "myapp::config::default_url",
            "myapp::other::config::AppConfig",
        );
        let idx = resolved.expect("exact match must win");
        let node = state.graph.node_weight(idx).expect("node");
        assert_eq!(
            node.props.get("qname").and_then(PropValue::as_str),
            Some("myapp::config::default_url")
        );
    }

    /// Same-module fallback when no exact match exists.
    #[test]
    fn resolver_falls_back_to_same_module() {
        let mut state = KeyspaceState::new();
        state.ingest_nodes(vec![make_item("myapp::config::default_url")]);
        let resolved = resolve_callee_to_item(&state, "default_url", "myapp::config::AppConfig");
        assert!(resolved.is_some(), "same-module resolution must succeed");
    }

    /// Same-crate fallback for crate-relative paths.
    #[test]
    fn resolver_falls_back_to_same_crate() {
        let mut state = KeyspaceState::new();
        state.ingest_nodes(vec![make_item("myapp::config::default_url")]);
        let resolved =
            resolve_callee_to_item(&state, "config::default_url", "myapp::main::AppConfig");
        assert!(resolved.is_some(), "same-crate resolution must succeed");
    }

    /// Unresolvable returns None, never panics.
    #[test]
    fn resolver_returns_none_for_unknown_path() {
        let state = KeyspaceState::new();
        assert!(resolve_callee_to_item(&state, "nowhere::fn", "myapp::AppConfig").is_none());
    }
}
