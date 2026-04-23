//! `enrich_metrics` — populate `unwrap_count`, `cyclomatic`,
//! `test_coverage`, and `dup_cluster_id` on `:Item{kind:"Fn"}` nodes
//! (RFC-036 §3.3 / issue #203).
//!
//! # SRP decomposition (RFC-036 §3.3)
//!
//! | Submodule | Responsibility |
//! |---|---|
//! | [`ast_signals`] | `.unwrap()` / `.expect()` count + McCabe cyclomatic from a parsed `syn::File` |
//! | [`coverage`] | `cargo-llvm-cov` JSON → per-qname `f64` line-coverage ratio (subfeature `llvm-cov`) |
//! | [`clustering`] | Group items by `signature_hash`, emit `dup_cluster_id = sha256(lex_sorted(member_qnames).join("\n"))` for clusters of size ≥ 2 |
//!
//! # Stateless full re-walk (RFC-036 §3.3)
//!
//! No `changed_files` parameter; every call re-parses every distinct
//! source file referenced by a `:Item{kind:"Fn"}.file` prop. Cheap —
//! syn is fast and the file set is bounded by the crate count, not the
//! item count.
//!
//! # Determinism under rayon (RFC-036 §3.3 / specs/concepts/cfdb-petgraph.md)
//!
//! Per-file parsing is parallelised with rayon. Sort-before-emit: the
//! computed `(qname, AstSignals)` pairs are collected into a
//! `BTreeMap<String, AstSignals>` keyed by qname — deterministic
//! iteration regardless of thread scheduling. `dup_cluster_id` emission
//! order is driven by the `BTreeMap<signature_hash, Vec<qname>>` which
//! is also inherently sorted. G1 canonical-dump sha256 is therefore
//! stable across runs (modulo `test_coverage` which is excluded per G6).

pub(crate) mod ast_signals;
pub(crate) mod clustering;
pub(crate) mod coverage;

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;

use crate::graph::KeyspaceState;

pub(crate) const VERB: &str = "enrich_metrics";

/// Per-run configuration. Exposed so the CLI composition root can decide
/// whether to load a cargo-llvm-cov JSON file without forcing every
/// caller to provide one. `coverage_json` is `None` on most dogfood runs
/// — the three ast_signals / clustering attrs still populate.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Optional path to a `cargo llvm-cov --json` output file. When
    /// `Some`, `:Item{kind:"Fn"}.test_coverage` is populated from the
    /// file's per-function covered-line ratio. When `None`, the attr is
    /// left absent on every item (consumers interpret absence as "no
    /// coverage data available for this run").
    pub coverage_json: Option<std::path::PathBuf>,
}

/// Entry point called by `impl EnrichBackend for PetgraphStore` in
/// `enrich_backend.rs`. Returns `EnrichReport` by value — never `Err`.
/// Keyspace-not-found and workspace-root-missing are handled upstream.
pub(crate) fn run(
    state: &mut KeyspaceState,
    workspace_root: &Path,
    config: &Config,
) -> EnrichReport {
    let mut warnings: Vec<String> = Vec::new();

    let items = collect_fn_items(state);
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

    // ast_signals: parse each distinct source file once, index by qname.
    let signals_by_qname = ast_signals::scan_workspace(&items, workspace_root, &mut warnings);

    // Coverage: optional, subfeature-gated at the call site. Absent → empty map.
    let coverage_by_qname = match config.coverage_json.as_deref() {
        Some(path) => coverage::load_from_path(path, &mut warnings),
        None => BTreeMap::new(),
    };

    // Clustering: group by signature_hash prop on the node, emit
    // dup_cluster_id for clusters of size ≥ 2.
    let cluster_id_by_qname = clustering::compute_dup_cluster_ids(&items);

    // Apply all three signal maps to the graph. One pass per item;
    // props written in sorted-qname order (items is already sorted).
    let attrs_written = apply_attrs(
        state,
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

/// Projection of a `:Item{kind:"Fn"}` node needed by every submodule.
/// Sorted by qname across the whole function so downstream iteration is
/// deterministic without re-sorting.
#[derive(Debug, Clone)]
pub(crate) struct FnItem {
    pub(crate) qname: String,
    pub(crate) name: String,
    pub(crate) file: String,
    pub(crate) signature_hash: Option<String>,
    pub(crate) node_idx: petgraph::stable_graph::NodeIndex,
}

fn collect_fn_items(state: &KeyspaceState) -> Vec<FnItem> {
    let item_label = Label::new(Label::ITEM);
    let mut out: Vec<FnItem> = state
        .nodes_with_label(&item_label)
        .into_iter()
        .filter_map(|idx| {
            let node = state.graph.node_weight(idx)?;
            let kind = node.props.get("kind").and_then(PropValue::as_str)?;
            // Extractor emits lowercase `"fn"` per
            // cfdb-extractor::item_visitor::visits.rs:61. The schema
            // describe doc uses the Rust-camel-case `Fn` in prose but
            // the on-wire value is lowercase.
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
                node_idx: idx,
            })
        })
        .collect();
    out.sort_by(|a, b| a.qname.cmp(&b.qname));
    out
}

fn apply_attrs(
    state: &mut KeyspaceState,
    items: &[FnItem],
    signals: &BTreeMap<String, ast_signals::AstSignals>,
    coverage: &BTreeMap<String, f64>,
    clusters: &BTreeMap<String, String>,
) -> u64 {
    let mut count: u64 = 0;
    for item in items {
        let Some(node) = state.graph.node_weight_mut(item.node_idx) else {
            continue;
        };
        if let Some(sig) = signals.get(&item.qname) {
            node.props.insert(
                "unwrap_count".into(),
                PropValue::Int(i64::try_from(sig.unwrap_count).unwrap_or(i64::MAX)),
            );
            node.props.insert(
                "cyclomatic".into(),
                PropValue::Int(i64::try_from(sig.cyclomatic).unwrap_or(i64::MAX)),
            );
            count = count.saturating_add(2);
        }
        if let Some(&cov) = coverage.get(&item.qname) {
            node.props
                .insert("test_coverage".into(), PropValue::Float(cov));
            count = count.saturating_add(1);
        }
        if let Some(cluster_id) = clusters.get(&item.qname) {
            node.props
                .insert("dup_cluster_id".into(), PropValue::Str(cluster_id.clone()));
            count = count.saturating_add(1);
        }
    }
    count
}
