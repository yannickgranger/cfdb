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

fn syn_item_ids(root: &Path) -> BTreeSet<String> {
    let (nodes, _edges) =
        cfdb_extractor::extract_workspace(root).expect("syn extract_workspace on parityfixture");
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .map(|n| n.id.clone())
        .collect()
}

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

    let shared: BTreeSet<&String> = syn_ids.intersection(&hir_ids).collect();

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
