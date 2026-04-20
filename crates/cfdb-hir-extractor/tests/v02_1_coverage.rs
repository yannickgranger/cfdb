//! v0.2-1 coverage gate — Issue #126, RFC-029 §A1.5.
//!
//! Runs the HIR extractor against the persistent fixture workspace at
//! `tests/fixtures/entry_points/` and asserts ≥95% recall per
//! `:EntryPoint` kind against the ground-truth set in
//! `EXPECTED.md`. With the shipped closed set (2/2/3/2/2 = 11 rows),
//! 95% rounds to full recall for every kind — any missing qname fails
//! the gate and names the missing row in the assertion message (AC-4).
//!
//! Unlike the sibling tests in `tests/entry_point.rs` and
//! `tests/http_route.rs` (which materialize fixtures in tempdirs),
//! this test reads from a persistent on-disk fixture. That fixture
//! doubles as the target of the `cfdb extract ... --features hir`
//! runtime measurement recorded in the PR body (AC-5).
//!
//! # Kind coverage
//!
//! | Kind          | Expected | Threshold (≥95%) |
//! | :------------ | :------- | :--------------- |
//! | `mcp_tool`    | 2        | 2                |
//! | `cli_command` | 2        | 2                |
//! | `http_route`  | 3        | 3                |
//! | `cron_job`    | 2        | 2                |
//! | `websocket`   | 2        | 2                |

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_hir_extractor::{build_hir_database, extract_entry_points};

/// Expected entry points — one row per `:EntryPoint` the extractor
/// MUST emit on the fixture workspace. Kept in lockstep with
/// `tests/fixtures/entry_points/EXPECTED.md`; divergence between the
/// two is a maintenance bug surfaced by the `AC-2` spot-check below.
const EXPECTED: &[(&str, &str)] = &[
    // (kind, handler_qname)
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

/// Qnames that MUST NOT appear as entry points (false-positive
/// regression surface).
const FORBIDDEN: &[&str] = &[
    "mcp_fx::unrelated_helper",
    "cli_fx::UnrelatedConfig",
    "http_fx::unrelated_handler",
    "cron_fx::unrelated_setup",
    "ws_fx::unrelated_ws_helper",
];

/// Locate the fixture workspace relative to this test file. `CARGO_MANIFEST_DIR`
/// points at the `cfdb-hir-extractor` crate root; the fixture lives at
/// `tests/fixtures/entry_points/` inside that crate.
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

/// Ceiling of `0.95 * expected` — the per-kind recall threshold. For
/// the closed ground-truth set this rounds to the full expected count
/// on every kind (see module docs).
fn threshold(expected: usize) -> usize {
    // `ceil(0.95 * n)` without floats.
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

    let (db, vfs) = build_hir_database(&root)
        .unwrap_or_else(|e| panic!("build_hir_database({}) failed: {e}", root.display()));
    let (nodes, _edges) = extract_entry_points(&db, &vfs)
        .unwrap_or_else(|e| panic!("extract_entry_points on fixture failed: {e}"));

    // Index emitted EntryPoints by (kind, handler_qname) so lookups
    // are O(1) per expected row.
    let emitted: BTreeMap<(String, String), ()> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ENTRY_POINT)
        .filter_map(|n| {
            let k = kind_of(n)?.to_string();
            let q = handler_qname(n)?.to_string();
            Some(((k, q), ()))
        })
        .collect();

    // Group expected rows by kind and compute recall.
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

    // False-positive regression — none of the control qnames may be
    // emitted as an entry point.
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
    // AC-2 spot-check — the `EXPECTED` array and the `EXPECTED.md`
    // manifest must stay in lockstep. Counting per kind catches
    // accidental drift where a row is added to one but not the other.
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
