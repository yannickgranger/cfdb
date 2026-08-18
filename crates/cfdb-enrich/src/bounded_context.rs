//! `enrich_bounded_context` — re-read `.cfdb/concepts/*.toml` and patch
//! `:Item.bounded_context` on crates whose TOML mapping changed between
//! extractions.
//!
//! # Scope — this is a re-enrichment pass
//!
//! The extract-time path in `cfdb-extractor::lib.rs` already populates
//! `:Item.bounded_context` for every item via
//! `cfdb_concepts::compute_bounded_context` (overrides first, heuristic
//! fallback). On a **fresh extraction** this pass is a no-op: every item's
//! stored value already matches what the current TOML + heuristic would
//! produce, so `attrs_written = 0, ran = true`.
//!
//! The pass earns its keep when `.cfdb/concepts/*.toml` files change
//! *between extractions* — a full re-extract would be expensive, but
//! `enrich-bounded-context` re-reads the TOML and patches just the
//! `:Item.bounded_context` props on items whose owning crate's mapping
//! changed. Extract-time-derived `:Context` nodes and `:Crate -[:BELONGS_TO]->
//! :Context` edges are NOT re-wired here (re-extract is the supported path
//! for those); only the per-item attribute is patched.
//!
//! # Single resolution point (no split-brain)
//!
//! Both the extract-time path and this re-enrichment path call into the
//! same `cfdb_concepts::compute_bounded_context` — the override-first,
//! heuristic-fallback resolution lives in exactly one place. If a future
//! change alters the heuristic, `audit-split-brain` will not be able to
//! detect a divergence because there is nowhere for one to arise.
//!
//! # Determinism
//!
//! - Expected-mapping memoisation uses a `BTreeMap<crate_name, String>`.
//! - Item ids come from `nodes_with_label`, which per the port contract
//!   preserves whatever ordering guarantee the underlying storage already
//!   provides (G1).
//! - Patches are applied in iteration order; the mutation order does not
//!   affect canonical-dump output (canonical dump re-sorts by `(label,
//!   qname)` regardless).

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_concepts::{compute_bounded_context, ConceptOverrides};
use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphView;
use cfdb_core::schema::Label;

pub(crate) const VERB: &str = "enrich_bounded_context";
const ATTR: &str = "bounded_context";
const ITEM_CRATE_PROP: &str = "crate";

/// Entry point called by [`crate::EnrichEngine`].
///
/// Returns `EnrichReport` by value — never `Err`. Keyspace-not-found and
/// workspace-root-missing are handled upstream in `lib.rs`. A TOML parse
/// error surfaces as a warning with `ran: false` (we prefer a loud failure
/// over a silent partial patch).
pub(crate) fn run(view: &mut dyn GraphView, workspace_root: &Path) -> EnrichReport {
    let overrides = match cfdb_concepts::load_concept_overrides(workspace_root) {
        Ok(o) => o,
        Err(e) => {
            return EnrichReport {
                verb: VERB.into(),
                ran: false,
                facts_scanned: 0,
                attrs_written: 0,
                edges_written: 0,
                warnings: vec![format!(
                    "{VERB}: failed to load `.cfdb/concepts/*.toml` under {workspace_root:?}: {e}"
                )],
            };
        }
    };

    let item_ids = view.nodes_with_label(&Label::new(Label::ITEM));
    if item_ids.is_empty() {
        return EnrichReport {
            verb: VERB.into(),
            ran: true,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![format!(
                "{VERB}: no :Item nodes in keyspace — nothing to enrich"
            )],
        };
    }

    let patches = collect_patches(view, &item_ids, &overrides);
    let attrs_written = apply_patches(view, patches);

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: u64::try_from(item_ids.len()).unwrap_or(u64::MAX),
        attrs_written,
        edges_written: 0,
        warnings: Vec::new(),
    }
}

/// Determine which `:Item` nodes need their `bounded_context` patched.
/// Returns `(id, expected_context)` pairs. Expected-per-crate values are
/// memoised in a `BTreeMap` so `compute_bounded_context` runs O(distinct
/// crates), not O(items).
fn collect_patches(
    view: &dyn GraphView,
    item_ids: &[String],
    overrides: &ConceptOverrides,
) -> Vec<(String, String)> {
    let mut memo: BTreeMap<String, String> = BTreeMap::new();
    item_ids
        .iter()
        .filter_map(|id| diff_one_item(view, id, overrides, &mut memo).map(|s| (id.clone(), s)))
        .collect()
}

/// For a single `:Item`: look up the current `bounded_context`, compute the
/// expected value from the overrides + heuristic, and return `Some(expected)`
/// iff they differ (or `None` if already correct / no crate prop / node
/// missing).
fn diff_one_item(
    view: &dyn GraphView,
    id: &str,
    overrides: &ConceptOverrides,
    memo: &mut BTreeMap<String, String>,
) -> Option<String> {
    let node = view.node_by_id(id)?;
    let crate_name = prop_str(node, ITEM_CRATE_PROP)?;
    let expected = expected_for_crate(memo, &crate_name, overrides);
    let current = prop_str(node, ATTR).unwrap_or_default();
    if current == *expected {
        None
    } else {
        Some(expected.clone())
    }
}

/// Memoised lookup: `crate_name -> compute_bounded_context(crate_name, overrides)`.
/// Returns a borrowed reference so the caller only clones on mismatch.
fn expected_for_crate<'a>(
    memo: &'a mut BTreeMap<String, String>,
    crate_name: &str,
    overrides: &ConceptOverrides,
) -> &'a String {
    if !memo.contains_key(crate_name) {
        memo.insert(
            crate_name.to_string(),
            compute_bounded_context(crate_name, overrides).name,
        );
    }
    memo.get(crate_name)
        .expect("just inserted if absent — present now")
}

/// Apply the patches to the graph. Returns the number of attrs written
/// (which equals `patches.len()` unless a node has since been removed).
fn apply_patches(view: &mut dyn GraphView, patches: Vec<(String, String)>) -> u64 {
    let mut count: u64 = 0;
    for (id, expected) in patches {
        if view.set_attr(&id, ATTR, PropValue::Str(expected)) {
            count += 1;
        }
    }
    count
}

fn prop_str(node: &cfdb_core::fact::Node, key: &str) -> Option<String> {
    node.props
        .get(key)
        .and_then(PropValue::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests;
