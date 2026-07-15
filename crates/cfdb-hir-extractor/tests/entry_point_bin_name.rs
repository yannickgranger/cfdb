//! Regression (#517): the HIR crate-name prefix must match the syn
//! extractor's PACKAGE-name convention even when a member's `[[bin]]`
//! target name differs from its `[package]` name.
//!
//! ## The defect
//!
//! rust-analyzer files a `[[bin]]` target's items under a crate whose
//! `display_name` is the **bin target** name. The syn extractor — and
//! every `:Item` qname — keys the crate segment off the **package** name
//! (`package.name`, dashes→underscores). When the two differ (e.g.
//! `[package] name = "cfdb-cli"` + `[[bin]] name = "cfdb"`), the HIR
//! emitters build `EXPOSES` / `CALLS` endpoints from the bin name
//! (`item:cfdb::…`) while the only matching `:Item` is `item:cfdb_cli::…`,
//! so every such edge dangles — the worst class of graph corruption
//! (passes every schema validator, makes reachability queries wrong).
//!
//! ## Why a cross-extractor runtime test
//!
//! The HIR extractor emits no `:Item` nodes of its own (those are the syn
//! extractor's exclusive domain — see `tests/exclusion.rs`); its qnames
//! surface only as edge endpoints. Resolution is therefore only provable
//! by running BOTH extractors on the same on-disk fixture and checking the
//! HIR edge endpoints land on syn `:Item` ids. This mirrors
//! `tests/qname_parity.rs` (which covers a lib crate, where package name
//! and target name coincide and the bug is invisible).
//!
//! ## Fixture
//!
//! A single bin-only member `bin-dash-pkg` whose bin target is named
//! `toolbin` (≠ package). Its `main.rs` holds a clap `#[derive(Parser)]`
//! command (entry-point path) and two free fns `run`/`helper` where `run`
//! calls `helper` (call-site path), so both HIR crate-name sites are
//! exercised against the one divergent crate.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_hir_extractor::{build_hir_database, extract_call_sites, extract_entry_points};
use tempfile::tempdir;

fn write(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("fixture mkdir -p");
    }
    fs::write(p, contents).expect("fixture write");
}

/// Write the divergent single-member workspace under `root`. Both
/// extractors run against this same source so the comparison is genuinely
/// cross-extractor. The package name carries a dash so the
/// `-`→`_` normalisation is exercised alongside the package-vs-target
/// divergence.
fn write_fixture(root: &Path) {
    write(
        root,
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"binpkg\", \"clap\"]\n",
    );
    // Empty stub crate named `clap` — satisfies the RFC-049 §3.1 manifest
    // gate so the clap detector runs on `binpkg` (the `cli_fx` fixture
    // idiom). Detection stays textual against the local stand-in trait.
    write(
        root,
        "clap/Cargo.toml",
        r#"[package]
name = "clap"
version = "0.0.1"
edition = "2021"
"#,
    );
    write(root, "clap/src/lib.rs", "");
    write(
        root,
        "binpkg/Cargo.toml",
        r#"[package]
name = "bin-dash-pkg"
version = "0.0.1"
edition = "2021"

[[bin]]
name = "toolbin"
path = "src/main.rs"

[dependencies]
clap = { path = "../clap" }
"#,
    );
    // Stand-in for `clap::Parser` — the cli_command scan is textual on the
    // derive attribute, so a user-defined trait of the same name fires it
    // (matching the `cli_fx` fixture idiom; the stub `clap` path dep exists
    // only to pass the RFC-049 §3.1 manifest gate). `run`
    // calls `helper` so the call-site emitter emits a resolved CALLS edge
    // whose endpoints are the same bin-crate items.
    write(
        root,
        "binpkg/src/main.rs",
        r#"pub trait Parser {}

#[derive(Parser)]
pub struct Cli {
    pub workspace: String,
}

pub fn helper() -> i32 {
    7
}

pub fn run() -> i32 {
    helper()
}

fn main() {
    let _ = run();
}
"#,
    );
}

/// Every `item:<qname>` id the syn extractor emits as a `:Item` node.
fn syn_item_ids(root: &Path) -> BTreeSet<String> {
    let (nodes, _edges) =
        cfdb_extractor::extract_workspace(root).expect("syn extract_workspace on bin fixture");
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .map(|n| n.id.clone())
        .collect()
}

#[test]
fn cli_command_exposes_resolves_to_syn_item_when_bin_name_differs_from_package() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write_fixture(root);

    let syn_ids = syn_item_ids(root);
    let (db, vfs, _pm) =
        build_hir_database(root, false).expect("build_hir_database on bin fixture");
    let (_nodes, edges) =
        extract_entry_points(&db, &vfs, root).expect("extract_entry_points on bin fixture");

    let exposes: BTreeSet<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::EXPOSES)
        .map(|e| e.dst.clone())
        .collect();

    // The cli_command handler is `Cli`, defined in the bin target's
    // main.rs. syn keys its :Item off the PACKAGE name (`bin-dash-pkg` →
    // `bin_dash_pkg`); the HIR emitter MUST agree or the edge dangles.
    let expected = item_node_id("bin_dash_pkg::Cli");

    assert!(
        syn_ids.contains(&expected),
        "fixture/syn broken — syn did not emit the package-name :Item `{expected}`. \
         syn :Item ids: {syn_ids:?}"
    );
    assert!(
        !exposes.is_empty(),
        "HIR emitted no EXPOSES edge for the cli_command"
    );

    // The defect (RED before #517): HIR keys the crate segment off the bin
    // TARGET name (`toolbin`) → `item:toolbin::Cli`, absent from the syn
    // :Item set. After the fix it uses the package name and resolves.
    assert!(
        exposes.contains(&expected),
        "HIR cli_command EXPOSES dst does not use the package-name qname `{expected}` \
         — emitted dsts: {exposes:?}"
    );

    // No EXPOSES endpoint may dangle: every dst is a real syn :Item.
    for dst in &exposes {
        assert!(
            syn_ids.contains(dst),
            "dangling HIR EXPOSES dst `{dst}` — not a syn :Item. syn :Item ids: {syn_ids:?}"
        );
    }
}

#[test]
fn call_site_endpoints_resolve_to_syn_items_when_bin_name_differs_from_package() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write_fixture(root);

    let syn_ids = syn_item_ids(root);
    let (db, vfs, _pm) = build_hir_database(root, false).expect("build_hir_database on bin fixture");
    let (_nodes, edges) = extract_call_sites(&db, &vfs).expect("extract_call_sites on bin fixture");

    let call_endpoints: BTreeSet<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .flat_map(|e| [e.src.clone(), e.dst.clone()])
        .filter(|id| id.starts_with("item:"))
        .collect();

    // `run` calls `helper`, both free fns in the bin target's main.rs. The
    // shared crate-name resolver keys their qnames off the PACKAGE name so
    // the resolved CALLS edge lands on the syn `:Item`s.
    let caller = item_node_id("bin_dash_pkg::run");
    let callee = item_node_id("bin_dash_pkg::helper");

    assert!(
        syn_ids.contains(&caller) && syn_ids.contains(&callee),
        "fixture/syn broken — expected package-name :Items for run/helper. \
         syn :Item ids: {syn_ids:?}"
    );
    assert!(
        !call_endpoints.is_empty(),
        "HIR emitted no CALLS endpoints — fixture or extractor broken. syn :Item ids: {syn_ids:?}"
    );

    // The defect (RED before #517): HIR keys off the bin TARGET name
    // (`toolbin`), so the endpoints are `item:toolbin::{run,helper}` and
    // dangle. After the fix they use the package name and resolve.
    assert!(
        call_endpoints.contains(&caller),
        "HIR CALLS caller endpoint is not the package-name qname `{caller}` \
         — endpoints: {call_endpoints:?}"
    );
    assert!(
        call_endpoints.contains(&callee),
        "HIR CALLS callee endpoint is not the package-name qname `{callee}` \
         — endpoints: {call_endpoints:?}"
    );
}
