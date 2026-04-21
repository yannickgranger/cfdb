//! `enrich_bounded_context` — re-read `.cfdb/concepts/*.toml` and patch
//! `:Item.bounded_context` on crates whose TOML mapping changed between
//! extractions (slice 43-E / issue #108).
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
//! - Item indices come from `nodes_with_label` which returns a
//!   `BTreeSet`-sourced sorted slice.
//! - Patches are applied in iteration order; the mutation order does not
//!   affect canonical-dump output (canonical dump re-sorts by `(label,
//!   qname)` regardless).

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_concepts::{compute_bounded_context, ConceptOverrides};
use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::{PropValue, Props};
use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;

pub(crate) const VERB: &str = "enrich_bounded_context";
const ATTR: &str = "bounded_context";
const ITEM_CRATE_PROP: &str = "crate";

/// Entry point called by `impl EnrichBackend for PetgraphStore` in `crate`.
///
/// Returns `EnrichReport` by value — never `Err`. Keyspace-not-found and
/// workspace-root-missing are handled upstream in `lib.rs`. A TOML parse
/// error surfaces as a warning with `ran: false` (we prefer a loud failure
/// over a silent partial patch).
pub(crate) fn run(state: &mut KeyspaceState, workspace_root: &Path) -> EnrichReport {
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

    let item_indices = state.nodes_with_label(&Label::new(Label::ITEM));
    if item_indices.is_empty() {
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

    let patches = collect_patches(state, &item_indices, &overrides);
    let attrs_written = apply_patches(state, patches);

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: u64::try_from(item_indices.len()).unwrap_or(u64::MAX),
        attrs_written,
        edges_written: 0,
        warnings: Vec::new(),
    }
}

/// Determine which `:Item` nodes need their `bounded_context` patched.
/// Returns `(node_index, expected_context)` pairs. Expected-per-crate
/// values are memoised in a `BTreeMap` so `compute_bounded_context` runs
/// O(distinct crates), not O(items).
fn collect_patches(
    state: &KeyspaceState,
    item_indices: &[NodeIndex],
    overrides: &ConceptOverrides,
) -> Vec<(NodeIndex, String)> {
    let mut memo: BTreeMap<String, String> = BTreeMap::new();
    item_indices
        .iter()
        .filter_map(|&idx| diff_one_item(state, idx, overrides, &mut memo).map(|s| (idx, s)))
        .collect()
}

/// For a single `:Item`: look up the current `bounded_context`, compute the
/// expected value from the overrides + heuristic, and return `Some(expected)`
/// iff they differ (or `None` if already correct / no crate prop / node
/// missing).
fn diff_one_item(
    state: &KeyspaceState,
    idx: NodeIndex,
    overrides: &ConceptOverrides,
    memo: &mut BTreeMap<String, String>,
) -> Option<String> {
    let node = state.graph.node_weight(idx)?;
    let crate_name = prop_str(&node.props, ITEM_CRATE_PROP)?;
    let expected = expected_for_crate(memo, &crate_name, overrides);
    let current = prop_str(&node.props, ATTR).unwrap_or_default();
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
            compute_bounded_context(crate_name, overrides),
        );
    }
    memo.get(crate_name)
        .expect("just inserted if absent — present now")
}

/// Apply the patches to the graph. Returns the number of attrs written
/// (which equals `patches.len()` unless a node has since been removed).
fn apply_patches(state: &mut KeyspaceState, patches: Vec<(NodeIndex, String)>) -> u64 {
    let mut count: u64 = 0;
    for (idx, expected) in patches {
        if let Some(node) = state.graph.node_weight_mut(idx) {
            node.props.insert(ATTR.into(), PropValue::Str(expected));
            count += 1;
        }
    }
    count
}

fn prop_str(props: &Props, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(PropValue::as_str)
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::Path;

    use cfdb_core::enrich::EnrichBackend;
    use cfdb_core::fact::{Node, PropValue, Props};
    use cfdb_core::schema::{Keyspace, Label};
    use cfdb_core::store::StoreBackend;

    use crate::PetgraphStore;

    fn write(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdirs");
        }
        std::fs::write(&path, contents).expect("write");
    }

    /// Build a store containing `:Item` nodes with given `(qname, name, crate,
    /// bounded_context)` tuples. Useful for simulating a previously-extracted
    /// keyspace.
    fn store_with_items(workspace: &Path, items: &[(&str, &str, &str, &str)]) -> PetgraphStore {
        let mut store = PetgraphStore::new().with_workspace(workspace);
        let ks = Keyspace::new("test");
        let nodes: Vec<Node> = items
            .iter()
            .map(|(qname, name, crate_name, ctx)| {
                let mut props = Props::new();
                props.insert("qname".into(), PropValue::Str((*qname).into()));
                props.insert("name".into(), PropValue::Str((*name).into()));
                props.insert("crate".into(), PropValue::Str((*crate_name).into()));
                props.insert("bounded_context".into(), PropValue::Str((*ctx).into()));
                props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
                Node {
                    id: format!("item:{qname}"),
                    label: Label::new(Label::ITEM),
                    props,
                }
            })
            .collect();
        store.ingest_nodes(&ks, nodes).expect("ingest");
        store
    }

    fn get_bounded_context(store: &PetgraphStore, keyspace: &Keyspace, qname: &str) -> String {
        let (nodes, _) = store.export(keyspace).expect("export");
        nodes
            .iter()
            .find(|n| {
                n.props
                    .get("qname")
                    .and_then(PropValue::as_str)
                    .is_some_and(|q| q == qname)
            })
            .and_then(|n| n.props.get("bounded_context").and_then(PropValue::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| panic!("item {qname} or its bounded_context prop missing"))
    }

    // ------------------------------------------------------------------
    // AC-1: TOML override patches mismatched items.
    // ------------------------------------------------------------------

    #[test]
    fn ac1_toml_override_patches_mismatched_items() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Declare that `domain-trading` belongs to context `"trading"` (which
        // happens to match the heuristic — so we instead use a non-heuristic
        // mapping to prove the override wins).
        write(
            tmp.path(),
            ".cfdb/concepts/trading.toml",
            "name = \"custom-trading\"\ncrates = [\"domain-trading\"]\n",
        );
        // Extractor-time values: stale heuristic ("trading" from stripping
        // "domain-"). TOML now says "custom-trading" — re-enrichment must
        // patch.
        let mut store = store_with_items(
            tmp.path(),
            &[
                ("crate::A", "A", "domain-trading", "trading"),
                ("crate::B", "B", "domain-trading", "trading"),
            ],
        );
        let ks = Keyspace::new("test");
        let report = store.enrich_bounded_context(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.facts_scanned, 2);
        assert_eq!(report.attrs_written, 2, "both items patched");
        assert_eq!(
            get_bounded_context(&store, &ks, "crate::A"),
            "custom-trading"
        );
        assert_eq!(
            get_bounded_context(&store, &ks, "crate::B"),
            "custom-trading"
        );
    }

    // ------------------------------------------------------------------
    // AC-2: no TOML changes (or no TOML at all) → no-op, ran=true.
    // ------------------------------------------------------------------

    #[test]
    fn ac2_no_toml_is_noop_on_items_that_match_heuristic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // No `.cfdb/concepts/` directory. Heuristic applies:
        // `domain-trading` → "trading". Stored value already matches.
        let mut store = store_with_items(
            tmp.path(),
            &[("crate::A", "A", "domain-trading", "trading")],
        );
        let ks = Keyspace::new("test");
        let report = store.enrich_bounded_context(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.facts_scanned, 1);
        assert_eq!(report.attrs_written, 0, "no-op on fresh-extract values");
    }

    // ------------------------------------------------------------------
    // AC-3: modified TOML → mismatched crates patched, matching ones untouched.
    // ------------------------------------------------------------------

    #[test]
    fn ac3_only_mismatched_crates_patched() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/trading.toml",
            "name = \"trading-v2\"\ncrates = [\"domain-trading\"]\n",
        );
        // Two items in `domain-trading` (out of sync) + one in
        // `ports-trading` (where stored value "trading" already matches the
        // heuristic output for that crate; no override for ports-trading, so
        // it stays unchanged).
        let mut store = store_with_items(
            tmp.path(),
            &[
                ("crate::A", "A", "domain-trading", "trading"),
                ("crate::B", "B", "domain-trading", "trading"),
                ("crate::C", "C", "ports-trading", "trading"),
            ],
        );
        let ks = Keyspace::new("test");
        let report = store.enrich_bounded_context(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.attrs_written, 2, "two domain-trading items patched");
        assert_eq!(get_bounded_context(&store, &ks, "crate::A"), "trading-v2");
        assert_eq!(get_bounded_context(&store, &ks, "crate::B"), "trading-v2");
        assert_eq!(get_bounded_context(&store, &ks, "crate::C"), "trading");
    }

    // ------------------------------------------------------------------
    // AC-7: two runs on identical workspace + TOML produce byte-identical
    // canonical dumps.
    // ------------------------------------------------------------------

    #[test]
    fn ac7_two_runs_produce_identical_canonical_dumps() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/trading.toml",
            "name = \"custom-trading\"\ncrates = [\"domain-trading\"]\n",
        );

        fn build(root: &Path) -> PetgraphStore {
            let mut store = PetgraphStore::new().with_workspace(root);
            let ks = Keyspace::new("test");
            for (q, n, c, ctx) in [
                ("crate::A", "A", "domain-trading", "trading"),
                ("crate::B", "B", "ports-trading", "trading"),
            ] {
                let mut props = Props::new();
                props.insert("qname".into(), PropValue::Str(q.into()));
                props.insert("name".into(), PropValue::Str(n.into()));
                props.insert("crate".into(), PropValue::Str(c.into()));
                props.insert("bounded_context".into(), PropValue::Str(ctx.into()));
                store
                    .ingest_nodes(
                        &ks,
                        vec![Node {
                            id: format!("item:{q}"),
                            label: Label::new(Label::ITEM),
                            props,
                        }],
                    )
                    .expect("ingest");
            }
            store
        }

        let ks = Keyspace::new("test");
        let mut s1 = build(tmp.path());
        s1.enrich_bounded_context(&ks).expect("run 1");
        let mut s2 = build(tmp.path());
        s2.enrich_bounded_context(&ks).expect("run 2");
        let d1 = s1.canonical_dump(&ks).expect("dump 1");
        let d2 = s2.canonical_dump(&ks).expect("dump 2");
        assert_eq!(d1, d2, "two runs must be byte-identical (AC-7)");
    }

    // ------------------------------------------------------------------
    // Degraded paths
    // ------------------------------------------------------------------

    #[test]
    fn malformed_toml_returns_ran_false_with_warning() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/broken.toml",
            "this is = not [valid toml",
        );
        let mut store = store_with_items(
            tmp.path(),
            &[("crate::A", "A", "domain-trading", "trading")],
        );
        let ks = Keyspace::new("test");
        let report = store.enrich_bounded_context(&ks).expect("pass");

        assert!(!report.ran, "TOML load error → ran=false");
        assert_eq!(report.attrs_written, 0);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("concepts") || w.contains("toml")),
            "warning must name the load failure: {:?}",
            report.warnings
        );
    }

    #[test]
    fn empty_keyspace_returns_ran_true_with_zero_counters() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut store = PetgraphStore::new().with_workspace(tmp.path());
        let ks = Keyspace::new("test");
        store.ingest_nodes(&ks, Vec::new()).expect("ingest empty");
        let report = store.enrich_bounded_context(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.facts_scanned, 0);
        assert_eq!(report.attrs_written, 0);
    }

    #[test]
    fn unknown_keyspace_returns_err() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut store = PetgraphStore::new().with_workspace(tmp.path());
        let ks = Keyspace::new("never");
        let err = store
            .enrich_bounded_context(&ks)
            .expect_err("unknown keyspace must err");
        assert!(format!("{err:?}").contains("UnknownKeyspace"));
    }

    #[test]
    fn no_workspace_root_returns_degraded_report() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        let mut props = Props::new();
        props.insert("qname".into(), PropValue::Str("crate::A".into()));
        props.insert("name".into(), PropValue::Str("A".into()));
        props.insert("crate".into(), PropValue::Str("domain-x".into()));
        props.insert("bounded_context".into(), PropValue::Str("x".into()));
        store
            .ingest_nodes(
                &ks,
                vec![Node {
                    id: "item:crate::A".into(),
                    label: Label::new(Label::ITEM),
                    props,
                }],
            )
            .expect("ingest");
        let report = store.enrich_bounded_context(&ks).expect("pass");
        assert!(!report.ran);
        assert!(report.warnings.iter().any(|w| w.contains("workspace_root")));
    }
}
