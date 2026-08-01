//! Cross-extractor qname-parity fixture test (RFC-044 §3.4 sub-band 1).
//!
//! ## The invariant
//!
//! Both the syn-based `cfdb-extractor` and the HIR-based
//! `cfdb-hir-extractor` emit `:Item` node ids of the form `item:<qname>`
//! and reference those ids from cross-extractor edges
//! (`CALLS(item:caller, item:callee)`). For a syn `:Item` node and a HIR
//! `CALLS` endpoint to land on the *same* graph node, both extractors MUST
//! compute the `<qname>` component bit-identically for the same source
//! item. Any divergence produces silently dangling edges — the worst class
//! of graph corruption (passes every schema validator, makes every
//! reachability query wrong). `cfdb_core::qname` is the single canonical
//! formula owner; this test proves both extractors actually agree on a
//! real fixture, end to end.
//!
//! ## Why this is a runtime parity test, not a static scan
//!
//! Routing through `cfdb_core::qname` is *necessary* but not *sufficient*:
//! the two extractors feed DIFFERENT raw renderings into the same helpers.
//! In particular `normalize_impl_target` is the historical divergence
//! point — syn's shallow `render_type` produces `Wrapper` for an
//! `impl<T> Wrapper<T>` target while HIR's `HirDisplay` produces
//! `Wrapper<T>`; both must normalize to `Wrapper` so the method qname
//! agrees. A static "did you call the helper" scan cannot catch a
//! divergence in *what is passed to* the helper. Only running both
//! extractors on the same source and comparing the emitted qnames does.
//!
//! ## Fixture
//!
//! A single-crate synthetic cargo workspace (`parityfixture`) with:
//!   - a generic type `Wrapper<T>` with an inherent method `inner`
//!     (exercises `normalize_impl_target`: syn `Wrapper` vs HIR
//!     `Wrapper<T>` → normalized `Wrapper`);
//!   - a plain struct `Greeter` with an inherent method `greet`;
//!   - free functions `helper` and `dispatch`;
//!   - a `caller` that invokes `Wrapper::inner`, `Greeter::greet`, and
//!     `helper` so the HIR extractor emits resolved CALLS edges whose
//!     endpoints are `item:<qname>` ids for those same items.
//!
//! ## What is compared
//!
//! - **syn side** (`extract_workspace`): the `id` of every emitted
//!   `:Item` node (already `item:<qname>` via `item_node_id`).
//! - **HIR side** (`extract_call_sites`): the `item:<qname>` endpoints of
//!   every `CALLS` edge (HIR emits no `:Item` nodes of its own — see
//!   `tests/exclusion.rs` — its qnames surface as edge endpoints).
//!
//! The test asserts (a) the intersection of the two `item:` id sets is
//! NON-EMPTY (non-vacuity — a parity test over two disjoint/empty sets is
//! worthless) and (b) the generic-impl-target method `Wrapper::inner` is
//! present in BOTH, since that is the qname the normalization divergence
//! would corrupt. Bit-identity of shared ids is structural: an id is in
//! the intersection iff the exact same byte string was produced by both
//! extractors.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_hir_extractor::{build_hir_database, extract_call_sites};
use tempfile::tempdir;

fn write(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("fixture mkdir -p");
    }
    fs::write(p, contents).expect("fixture write");
}

/// Build the shared synthetic workspace under `root`. Both extractors run
/// against this same on-disk source so the comparison is genuinely
/// cross-extractor (not two views of two different inputs).
fn write_fixture(root: &Path) {
    write(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["parityfixture"]
"#,
    );
    write(
        root,
        "parityfixture/Cargo.toml",
        r#"[package]
name = "parityfixture"
version = "0.0.1"
edition = "2021"

[dependencies]
"#,
    );
    // `Wrapper<T>` is the generic-impl-target case: its inherent method
    // `inner` has impl target `Wrapper<T>` (HIR rendering) / `Wrapper`
    // (syn rendering); both MUST normalize to `Wrapper` via
    // `normalize_impl_target` so the method qname is
    // `parityfixture::Wrapper::inner` in BOTH extractors. `caller`
    // invokes it (plus a plain inherent method and a free fn) so the HIR
    // extractor emits resolved CALLS edges whose endpoints are the same
    // `item:` ids the syn extractor emits as `:Item` nodes.
    write(
        root,
        "parityfixture/src/lib.rs",
        r#"pub struct Wrapper<T> {
    pub value: T,
}

impl<T: Copy> Wrapper<T> {
    pub fn inner(&self) -> T {
        self.value
    }
}

pub struct Greeter;

impl Greeter {
    pub fn greet(&self) -> &'static str {
        "hello"
    }
}

pub fn helper() -> i32 {
    7
}

pub fn caller() -> i32 {
    let w = Wrapper { value: 1i32 };
    let _v = w.inner();
    let g = Greeter;
    let _s = g.greet();
    helper()
}
"#,
    );
}

/// Collect every `item:<qname>` id emitted by the syn extractor as a
/// `:Item` node.
fn syn_item_ids(root: &Path) -> BTreeSet<String> {
    let (nodes, _edges) =
        cfdb_extractor::extract_workspace(root).expect("syn extract_workspace on parityfixture");
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .map(|n| n.id.clone())
        .collect()
}

/// Collect every `item:<qname>` id the HIR extractor references as a
/// `CALLS` edge endpoint. The HIR extractor emits no `:Item` nodes of its
/// own (see `tests/exclusion.rs`); its qnames surface only as edge
/// endpoints, so the CALLS src/dst ids are the parity surface.
fn hir_item_ids(root: &Path) -> BTreeSet<String> {
    let (_db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on parityfixture");
    let (_nodes, edges) = extract_call_sites(&_db, &vfs, root, &targets)
        .expect("extract_call_sites on parityfixture");
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .flat_map(|e| [e.src.clone(), e.dst.clone()])
        .filter(|id| id.starts_with("item:"))
        .collect()
}

#[test]
fn syn_and_hir_emit_bit_identical_qnames_for_shared_items() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write_fixture(root);

    let syn_ids = syn_item_ids(root);
    let hir_ids = hir_item_ids(root);

    // The qnames both extractors agree on. Membership in this set is, by
    // construction, bit-level identity of the `item:<qname>` byte string:
    // an id is shared iff syn and HIR produced the exact same string.
    let shared: BTreeSet<&String> = syn_ids.intersection(&hir_ids).collect();

    // Non-vacuity guard (RFC-044 §3.4): a parity test over an empty
    // intersection compares nothing and is worthless. Both extractors must
    // have run and produced overlapping qnames.
    assert!(
        !syn_ids.is_empty(),
        "syn extractor produced zero :Item ids — fixture or extractor broken"
    );
    assert!(
        !hir_ids.is_empty(),
        "HIR extractor produced zero CALLS-endpoint item: ids — fixture or extractor broken. \
         syn ids: {syn_ids:?}"
    );
    assert!(
        !shared.is_empty(),
        "vacuous parity: syn and HIR share NO qnames.\n  syn :Item ids: {syn_ids:?}\n  \
         HIR CALLS-endpoint ids: {hir_ids:?}"
    );

    // The load-bearing case: the generic-impl-target method must agree.
    // This is the qname `normalize_impl_target` divergence would corrupt
    // (syn `Wrapper` vs HIR `Wrapper<T>`). If the two extractors disagree
    // here, one of these sets is missing it and the assert fires with the
    // diff.
    let wrapper_inner = item_node_id("parityfixture::Wrapper::inner");
    assert!(
        syn_ids.contains(&wrapper_inner),
        "syn extractor did not emit the generic-impl method id `{wrapper_inner}` \
         (normalize_impl_target divergence?). syn :Item ids: {syn_ids:?}"
    );
    assert!(
        hir_ids.contains(&wrapper_inner),
        "HIR extractor did not reference the generic-impl method id `{wrapper_inner}` \
         as a CALLS endpoint (normalize_impl_target divergence?). HIR ids: {hir_ids:?}"
    );
    assert!(
        shared.contains(&&wrapper_inner),
        "generic-impl method `{wrapper_inner}` is not in the cross-extractor \
         intersection — the extractors rendered its impl target differently \
         despite both routing through normalize_impl_target"
    );
}
