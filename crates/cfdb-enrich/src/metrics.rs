pub(crate) mod ast_signals;
pub(crate) mod clustering;
pub(crate) mod coverage;

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphView;
use cfdb_core::schema::Label;

pub(crate) const VERB: &str = "enrich_metrics";

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub coverage_json: Option<std::path::PathBuf>,
}

pub(crate) fn run(
    view: &mut dyn GraphView,
    workspace_root: &Path,
    config: &Config,
) -> EnrichReport {
    let mut warnings: Vec<String> = Vec::new();

    let items = collect_fn_items(view);
    let facts_scanned = u64::try_from(items.len()).unwrap_or(u64::MAX);

    if items.is_empty() {
        return EnrichReport {
            verb: VERB.into(),
            ran: true,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![format!(
                "{VERB}: no :Item{{kind:Fn}} nodes in keyspace — nothing to enrich"
            )],
        };
    }

    let signals_by_qname = ast_signals::scan_workspace(&items, workspace_root, &mut warnings);

    let coverage_by_qname = match config.coverage_json.as_deref() {
        Some(path) => coverage::load_from_path(path, &mut warnings),
        None => BTreeMap::new(),
    };

    let cluster_id_by_qname = clustering::compute_dup_cluster_ids(&items);

    let attrs_written = apply_attrs(
        view,
        &items,
        &signals_by_qname,
        &coverage_by_qname,
        &cluster_id_by_qname,
    );

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned,
        attrs_written,
        edges_written: 0,
        warnings,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FnItem {
    pub(crate) qname: String,
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) signature_hash: Option<String>,
    pub(crate) id: String,
}

fn collect_fn_items(view: &dyn GraphView) -> Vec<FnItem> {
    let item_label = Label::new(Label::ITEM);
    let mut out: Vec<FnItem> = view
        .nodes_with_label(&item_label)
        .into_iter()
        .filter_map(|id| {
            let node = view.node_by_id(&id)?;
            let kind = node.props.get("kind").and_then(PropValue::as_str)?;
            if kind != "fn" {
                return None;
            }
            let qname = node.props.get("qname").and_then(PropValue::as_str)?;
            let name = node.props.get("name").and_then(PropValue::as_str)?;
            let file = node.props.get("file").and_then(PropValue::as_str)?;
            let signature_hash = node
                .props
                .get("signature_hash")
                .and_then(PropValue::as_str)
                .map(str::to_string);
            Some(FnItem {
                qname: qname.to_string(),
                name: name.to_string(),
                file: file.to_string(),
                signature_hash,
                id,
            })
        })
        .collect();
    out.sort_by(|a, b| a.qname.cmp(&b.qname));
    out
}

fn apply_attrs(
    view: &mut dyn GraphView,
    items: &[FnItem],
    signals: &BTreeMap<String, ast_signals::AstSignals>,
    coverage: &BTreeMap<String, f64>,
    clusters: &BTreeMap<String, String>,
) -> u64 {
    let mut count: u64 = 0;
    for item in items {
        count = count.saturating_add(apply_item_attrs(view, item, signals, coverage, clusters));
    }
    count
}

fn apply_item_attrs(
    view: &mut dyn GraphView,
    item: &FnItem,
    signals: &BTreeMap<String, ast_signals::AstSignals>,
    coverage: &BTreeMap<String, f64>,
    clusters: &BTreeMap<String, String>,
) -> u64 {
    if view.node_by_id(&item.id).is_none() {
        return 0;
    }
    let mut count: u64 = 0;
    if let Some(sig) = signals.get(&item.qname) {
        view.set_attr(
            &item.id,
            "unwrap_count",
            PropValue::Int(i64::try_from(sig.unwrap_count).unwrap_or(i64::MAX)),
        );
        view.set_attr(
            &item.id,
            "cyclomatic",
            PropValue::Int(i64::try_from(sig.cyclomatic).unwrap_or(i64::MAX)),
        );
        count = count.saturating_add(2);
    }
    if let Some(&cov) = coverage.get(&item.qname) {
        view.set_attr(&item.id, "test_coverage", PropValue::Float(cov));
        count = count.saturating_add(1);
    }
    if let Some(cluster_id) = clusters.get(&item.qname) {
        view.set_attr(
            &item.id,
            "dup_cluster_id",
            PropValue::Str(cluster_id.clone()),
        );
        count = count.saturating_add(1);
    }
    count
}

#[cfg(test)]
mod tests;
