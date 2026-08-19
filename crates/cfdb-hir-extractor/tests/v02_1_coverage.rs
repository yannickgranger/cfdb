use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_hir_extractor::{build_hir_database, extract_entry_points};

const EXPECTED: &[(&str, &str)] = &[
    ("mcp_tool", "mcp_fx::echo"),
    ("mcp_tool", "mcp_fx::ping"),
    ("cli_command", "cli_fx::RunCmd"),
    ("cli_command", "cli_fx::Verb"),
    ("http_route", "http_fx::list_users"),
    ("http_route", "http_fx::show_user"),
    ("http_route", "http_fx::health"),
    ("cron_job", "cron_fx::register_minute_job"),
    ("cron_job", "cron_fx::install_hourly"),
    ("websocket", "ws_fx::chat_handler"),
    ("websocket", "ws_fx::mount_inline"),
];

const FORBIDDEN: &[&str] = &[
    "mcp_fx::unrelated_helper",
    "cli_fx::UnrelatedConfig",
    "http_fx::unrelated_handler",
    "cron_fx::unrelated_setup",
    "ws_fx::unrelated_ws_helper",
];

fn fixture_root() -> PathBuf {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .join("tests")
        .join("fixtures")
        .join("entry_points")
}

fn kind_of(n: &Node) -> Option<&str> {
    n.props.get("kind").and_then(PropValue::as_str)
}

fn handler_qname(n: &Node) -> Option<&str> {
    n.props.get("handler_qname").and_then(PropValue::as_str)
}

fn threshold(expected: usize) -> usize {
    (95 * expected).div_ceil(100)
}

#[test]
fn v02_1_coverage_gate_meets_95_percent_recall_per_kind() {
    let root = fixture_root();
    assert!(
        root.join("Cargo.toml").exists(),
        "fixture workspace root missing Cargo.toml at {}",
        root.display()
    );

    let (db, vfs, _pm_client, targets) = build_hir_database(&root, false)
        .unwrap_or_else(|e| panic!("build_hir_database({}) failed: {e}", root.display()));
    let (nodes, _edges) = extract_entry_points(&db, &vfs, &root, &targets)
        .unwrap_or_else(|e| panic!("extract_entry_points on fixture failed: {e}"));

    let emitted: BTreeMap<(String, String), ()> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ENTRY_POINT)
        .filter_map(|n| {
            let k = kind_of(n)?.to_string();
            let q = handler_qname(n)?.to_string();
            Some(((k, q), ()))
        })
        .collect();

    let mut by_kind: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (kind, qname) in EXPECTED {
        by_kind.entry(kind).or_default().push(qname);
    }

    let mut failures: Vec<String> = Vec::new();
    for (kind, expected_qnames) in &by_kind {
        let expected_count = expected_qnames.len();
        let required = threshold(expected_count);
        let mut missing: Vec<&str> = Vec::new();
        let mut found = 0usize;
        for q in expected_qnames {
            if emitted.contains_key(&((*kind).to_string(), (*q).to_string())) {
                found += 1;
            } else {
                missing.push(q);
            }
        }
        if found < required {
            failures.push(format!(
                "kind={kind}: found {found}/{expected_count} (need ≥{required} for 95% recall); \
                 missing: {missing:?}",
            ));
        }
    }

    for forbidden in FORBIDDEN {
        let leaked: Vec<&str> = emitted
            .keys()
            .filter(|(_, q)| q == forbidden)
            .map(|(k, _)| k.as_str())
            .collect();
        if !leaked.is_empty() {
            failures.push(format!(
                "false positive: `{forbidden}` was emitted as an EntryPoint under kind(s) \
                 {leaked:?} but is a control row (must not fire)",
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "v0.2-1 coverage gate FAILED:\n  - {}\n\nFull emitted set ({} rows):\n{}",
        failures.join("\n  - "),
        emitted.len(),
        emitted
            .keys()
            .map(|(k, q)| format!("    {k:<12} {q}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

#[test]
fn v02_1_expected_total_matches_documented_ground_truth() {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (kind, _) in EXPECTED {
        *counts.entry(kind).or_default() += 1;
    }
    assert_eq!(counts.get("mcp_tool").copied(), Some(2));
    assert_eq!(counts.get("cli_command").copied(), Some(2));
    assert_eq!(counts.get("http_route").copied(), Some(3));
    assert_eq!(counts.get("cron_job").copied(), Some(2));
    assert_eq!(counts.get("websocket").copied(), Some(2));
    assert_eq!(EXPECTED.len(), 11);
}
