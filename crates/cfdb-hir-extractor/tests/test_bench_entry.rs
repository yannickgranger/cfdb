//! Integration test for RFC-042 042-A — `:EntryPoint{kind=test|bench}`
//! emission via the persistent fixture `test_bench_fx` (sibling to the
//! existing `cli_fx` / `mcp_fx` / `http_fx` / `cron_fx` / `ws_fx`
//! fixtures under `tests/fixtures/entry_points/`).
//!
//! Coverage per RFC-042 §3.4:
//!   - `tests/integration.rs::test_plain` — `#[test]` attribute
//!   - `tests/integration.rs::test_tokio` — `#[tokio::test]` (last segment `test`)
//!   - `tests/integration.rs::test_helper` — file-location fallback under `tests/`
//!   - `tests/integration.rs::helper_with_tool` — `#[tool]` precedence over `tests/`
//!   - `tests/bdd.rs::step_given` / `step_when` / `step_then` — cucumber BDD attrs
//!   - `benches/bench.rs::bench_one` — `#[bench]` attribute
//!   - `benches/bench.rs::bench_helper` — file-location fallback under `benches/`
//!   - `src/lib.rs::library_fn` — CONTROL row (must NOT emit)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_hir_extractor::{build_hir_database, extract_entry_points};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("entry_points")
}

fn kind_of(n: &Node) -> Option<&str> {
    n.props.get("kind").and_then(PropValue::as_str)
}

fn name_of(n: &Node) -> Option<&str> {
    n.props.get("name").and_then(PropValue::as_str)
}

fn file_of(n: &Node) -> Option<&str> {
    n.props.get("file").and_then(PropValue::as_str)
}

/// Index emitted `:EntryPoint`s under the `test_bench_fx` fixture by
/// `(kind, name)`. Filtering by file path (rather than handler_qname)
/// keeps assertions stable across whatever qname-formula adjustments
/// ra_ap_load_cargo / `build_item_qname` produce for integration-test
/// and bench targets.
fn entry_points_in_fixture() -> BTreeMap<(String, String), String> {
    let root = fixture_root();
    let (db, vfs, _pm_client, targets) = build_hir_database(&root, false)
        .unwrap_or_else(|e| panic!("build_hir_database({}) failed: {e}", root.display()));
    let (nodes, _edges) = extract_entry_points(&db, &vfs, &root, &targets)
        .unwrap_or_else(|e| panic!("extract_entry_points on fixture failed: {e}"));

    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ENTRY_POINT)
        .filter_map(|n| {
            let kind = kind_of(n)?.to_string();
            let name = name_of(n)?.to_string();
            let file = file_of(n)?.to_string();
            if !file.contains("test_bench_fx") {
                return None;
            }
            Some(((kind, name), file))
        })
        .collect()
}

#[test]
fn attribute_test_attrs_emit_kind_test() {
    let eps = entry_points_in_fixture();

    // Bare `#[test]`
    assert!(
        eps.contains_key(&("test".into(), "test_plain".into())),
        "expected :EntryPoint{{kind:\"test\", name:\"test_plain\"}}; got:\n{eps:#?}",
    );

    // `#[tokio::test]` — last segment `test`
    assert!(
        eps.contains_key(&("test".into(), "test_tokio".into())),
        "expected :EntryPoint{{kind:\"test\", name:\"test_tokio\"}}; got:\n{eps:#?}",
    );
}

#[test]
fn cucumber_step_attrs_emit_kind_test() {
    let eps = entry_points_in_fixture();
    for step in &["step_given", "step_when", "step_then"] {
        assert!(
            eps.contains_key(&("test".into(), (*step).into())),
            "expected :EntryPoint{{kind:\"test\", name:\"{step}\"}}; got:\n{eps:#?}",
        );
    }
}

#[test]
fn bench_attr_emits_kind_bench() {
    let eps = entry_points_in_fixture();
    assert!(
        eps.contains_key(&("bench".into(), "bench_one".into())),
        "expected :EntryPoint{{kind:\"bench\", name:\"bench_one\"}}; got:\n{eps:#?}",
    );
}

#[test]
fn file_location_under_tests_dir_emits_kind_test() {
    let eps = entry_points_in_fixture();
    assert!(
        eps.contains_key(&("test".into(), "test_helper".into())),
        "expected :EntryPoint{{kind:\"test\", name:\"test_helper\"}} (file-location \
         fallback for unattributed fn under tests/); got:\n{eps:#?}",
    );
}

#[test]
fn file_location_under_benches_dir_emits_kind_bench() {
    let eps = entry_points_in_fixture();
    assert!(
        eps.contains_key(&("bench".into(), "bench_helper".into())),
        "expected :EntryPoint{{kind:\"bench\", name:\"bench_helper\"}} (file-location \
         fallback for unattributed fn under benches/); got:\n{eps:#?}",
    );
}

#[test]
fn tool_attr_takes_precedence_over_tests_dir() {
    let eps = entry_points_in_fixture();
    // helper_with_tool lives in tests/integration.rs but carries
    // `#[tool]`. Per RFC-042 §3.1 item 1, `#[tool]` wins: kind=mcp_tool.
    assert!(
        eps.contains_key(&("mcp_tool".into(), "helper_with_tool".into())),
        "expected :EntryPoint{{kind:\"mcp_tool\", name:\"helper_with_tool\"}} \
         (#[tool] precedence over tests/ file-location); got:\n{eps:#?}",
    );
    // The same fn must NOT also emit kind=test.
    assert!(
        !eps.contains_key(&("test".into(), "helper_with_tool".into())),
        "helper_with_tool must NOT emit kind=test — would violate the \
         no-duplicate-emission invariant (RFC-042 §4); got:\n{eps:#?}",
    );
}

#[test]
fn library_fn_in_src_is_not_emitted() {
    let eps = entry_points_in_fixture();
    // library_fn lives in src/lib.rs — no attr, file-location detector
    // doesn't fire for src/. CONTROL row.
    assert!(
        !eps.contains_key(&("test".into(), "library_fn".into())),
        "library_fn must NOT emit kind=test (lives in src/, no attr); got:\n{eps:#?}",
    );
    assert!(
        !eps.contains_key(&("bench".into(), "library_fn".into())),
        "library_fn must NOT emit kind=bench (lives in src/, no attr); got:\n{eps:#?}",
    );
}
