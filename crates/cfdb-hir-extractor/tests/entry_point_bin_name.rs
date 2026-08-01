//! Regression (#517 → RFC-054 54-C, #558): HIR edge endpoints must
//! land on the syn extractor's `:Item` ids for bin-target crates.
//!
//! ## History
//!
//! #517 fixed the QNAME half: rust-analyzer names a `[[bin]]` target's
//! crate by the bin TARGET name while syn keys qnames off the PACKAGE
//! name; the HIR emitters were re-keyed onto the package name. RFC-054
//! 54-B (#557) then gave syn's bin-target items a `#bin:{target}`
//! IDENTITY suffix, opening a documented window in which HIR endpoints
//! (still undiscriminated) dangled against syn's discriminated ids —
//! pinned by the previous revision of this file, with tripwires that
//! fired the moment 54-C landed. 54-C (#558) teaches the HIR emitters
//! the target discriminator via the CargoWorkspace root-file
//! correlation, and this file is flipped back to FULL JOIN assertions
//! ("extended, not replaced" per RFC-054 §7 54-C).
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
//! ## Fixtures
//!
//! 1. `binpkg` — the #517 shape: bin-only member `bin-dash-pkg` whose
//!    bin target is named `toolbin` (≠ package). Exercises the
//!    package-name qname + `#bin:toolbin` identity.
//! 2. `samename` — the council rust-systems Finding 1 shape: a package
//!    with BOTH a `[lib]` and a `[[bin]]` named exactly like the
//!    package. `CrateOrigin` and `display_name` are byte-identical
//!    between the two crate inputs; only the root-file correlation can
//!    separate them. Lib items must stay bare, bin items must carry
//!    `#bin:samename`.
//!
//! Also asserted here (#561): `:CallSite` / `:EntryPoint` `file` props
//! from the HIR producer are WORKSPACE-RELATIVE, matching the schema
//! contract every other producer honors (#527 / #540).

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
    let (db, vfs, _pm, targets) =
        build_hir_database(root, false).expect("build_hir_database on bin fixture");
    let (ep_nodes, edges) = extract_entry_points(&db, &vfs, root, &targets)
        .expect("extract_entry_points on bin fixture");

    let exposes: BTreeSet<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::EXPOSES)
        .map(|e| e.dst.clone())
        .collect();

    // The cli_command handler is `Cli`, defined in the bin target's
    // main.rs. syn keys its :Item off the PACKAGE name (`bin-dash-pkg` →
    // `bin_dash_pkg`, #517) and RFC-054 54-B gives it the `#bin:toolbin`
    // identity suffix. 54-C: the HIR EXPOSES dst carries the SAME
    // discriminated id — full join, no window.
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
    // #517's guarantee still holds: package-name keyed, never
    // target-name keyed.
    assert!(
        !exposes.iter().any(|d| d.contains("toolbin::")),
        "#517 regression — HIR keyed a dst off the bin TARGET name: {exposes:?}"
    );
    // 54-C join: the discriminated dst is emitted…
    assert!(
        exposes.contains(&discriminated),
        "54-C: HIR EXPOSES dst must be the discriminated syn id \
         `{discriminated}` — emitted dsts: {exposes:?}"
    );
    // …and NOTHING dangles: every EXPOSES dst is a real syn :Item id.
    for dst in &exposes {
        assert!(
            syn_ids.contains(dst),
            "dangling HIR EXPOSES dst `{dst}` — 54-C closed the 54-B \
             window; no exceptions remain. syn ids: {syn_ids:?}"
        );
    }
    // The :EntryPoint id embeds the discriminated identity (RFC-054
    // 54-C via cfdb_core::qname::entrypoint_node_id) — two same-qname
    // commands in sibling bins must stay distinct rows.
    assert!(
        ep_nodes
            .iter()
            .any(|n| n.id == "entrypoint:cli_command:bin_dash_pkg::Cli#bin:toolbin"),
        "expected the identity-embedding :EntryPoint id — got: {:?}",
        ep_nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>()
    );
    // #561: :EntryPoint.file is workspace-relative.
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

    // `run` calls `helper`, both free fns in the bin target's main.rs.
    // #517 keys their HIR qnames off the PACKAGE name; RFC-054 54-B gives
    // syn's bin items the `#bin:toolbin` suffix; 54-C makes the HIR
    // endpoints carry the same discriminated ids — full join.
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
    // #517's guarantee holds: package-name keyed, never target-name keyed.
    assert!(
        !call_endpoints.iter().any(|d| d.contains("toolbin::")),
        "#517 regression — HIR keyed an endpoint off the bin TARGET name: {call_endpoints:?}"
    );
    // 54-C join: the discriminated endpoints are emitted…
    assert!(
        call_endpoints.contains(&caller_disc) && call_endpoints.contains(&callee_disc),
        "54-C: HIR CALLS endpoints must be the discriminated syn ids \
         `{caller_disc}` / `{callee_disc}` — endpoints: {call_endpoints:?}"
    );
    // …and NOTHING dangles: every endpoint is a real syn :Item id.
    for ep in &call_endpoints {
        assert!(
            syn_ids.contains(ep),
            "dangling HIR CALLS endpoint `{ep}` — 54-C closed the 54-B \
             window; no exceptions remain. syn ids: {syn_ids:?}"
        );
    }
    // The :CallSite id derives from the DISCRIMINATED caller identity
    // (cfdb_core::qname::callsite_node_id) — syntactically identical
    // calls in sibling bins stay distinct (#542).
    assert!(
        cs_nodes
            .iter()
            .any(|n| n.id == "callsite:bin_dash_pkg::run#bin:toolbin:bin_dash_pkg::helper:0"),
        "expected the identity-embedding :CallSite id — got: {:?}",
        cs_nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>()
    );
    // #561: :CallSite / :Argument file props are workspace-relative.
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

/// The council rust-systems Finding 1 shape: `[lib]` + `[[bin]]` named
/// exactly like the package. `origin` and `display_name` are
/// byte-identical between the two crate inputs — only the root-file
/// correlation separates them. Lib items stay bare; bin items carry
/// `#bin:samename`; every HIR endpoint joins a syn :Item.
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
    // The bin calls its own lib by crate name AND a bin-local fn — the
    // CALLS edges must discriminate per ENDPOINT (bin caller, lib
    // callee).
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
    // proc_macros=true per the #558 Tests: prescription — exercises the
    // two-step load's sysroot-discovery path (RustLibSource::Discover)
    // alongside the correlation map. On runners without the sysroot
    // proc-macro server the loader falls back gracefully (RFC-043 §3.3
    // case 1); the discrimination assertions hold either way.
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
    // The bin-side caller discriminates even though origin/display_name
    // cannot tell the two `samename` crates apart…
    assert!(
        call_endpoints.contains(&bin_caller),
        "same-named-bin caller must carry `#bin:samename` \
         — endpoints: {call_endpoints:?}"
    );
    // …while the lib-side callee stays byte-stable bare.
    assert!(
        call_endpoints.contains(&lib_callee),
        "lib callee must stay bare (byte-stable lib ids) \
         — endpoints: {call_endpoints:?}"
    );
    // Nothing dangles.
    for ep in &call_endpoints {
        assert!(
            syn_ids.contains(ep),
            "dangling endpoint `{ep}` on the same-named fixture. \
             syn ids: {syn_ids:?}"
        );
    }
}
