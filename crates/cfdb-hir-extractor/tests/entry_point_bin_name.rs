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

fn write_fixture(root: &Path) {
    write(
        root,
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"binpkg\", \"clap\"]\n",
    );
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
    let (db, vfs, _pm, targets) =
        build_hir_database(root, false).expect("build_hir_database on bin fixture");
    let (ep_nodes, edges) = extract_entry_points(&db, &vfs, root, &targets)
        .expect("extract_entry_points on bin fixture");

    let exposes: BTreeSet<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::EXPOSES)
        .map(|e| e.dst.clone())
        .collect();

    let discriminated = item_node_id("bin_dash_pkg::Cli#bin:toolbin");

    assert!(
        syn_ids.contains(&discriminated),
        "syn must emit the discriminated bin-target :Item `{discriminated}`. \
         syn :Item ids: {syn_ids:?}"
    );
    assert!(
        !exposes.is_empty(),
        "HIR emitted no EXPOSES edge for the cli_command"
    );
    assert!(
        !exposes.iter().any(|d| d.contains("toolbin::")),
        "#517 regression — HIR keyed a dst off the bin TARGET name: {exposes:?}"
    );
    assert!(
        exposes.contains(&discriminated),
        "54-C: HIR EXPOSES dst must be the discriminated syn id \
         `{discriminated}` — emitted dsts: {exposes:?}"
    );
    for dst in &exposes {
        assert!(
            syn_ids.contains(dst),
            "dangling HIR EXPOSES dst `{dst}` — 54-C closed the 54-B \
             window; no exceptions remain. syn ids: {syn_ids:?}"
        );
    }
    assert!(
        ep_nodes
            .iter()
            .any(|n| n.id == "entrypoint:cli_command:bin_dash_pkg::Cli#bin:toolbin"),
        "expected the identity-embedding :EntryPoint id — got: {:?}",
        ep_nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>()
    );
    for n in ep_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ENTRY_POINT)
    {
        let file = n
            .props
            .get("file")
            .and_then(cfdb_core::fact::PropValue::as_str)
            .expect(":EntryPoint carries a file prop");
        assert_eq!(
            file, "binpkg/src/main.rs",
            "#561: :EntryPoint.file must be workspace-relative"
        );
    }
}

#[test]
fn call_site_endpoints_resolve_to_syn_items_when_bin_name_differs_from_package() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write_fixture(root);

    let syn_ids = syn_item_ids(root);
    let (db, vfs, _pm, targets) =
        build_hir_database(root, false).expect("build_hir_database on bin fixture");
    let (cs_nodes, edges) =
        extract_call_sites(&db, &vfs, root, &targets).expect("extract_call_sites on bin fixture");

    let call_endpoints: BTreeSet<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .flat_map(|e| [e.src.clone(), e.dst.clone()])
        .filter(|id| id.starts_with("item:"))
        .collect();

    let caller_disc = item_node_id("bin_dash_pkg::run#bin:toolbin");
    let callee_disc = item_node_id("bin_dash_pkg::helper#bin:toolbin");

    assert!(
        syn_ids.contains(&caller_disc) && syn_ids.contains(&callee_disc),
        "syn must emit discriminated bin-target :Items for run/helper. \
         syn :Item ids: {syn_ids:?}"
    );
    assert!(
        !call_endpoints.is_empty(),
        "HIR emitted no CALLS endpoints — fixture or extractor broken. syn :Item ids: {syn_ids:?}"
    );
    assert!(
        !call_endpoints.iter().any(|d| d.contains("toolbin::")),
        "#517 regression — HIR keyed an endpoint off the bin TARGET name: {call_endpoints:?}"
    );
    assert!(
        call_endpoints.contains(&caller_disc) && call_endpoints.contains(&callee_disc),
        "54-C: HIR CALLS endpoints must be the discriminated syn ids \
         `{caller_disc}` / `{callee_disc}` — endpoints: {call_endpoints:?}"
    );
    for ep in &call_endpoints {
        assert!(
            syn_ids.contains(ep),
            "dangling HIR CALLS endpoint `{ep}` — 54-C closed the 54-B \
             window; no exceptions remain. syn ids: {syn_ids:?}"
        );
    }
    assert!(
        cs_nodes
            .iter()
            .any(|n| n.id == "callsite:bin_dash_pkg::run#bin:toolbin:bin_dash_pkg::helper:0"),
        "expected the identity-embedding :CallSite id — got: {:?}",
        cs_nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>()
    );
    for n in &cs_nodes {
        if let Some(file) = n
            .props
            .get("file")
            .and_then(cfdb_core::fact::PropValue::as_str)
        {
            assert_eq!(
                file, "binpkg/src/main.rs",
                "#561: `{}` file prop must be workspace-relative",
                n.id
            );
        }
    }
}

#[test]
fn same_named_bin_and_lib_discriminate_by_root_file() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"samename\"]\n",
    );
    write(
        root,
        "samename/Cargo.toml",
        r#"[package]
name = "samename"
version = "0.0.1"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "samename"
path = "src/main.rs"
"#,
    );
    write(
        root,
        "samename/src/lib.rs",
        "pub fn lib_helper() -> i32 {\n    41\n}\n",
    );
    write(
        root,
        "samename/src/main.rs",
        r#"fn bin_local() -> i32 {
    samename::lib_helper() + 1
}

fn main() {
    let _ = bin_local();
}
"#,
    );

    let syn_ids = syn_item_ids(root);
    let (db, vfs, _pm, targets) =
        build_hir_database(root, true).expect("build_hir_database on samename fixture");
    let (_cs_nodes, edges) =
        extract_call_sites(&db, &vfs, root, &targets).expect("extract_call_sites on samename");

    let call_endpoints: BTreeSet<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .flat_map(|e| [e.src.clone(), e.dst.clone()])
        .filter(|id| id.starts_with("item:"))
        .collect();

    let bin_caller = item_node_id("samename::bin_local#bin:samename");
    let lib_callee = item_node_id("samename::lib_helper");

    assert!(
        syn_ids.contains(&bin_caller) && syn_ids.contains(&lib_callee),
        "syn must emit the discriminated bin item AND the bare lib item. \
         syn ids: {syn_ids:?}"
    );
    assert!(
        call_endpoints.contains(&bin_caller),
        "same-named-bin caller must carry `#bin:samename` \
         — endpoints: {call_endpoints:?}"
    );
    assert!(
        call_endpoints.contains(&lib_callee),
        "lib callee must stay bare (byte-stable lib ids) \
         — endpoints: {call_endpoints:?}"
    );
    for ep in &call_endpoints {
        assert!(
            syn_ids.contains(ep),
            "dangling endpoint `{ep}` on the same-named fixture. \
             syn ids: {syn_ids:?}"
        );
    }
}
