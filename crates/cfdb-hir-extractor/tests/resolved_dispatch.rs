//! Integration test — synthetic workspace with a concrete method
//! dispatch that HIR resolves via type inference. Validates the full
//! chain:
//!
//!   build_hir_database(workspace_root)
//!     → extract_call_sites(db, vfs)
//!       → :CallSite (resolver="hir", callee_resolved=true)
//!       → CALLS(item:caller, item:callee)
//!       → INVOKES_AT(item:caller, :CallSite)
//!
//! ## Fixture shape
//!
//! A single-crate workspace with one struct `Greeter` whose inherent
//! method `greet` is invoked from a free function `dispatch`. This is
//! the simplest case HIR resolves that syn cannot: syn sees
//! `g.greet()` as textual `greet` with no type info; HIR infers the
//! receiver type from `let g = Greeter` and resolves the method to
//! `Greeter::greet`.
//!
//! ## Asserts
//!
//! 1. At least one `:CallSite` emitted with `resolver="hir"` +
//!    `callee_resolved=true`
//! 2. A matching `CALLS(item:<caller>, item:<callee>)` edge with
//!    `resolved=true` — where `<caller>` is the `dispatch` fn and
//!    `<callee>` is `Greeter::greet`
//! 3. A matching `INVOKES_AT(item:<caller>, :CallSite)` edge
//! 4. The `:CallSite` node's `callee_path` prop resolves to the
//!    method-qname form (`<crate>::Greeter::greet`), NOT the textual
//!    path `greet`

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

    // Single-crate cargo workspace. `resolver = "2"` is required to
    // match cfdb's edition-2021 default resolution.
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

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on hirfixture workspace");

    let (nodes, edges) =
        extract_call_sites(&db, &vfs).expect("extract_call_sites succeeds on hirfixture");

    // (1) At least one :CallSite with HIR discriminator props.
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

    // (4) The :CallSite's callee_path resolves to Greeter::greet
    // (not the textual `greet`). At least one of the HIR :CallSite
    // nodes must point at Greeter::greet.
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

    // (2) A CALLS edge from dispatch → Greeter::greet with
    // resolved=true prop.
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

    // (3) An INVOKES_AT edge from the caller item → the :CallSite.
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

/// Q7 fix from #94 ddd-specialist review — prove HIR offers value
/// BEYOND what syn can do.
///
/// Fixture: trait `Greet` with TWO impls (`En`, `Fr`). A generic
/// function `dispatch<G: Greet>(g: &G) -> &str` calls `g.greet()`.
/// Syn sees only the textual path `greet` — it cannot determine
/// which impl's `greet` is called because the receiver type is a
/// generic parameter. HIR monomorphises the call site and resolves
/// the concrete callee when the caller specialises `G`.
///
/// This test exercises trait-dispatch resolution — the case RFC-029
/// §A1.2 line 92 specifically named as the HIR value proposition.
/// The inherent-method test above is simpler but does not prove the
/// HIR vs syn delta; this one does.
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
    // `dispatch::<En>(&En)` forces monomorphisation to En's greet.
    // Syn sees: `g.greet()` — a textual `greet` with no type context.
    // HIR resolves: En::greet (after monomorphising the callee).
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

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on traitfixture");
    let (nodes, edges) = extract_call_sites(&db, &vfs).expect("extract_call_sites on traitfixture");

    // HIR should resolve g.greet() in `dispatch<G>` to the trait
    // method `Greet::greet`. The callee_path ends in `Greet::greet`.
    // (Generic monomorphisation may surface either the trait method
    // or a specific impl depending on solver state; both are
    // acceptable — the point is that HIR resolves SOMETHING where
    // syn would see nothing.)
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

    // The CALLS edge must point to a real Greet impl or the trait
    // method — NOT to a textual `greet` with no prefix. Syn could
    // never produce this target.
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
