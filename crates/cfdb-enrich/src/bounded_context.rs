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
