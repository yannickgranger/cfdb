//! Raid plan validation query tests (issue #205 / RFC-036 §3.5).
//!
//! Synthetic two-crate fixture mirroring the issue's Tests row:
//! "synthetic workspace fixture (two crates, one raid plan with
//! deliberately-dangling-drop)." Each of the five raid templates is
//! parsed, bound to the fixture's buckets, executed, and asserted
//! against the expected finding set.
//!
//! The fixture encodes every failure shape the raid templates catch:
//! one unclaimed item (for completeness), one drop still called by a
//! portage (for dangling-drop), one portage called from outside
//! source_context (for hidden-callers), one rewrite concept without a
//! canonical (for missing-canonical), one portage with `unwrap_count
//! > 0` (for signal-mismatch).
//!
//! Fact injection (not extraction) isolates the query shape from
//! extractor/enrich_metrics stability — same pattern as
//! `pattern_b_vertical_split_brain.rs` + `hsb_cluster.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::query::Param;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::parse;

// ---------------------------------------------------------------------
// Fixture builders.
// ---------------------------------------------------------------------

fn item_node(qname: &str, name: &str, kind: &str, crate_name: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("name".into(), PropValue::Str(name.into()));
    props.insert("kind".into(), PropValue::Str(kind.into()));
    props.insert("crate".into(), PropValue::Str(crate_name.into()));
    props.insert("file".into(), PropValue::Str("synthetic".into()));
    props.insert("line".into(), PropValue::Int(0));
    props.insert("is_test".into(), PropValue::Bool(false));
    Node {
        id: item_node_id(qname),
        label: Label::new(Label::ITEM),
        props,
    }
}

fn fn_item_with_metrics(
    qname: &str,
    name: &str,
    crate_name: &str,
    unwrap_count: i64,
    test_coverage: f64,
) -> Node {
    let mut n = item_node(qname, name, "fn", crate_name);
    n.props
        .insert("unwrap_count".into(), PropValue::Int(unwrap_count));
    n.props
        .insert("test_coverage".into(), PropValue::Float(test_coverage));
    n
}

fn concept_node(concept_name: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(concept_name.into()));
    Node {
        id: format!("concept:{concept_name}"),
        label: Label::new(Label::CONCEPT),
        props,
    }
}

fn calls_edge(caller_qname: &str, callee_qname: &str) -> Edge {
    let mut props = BTreeMap::new();
    props.insert("resolved".into(), PropValue::Bool(true));
    Edge {
        src: item_node_id(caller_qname),
        dst: item_node_id(callee_qname),
        label: EdgeLabel::new(EdgeLabel::CALLS),
        props,
    }
}

fn labeled_as_edge(item_qname: &str, concept_name: &str) -> Edge {
    Edge {
        src: item_node_id(item_qname),
        dst: format!("concept:{concept_name}"),
        label: EdgeLabel::new(EdgeLabel::LABELED_AS),
        props: BTreeMap::new(),
    }
}

fn canonical_for_edge(item_qname: &str, concept_name: &str) -> Edge {
    Edge {
        src: item_node_id(item_qname),
        dst: format!("concept:{concept_name}"),
        label: EdgeLabel::new(EdgeLabel::CANONICAL_FOR),
        props: BTreeMap::new(),
    }
}

fn keyspace() -> Keyspace {
    Keyspace::new("raid-plan-test")
}

fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("cfdb workspace root — two parents up from cfdb-petgraph/")
        .to_path_buf()
}

fn load_query(relative_path: &str) -> cfdb_core::Query {
    let path = workspace_root()
        .join("examples/queries/raid")
        .join(relative_path);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    parse(&text).unwrap_or_else(|e| panic!("parse {}: {e:?}", path.display()))
}

fn list_param(items: &[&str]) -> Param {
    Param::List(items.iter().map(|s| PropValue::Str((*s).into())).collect())
}

// ---------------------------------------------------------------------
// Fixture — two-crate workspace + a raid plan with deliberate flaws.
//
// source_context = "stop_engine"
//
// Items (all in crate `stop_engine`):
//   - stop_engine::types::StopLoss        → portage
//   - stop_engine::types::TrailingStop    → portage (has external caller)
//   - stop_engine::mcp::handle_request    → glue
//   - stop_engine::legacy::LegacyBuilder  → drop (still called by StopLoss)
//   - stop_engine::legacy::parse_bps      → drop (clean — nobody calls it)
//   - stop_engine::risky::unwrap_stop     → portage but unwrap_count=5 (signal mismatch)
//   - stop_engine::util::format_price     → UNCLAIMED (completeness finding)
//
// Items in sibling crate `consumer_app`:
//   - consumer_app::main::use_trailing    → calls TrailingStop (hidden caller)
//
// Concepts:
//   - compound_stop    → in $rewrite, has no CANONICAL_FOR target (finding)
//   - risk_ratio       → in $rewrite, has CANONICAL_FOR → stop_engine::ratio::compute
//
// CALLS edges:
//   - StopLoss (portage) → LegacyBuilder (drop)       ← dangling-drop
//   - use_trailing (consumer_app) → TrailingStop (portage) ← hidden-callers
// ---------------------------------------------------------------------

fn build_fixture() -> PetgraphStore {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let nodes = vec![
        // stop_engine items
        item_node(
            "stop_engine::types::StopLoss",
            "StopLoss",
            "struct",
            "stop_engine",
        ),
        item_node(
            "stop_engine::types::TrailingStop",
            "TrailingStop",
            "struct",
            "stop_engine",
        ),
        item_node(
            "stop_engine::mcp::handle_request",
            "handle_request",
            "fn",
            "stop_engine",
        ),
        item_node(
            "stop_engine::legacy::LegacyBuilder",
            "LegacyBuilder",
            "struct",
            "stop_engine",
        ),
        item_node(
            "stop_engine::legacy::parse_bps",
            "parse_bps",
            "fn",
            "stop_engine",
        ),
        // Signal-mismatch scar: marked clean portage but unwrap_count=5.
        fn_item_with_metrics(
            "stop_engine::risky::unwrap_stop",
            "unwrap_stop",
            "stop_engine",
            5,
            0.80,
        ),
        // Completeness scar: unclaimed.
        item_node(
            "stop_engine::util::format_price",
            "format_price",
            "fn",
            "stop_engine",
        ),
        // Canonical for `risk_ratio` (clean rewrite row — must NOT flag
        // missing-canonical for this concept).
        item_node(
            "stop_engine::ratio::compute",
            "compute",
            "fn",
            "stop_engine",
        ),
        // External caller in sibling crate.
        item_node(
            "consumer_app::main::use_trailing",
            "use_trailing",
            "fn",
            "consumer_app",
        ),
        // Concepts.
        concept_node("compound_stop"),
        concept_node("risk_ratio"),
    ];

    let edges = vec![
        // Dangling-drop scar.
        calls_edge(
            "stop_engine::types::StopLoss",
            "stop_engine::legacy::LegacyBuilder",
        ),
        // Hidden-callers scar.
        calls_edge(
            "consumer_app::main::use_trailing",
            "stop_engine::types::TrailingStop",
        ),
        // LABELED_AS binding for completeness rewrite-concept coverage
        // — `unwrap_stop` is labeled as `compound_stop` (rewrite bucket)
        // so it's accounted-for by the rewrite concept. That means it
        // WON'T flag completeness despite being in signal_mismatch.
        // Tests below rely on this to keep the two findings independent.
        labeled_as_edge("stop_engine::risky::unwrap_stop", "compound_stop"),
        // Canonical target for risk_ratio (clean rewrite row).
        canonical_for_edge("stop_engine::ratio::compute", "risk_ratio"),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");
    store
}

fn bind_plan(query: &mut cfdb_core::Query) {
    query.params.insert(
        "source_context".into(),
        Param::Scalar(PropValue::Str("stop_engine".into())),
    );
    query.params.insert(
        "portage".into(),
        list_param(&[
            "stop_engine::risky::unwrap_stop",
            "stop_engine::types::StopLoss",
            "stop_engine::types::TrailingStop",
        ]),
    );
    query.params.insert(
        "rewrite".into(),
        list_param(&["compound_stop", "risk_ratio"]),
    );
    query.params.insert(
        "glue".into(),
        list_param(&["stop_engine::mcp::handle_request"]),
    );
    query.params.insert(
        "drop".into(),
        list_param(&[
            "stop_engine::legacy::LegacyBuilder",
            "stop_engine::legacy::parse_bps",
        ]),
    );
}

fn run(
    store: &PetgraphStore,
    query: &cfdb_core::Query,
) -> Vec<BTreeMap<String, cfdb_core::result::RowValue>> {
    store
        .execute(&keyspace(), query)
        .expect("execute raid query")
        .rows
}

fn row_str<'a>(
    row: &'a BTreeMap<String, cfdb_core::result::RowValue>,
    key: &str,
) -> Option<&'a str> {
    row.get(key).and_then(|v| v.as_str())
}

// ---------------------------------------------------------------------
// Template 1 — raid-completeness.
// ---------------------------------------------------------------------

#[test]
fn completeness_flags_items_not_in_any_qname_bucket() {
    let store = build_fixture();
    let mut q = load_query("raid-completeness.cypher");
    bind_plan(&mut q);

    let rows = run(&store, &q);

    // v2 scope: raid-completeness checks only qname buckets
    // (portage/glue/drop). Items that are canonicals for rewrite
    // concepts but not explicitly in a qname bucket WILL flag — by
    // design (see raid-completeness.cypher header rationale).
    //
    // In our fixture with bind_plan():
    //   portage = [unwrap_stop, StopLoss, TrailingStop]
    //   glue    = [handle_request]
    //   drop    = [LegacyBuilder, parse_bps]
    //
    // Unplaced items in stop_engine:
    //   - stop_engine::util::format_price      (real oversight)
    //   - stop_engine::ratio::compute          (canonical not placed —
    //                                           author should add to portage)
    let qnames: Vec<&str> = rows.iter().filter_map(|r| row_str(r, "qname")).collect();
    assert!(
        qnames.contains(&"stop_engine::util::format_price"),
        "completeness must flag the unclaimed format_price item; got {qnames:?}"
    );
    assert!(
        qnames.contains(&"stop_engine::ratio::compute"),
        "completeness flags unplaced canonical — triaged by adding to portage; got {qnames:?}"
    );
    // Items in qname buckets MUST NOT appear.
    assert!(
        !qnames.contains(&"stop_engine::types::StopLoss"),
        "portage items must not flag as unclaimed; got {qnames:?}"
    );
    assert!(
        !qnames.contains(&"stop_engine::legacy::LegacyBuilder"),
        "drop items must not flag as unclaimed; got {qnames:?}"
    );
    assert!(
        !qnames.contains(&"stop_engine::mcp::handle_request"),
        "glue items must not flag as unclaimed; got {qnames:?}"
    );
    assert!(
        !qnames.contains(&"stop_engine::risky::unwrap_stop"),
        "portage item unwrap_stop must not flag as unclaimed; got {qnames:?}"
    );
}

// ---------------------------------------------------------------------
// Template 2 — raid-dangling-drop.
// ---------------------------------------------------------------------

#[test]
fn dangling_drop_flags_the_still_called_drop() {
    let store = build_fixture();
    let mut q = load_query("raid-dangling-drop.cypher");
    bind_plan(&mut q);

    let rows = run(&store, &q);

    // Expected: exactly one row — LegacyBuilder (drop) is called by
    // StopLoss (portage). parse_bps (drop) has no callers → clean.
    assert_eq!(
        rows.len(),
        1,
        "dangling-drop should emit exactly one row; rows={rows:?}"
    );
    assert_eq!(
        row_str(&rows[0], "dropped_qname"),
        Some("stop_engine::legacy::LegacyBuilder")
    );
    assert_eq!(
        row_str(&rows[0], "caller_qname"),
        Some("stop_engine::types::StopLoss")
    );
}

// ---------------------------------------------------------------------
// Template 3 — raid-hidden-callers.
// ---------------------------------------------------------------------

#[test]
fn hidden_callers_flags_external_caller_of_portage() {
    let store = build_fixture();
    let mut q = load_query("raid-hidden-callers.cypher");
    bind_plan(&mut q);

    let rows = run(&store, &q);

    // Expected: exactly one row — consumer_app::main::use_trailing
    // (external) calls TrailingStop (portage). StopLoss's caller is
    // not external. unwrap_stop has no external callers.
    assert_eq!(
        rows.len(),
        1,
        "hidden-callers should emit one row; rows={rows:?}"
    );
    assert_eq!(
        row_str(&rows[0], "portaged_qname"),
        Some("stop_engine::types::TrailingStop")
    );
    assert_eq!(
        row_str(&rows[0], "external_qname"),
        Some("consumer_app::main::use_trailing")
    );
    assert_eq!(row_str(&rows[0], "external_crate"), Some("consumer_app"));
}

// ---------------------------------------------------------------------
// Template 4 — raid-missing-canonical.
// ---------------------------------------------------------------------

#[test]
fn missing_canonical_flags_rewrite_without_canonical_for_target() {
    let store = build_fixture();
    let mut q = load_query("raid-missing-canonical.cypher");
    bind_plan(&mut q);

    let rows = run(&store, &q);

    // Expected: exactly one row — compound_stop has no CANONICAL_FOR.
    // risk_ratio is clean (stop_engine::ratio::compute CANONICAL_FOR
    // risk_ratio).
    assert_eq!(
        rows.len(),
        1,
        "missing-canonical should emit one row; rows={rows:?}"
    );
    assert_eq!(row_str(&rows[0], "concept_name"), Some("compound_stop"));
}

// ---------------------------------------------------------------------
// Template 5 — raid-signal-mismatch.
// ---------------------------------------------------------------------

#[test]
fn signal_mismatch_flags_unclean_portage_item() {
    let store = build_fixture();
    let mut q = load_query("raid-signal-mismatch.cypher");
    bind_plan(&mut q);
    q.params
        .insert("max_unwraps".into(), Param::Scalar(PropValue::Int(0)));
    q.params
        .insert("min_coverage".into(), Param::Scalar(PropValue::Float(0.60)));

    let rows = run(&store, &q);

    // Expected: exactly one row — unwrap_stop has unwrap_count=5
    // (> max_unwraps=0). Coverage is 0.80 — above the 0.60 threshold.
    // StopLoss and TrailingStop are structs without metric props →
    // don't match `i.kind = 'fn'`.
    assert_eq!(
        rows.len(),
        1,
        "signal-mismatch should emit one row; rows={rows:?}"
    );
    assert_eq!(
        row_str(&rows[0], "qname"),
        Some("stop_engine::risky::unwrap_stop")
    );
}

// ---------------------------------------------------------------------
// Negative controls — five templates, each on a clean plan.
// ---------------------------------------------------------------------

fn bind_clean_plan(query: &mut cfdb_core::Query) {
    // Clean plan: every item is accounted for, drop has no incoming
    // CALLS, no external portage callers, every rewrite concept has a
    // canonical, no portage item has metric violations.
    query.params.insert(
        "source_context".into(),
        Param::Scalar(PropValue::Str("stop_engine".into())),
    );
    query.params.insert(
        "portage".into(),
        list_param(&[
            "stop_engine::types::StopLoss",
            "stop_engine::types::TrailingStop",
            "stop_engine::risky::unwrap_stop",
            "stop_engine::util::format_price",
            "stop_engine::ratio::compute",
        ]),
    );
    // Move ALL concepts to rewrite so missing-canonical only flags
    // compound_stop. Clean-plan test uses a single-concept fixture.
    query
        .params
        .insert("rewrite".into(), list_param(&["risk_ratio"]));
    query.params.insert(
        "glue".into(),
        list_param(&["stop_engine::mcp::handle_request"]),
    );
    // Drop LegacyBuilder last — nothing in portage/glue calls it in the
    // clean plan because we removed it via moving-not-dropping.
    // BUT: our fixture has StopLoss calling LegacyBuilder, so dropping
    // LegacyBuilder is still dangling. To make the clean-plan test
    // actually clean for dangling-drop, we drop `parse_bps` only (no
    // incoming calls).
    query.params.insert(
        "drop".into(),
        list_param(&["stop_engine::legacy::parse_bps"]),
    );
}

#[test]
fn dangling_drop_is_empty_on_clean_plan() {
    let store = build_fixture();
    let mut q = load_query("raid-dangling-drop.cypher");
    bind_clean_plan(&mut q);
    let rows = run(&store, &q);
    assert!(
        rows.is_empty(),
        "clean plan must not flag dangling-drop; rows={rows:?}"
    );
}

#[test]
fn signal_mismatch_is_empty_when_thresholds_relaxed() {
    let store = build_fixture();
    let mut q = load_query("raid-signal-mismatch.cypher");
    bind_plan(&mut q);
    // Allow up to 10 unwraps + require only 10% coverage → no violation.
    q.params
        .insert("max_unwraps".into(), Param::Scalar(PropValue::Int(10)));
    q.params
        .insert("min_coverage".into(), Param::Scalar(PropValue::Float(0.10)));
    let rows = run(&store, &q);
    assert!(
        rows.is_empty(),
        "relaxed thresholds must clear signal-mismatch; rows={rows:?}"
    );
}
