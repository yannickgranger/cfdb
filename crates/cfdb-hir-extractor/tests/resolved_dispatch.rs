use std::fs;
use std::path::Path;

use cfdb_core::fact::PropValue;
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

#[test]
fn hir_resolves_inherent_method_call() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();

    write(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["hirfixture"]
"#,
    );
    write(
        root,
        "hirfixture/Cargo.toml",
        r#"[package]
name = "hirfixture"
version = "0.0.1"
edition = "2021"

[dependencies]
"#,
    );
    write(
        root,
        "hirfixture/src/lib.rs",
        r#"pub struct Greeter;

impl Greeter {
    pub fn greet(&self) -> &'static str { "hello" }
}

pub fn dispatch() -> &'static str {
    let g = Greeter;
    g.greet()
}
"#,
    );

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on hirfixture workspace");

    let (nodes, edges) = extract_call_sites(&db, &vfs, root, &targets)
        .expect("extract_call_sites succeeds on hirfixture");

    let hir_call_sites: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .filter(|n| {
            n.props.get("resolver").and_then(PropValue::as_str) == Some("hir")
                && n.props.get("callee_resolved") == Some(&PropValue::Bool(true))
        })
        .collect();
    assert!(
        !hir_call_sites.is_empty(),
        "expected ≥1 :CallSite with resolver=hir + callee_resolved=true; got {} :CallSite nodes total",
        nodes
            .iter()
            .filter(|n| n.label.as_str() == Label::CALL_SITE)
            .count(),
    );

    let greet_call_site = hir_call_sites.iter().find(|n| {
        n.props
            .get("callee_path")
            .and_then(PropValue::as_str)
            .is_some_and(|p| p.ends_with("Greeter::greet"))
    });
    assert!(
        greet_call_site.is_some(),
        "expected a :CallSite whose callee_path ends with Greeter::greet; \
         saw callee_paths: {:?}",
        hir_call_sites
            .iter()
            .filter_map(|n| n.props.get("callee_path").and_then(PropValue::as_str))
            .collect::<Vec<_>>(),
    );
    let cs = greet_call_site.unwrap();

    let expected_caller_id = item_node_id("hirfixture::dispatch");
    let expected_callee_id = item_node_id("hirfixture::Greeter::greet");
    let calls_edge = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .find(|e| e.src == expected_caller_id && e.dst == expected_callee_id);
    assert!(
        calls_edge.is_some(),
        "expected CALLS({} → {}); actual CALLS edges: {:?}",
        expected_caller_id,
        expected_callee_id,
        edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
            .map(|e| format!("{} → {}", e.src, e.dst))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        calls_edge.unwrap().props.get("resolved"),
        Some(&PropValue::Bool(true)),
        "CALLS edge must carry resolved=true prop",
    );

    let invokes_at = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::INVOKES_AT)
        .find(|e| e.src == expected_caller_id && e.dst == cs.id);
    assert!(
        invokes_at.is_some(),
        "expected INVOKES_AT({} → {}); actual INVOKES_AT edges: {:?}",
        expected_caller_id,
        cs.id,
        edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::INVOKES_AT)
            .map(|e| format!("{} → {}", e.src, e.dst))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn hir_resolves_trait_method_via_generic_receiver() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();

    write(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["traitfixture"]
"#,
    );
    write(
        root,
        "traitfixture/Cargo.toml",
        r#"[package]
name = "traitfixture"
version = "0.0.1"
edition = "2021"

[dependencies]
"#,
    );
    write(
        root,
        "traitfixture/src/lib.rs",
        r#"pub trait Greet {
    fn greet(&self) -> &'static str;
}

pub struct En;
pub struct Fr;

impl Greet for En {
    fn greet(&self) -> &'static str { "hello" }
}

impl Greet for Fr {
    fn greet(&self) -> &'static str { "bonjour" }
}

pub fn dispatch<G: Greet>(g: &G) -> &'static str {
    g.greet()
}

pub fn use_en() -> &'static str {
    dispatch(&En)
}
"#,
    );

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on traitfixture");
    let (nodes, edges) =
        extract_call_sites(&db, &vfs, root, &targets).expect("extract_call_sites on traitfixture");

    let hir_call_sites: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .filter(|n| n.props.get("resolver").and_then(PropValue::as_str) == Some("hir"))
        .collect();

    let resolved_greet = hir_call_sites.iter().find(|n| {
        n.props
            .get("callee_path")
            .and_then(PropValue::as_str)
            .is_some_and(|p| p.ends_with("::greet"))
    });
    assert!(
        resolved_greet.is_some(),
        "HIR failed to resolve trait-dispatch call `g.greet()` on `&G: Greet`. \
         This is the canonical case where HIR offers value beyond syn (RFC-029 §A1.2 \
         line 92). :CallSite callee_paths observed: {:?}",
        hir_call_sites
            .iter()
            .filter_map(|n| n.props.get("callee_path").and_then(PropValue::as_str))
            .collect::<Vec<_>>(),
    );

    let calls: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .filter(|e| e.dst.ends_with("::greet"))
        .collect();
    assert!(
        !calls.is_empty(),
        "expected at least one CALLS edge whose dst ends with `::greet`; \
         all CALLS edges: {:?}",
        edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
            .map(|e| format!("{} → {}", e.src, e.dst))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn hir_resolves_path_call_shapes() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();

    write(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["pathcallfixture"]
"#,
    );
    write(
        root,
        "pathcallfixture/Cargo.toml",
        r#"[package]
name = "pathcallfixture"
version = "0.0.1"
edition = "2021"

[dependencies]
"#,
    );
    write(
        root,
        "pathcallfixture/src/lib.rs",
        r#"pub fn helper() -> i32 { 7 }

pub struct MyType;

impl MyType {
    pub fn new() -> Self { MyType }
}

pub trait MyTrait {
    fn trait_static(x: i32) -> i32;
}

impl MyTrait for MyType {
    fn trait_static(x: i32) -> i32 { x + 1 }
}

pub fn caller() -> i32 {
    let _h = helper();
    let _m = MyType::new();
    let _t = <MyType as MyTrait>::trait_static(42);
    _h
}
"#,
    );

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on pathcallfixture");
    let (nodes, edges) = extract_call_sites(&db, &vfs, root, &targets)
        .expect("extract_call_sites on pathcallfixture");

    let hir_call_sites: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .filter(|n| n.props.get("resolver").and_then(PropValue::as_str) == Some("hir"))
        .collect();

    let caller_id = item_node_id("pathcallfixture::caller");
    let helper_id = item_node_id("pathcallfixture::helper");
    let calls_helper = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .find(|e| e.src == caller_id && e.dst == helper_id);
    assert!(
        calls_helper.is_some(),
        "expected CALLS({} → {}) for free-fn call `helper()`; this is the #387 \
         primary scope. CALLS edges: {:?}",
        caller_id,
        helper_id,
        edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
            .map(|e| format!("{} → {}", e.src, e.dst))
            .collect::<Vec<_>>(),
    );

    let new_id = item_node_id("pathcallfixture::MyType::new");
    let calls_new = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .find(|e| e.src == caller_id && e.dst == new_id);
    assert!(
        calls_new.is_some(),
        "expected CALLS({} → {}) for associated-fn call `MyType::new()`. \
         CALLS edges: {:?}",
        caller_id,
        new_id,
        edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
            .map(|e| format!("{} → {}", e.src, e.dst))
            .collect::<Vec<_>>(),
    );

    let trait_static_id = item_node_id("pathcallfixture::MyType::trait_static");
    let calls_trait_static = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .find(|e| e.src == caller_id && e.dst == trait_static_id);
    assert!(
        calls_trait_static.is_some(),
        "expected CALLS({} → {}) for qualified trait-static call \
         `<MyType as MyTrait>::trait_static(42)`. CALLS edges: {:?}",
        caller_id,
        trait_static_id,
        edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
            .map(|e| format!("{} → {}", e.src, e.dst))
            .collect::<Vec<_>>(),
    );

    let fn_kind_call_sites = hir_call_sites
        .iter()
        .filter(|n| n.props.get("kind").and_then(PropValue::as_str) == Some("fn"))
        .count();
    assert!(
        fn_kind_call_sites >= 3,
        "expected ≥3 :CallSite nodes with kind=\"fn\" (one per shape); got {}. \
         All :CallSite kinds observed: {:?}",
        fn_kind_call_sites,
        hir_call_sites
            .iter()
            .filter_map(|n| n.props.get("kind").and_then(PropValue::as_str))
            .collect::<Vec<_>>(),
    );
}
