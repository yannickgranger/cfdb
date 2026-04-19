//! Integration test for `extract_entry_points` (slice 4, Issue #86).
//!
//! Validates attribute-based heuristic detection of clap CLI commands
//! (via `#[derive(Parser)]` / `#[derive(Subcommand)]`) and MCP tools
//! (via `#[tool]`). Emits `:EntryPoint` nodes + `EXPOSES` edges to
//! handler items.
//!
//! The fixture deliberately mixes three items:
//!   - `Cli` struct  — has `#[derive(Parser)]` → expect `cli_command`
//!   - `Command` enum — has `#[derive(Subcommand)]` → expect `cli_command`
//!   - `echo` fn     — has `#[tool]` → expect `mcp_tool`
//!   - `unrelated_fn` — no clap / mcp attr → must NOT be detected

use std::fs;
use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_hir_extractor::{build_hir_database, extract_entry_points};
use tempfile::tempdir;

fn write(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("fixture mkdir -p");
    }
    fs::write(p, contents).expect("fixture write");
}

#[test]
fn attribute_based_entry_point_detection_covers_cli_and_mcp() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();

    write(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["epfixture"]
"#,
    );
    // We do NOT pull in actual `clap` or `rmcp` crates — the fixture
    // only needs the attributes textually; the HIR extractor's scan
    // is attribute-syntactic, not trait-resolution-based.
    write(
        root,
        "epfixture/Cargo.toml",
        r#"[package]
name = "epfixture"
version = "0.0.1"
edition = "2021"

[dependencies]
"#,
    );
    write(
        root,
        "epfixture/src/lib.rs",
        r#"
// Stand-ins for clap derives — the scan is textual, so a bare
// Parser/Subcommand identifier is sufficient. Real consumers use
// clap::Parser; the heuristic matches both.
pub trait Parser {}
pub trait Subcommand {}

#[derive(Parser)]
pub struct Cli {
    pub arg: String,
}

#[derive(Subcommand)]
pub enum Command {
    Run,
    Stop,
}

// Stand-in for an MCP-style tool attribute. The heuristic matches
// the last path segment `tool` regardless of the crate.
#[tool]
pub fn echo(input: &str) -> String {
    input.to_string()
}

pub fn unrelated_fn() {}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on epfixture");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs).expect("extract_entry_points on epfixture");

    // Filter :EntryPoint nodes.
    let entry_points: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ENTRY_POINT)
        .collect();

    // Expect exactly 3: Cli (cli_command), Command (cli_command), echo (mcp_tool).
    assert_eq!(
        entry_points.len(),
        3,
        "expected 3 :EntryPoint nodes (Cli, Command, echo); got {}: {:?}",
        entry_points.len(),
        entry_points.iter().map(|n| &n.id).collect::<Vec<_>>(),
    );

    // Kind distribution: 2 cli_command, 1 mcp_tool.
    let cli_count = entry_points
        .iter()
        .filter(|n| n.props.get("kind").and_then(PropValue::as_str) == Some("cli_command"))
        .count();
    let mcp_count = entry_points
        .iter()
        .filter(|n| n.props.get("kind").and_then(PropValue::as_str) == Some("mcp_tool"))
        .count();
    assert_eq!(cli_count, 2, "expected 2 cli_command :EntryPoint");
    assert_eq!(mcp_count, 1, "expected 1 mcp_tool :EntryPoint");

    // Each :EntryPoint must have an EXPOSES edge to the handler Item.
    let exposes: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::EXPOSES)
        .collect();
    assert_eq!(
        exposes.len(),
        3,
        "expected 3 EXPOSES edges (one per :EntryPoint); got {}",
        exposes.len()
    );

    // Spot-check: the `echo` mcp_tool's EXPOSES edge points to
    // item:epfixture::echo.
    let expected_handler = item_node_id("epfixture::echo");
    assert!(
        exposes.iter().any(|e| e.dst == expected_handler),
        "expected EXPOSES edge → {}; saw: {:?}",
        expected_handler,
        exposes.iter().map(|e| &e.dst).collect::<Vec<_>>(),
    );

    // unrelated_fn must NOT appear anywhere.
    assert!(
        !entry_points.iter().any(|n| n
            .props
            .get("handler_qname")
            .and_then(PropValue::as_str)
            .is_some_and(|q| q.ends_with("unrelated_fn"))),
        "unrelated_fn must not be detected as an entry point",
    );
}
