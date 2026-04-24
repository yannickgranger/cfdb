//! `RETURNS` edge emission tests (#216, RFC-037 §3.2; #239 closeout).
//!
//! After #216, every fn/method whose `syn::ReturnType` resolves to a
//! `:Item` qname emitted in the same workspace produces one `RETURNS`
//! edge from the fn's `:Item` to the return-type's `:Item`. Resolution
//! is post-walk, so a fn whose return type names an item declared
//! later in the same file (or in a different file walked later) still
//! emits the edge.
//!
//! Third-tier wrapper unwrap (#239, RFC-037 §6 closeout): when the
//! outer rendered return-type string misses both the exact-match and
//! unique-last-segment tiers, the resolver falls back to
//! `render_type_inner` on the stored `syn::Type` with a depth-3
//! budget. `fn v() -> Vec<Foo>` now emits a RETURNS edge to `Foo`;
//! `fn r() -> Result<Ok, Err>` emits two. The closed wrapper list
//! (`Vec`, `Option`, `Arc`, `Rc`, `Box`, `Result`, `Pin`, `Cell`,
//! `RefCell`) is in `type_render::WRAPPER_TYPES`.

use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;
use tempfile::tempdir;

fn write_fixture_file(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(
        p.parent()
            .expect("fixture path always has a parent directory"),
    )
    .expect("fixture mkdir -p");
    std::fs::write(p, contents).expect("fixture write");
}

fn write_cargo_workspace(root: &Path, crate_name: &str, lib_src: &str) {
    write_fixture_file(
        root,
        "Cargo.toml",
        &format!(
            r#"[workspace]
resolver = "2"
members = ["{crate_name}"]
"#
        ),
    );
    write_fixture_file(
        root,
        &format!("{crate_name}/Cargo.toml"),
        &format!(
            r#"[package]
name = "{crate_name}"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#
        ),
    );
    write_fixture_file(root, &format!("{crate_name}/src/lib.rs"), lib_src);
}

/// Find the `:Item` node id for the given fn / method `name` prop.
/// Panics with a helpful message if zero or more than one match.
fn item_id_by_name<'a>(nodes: &'a [cfdb_core::Node], kind: &str, name: &str) -> &'a str {
    let matches: Vec<&cfdb_core::Node> = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && matches!(n.props.get("kind"), Some(PropValue::Str(k)) if k == kind)
                && matches!(n.props.get("name"), Some(PropValue::Str(s)) if s == name)
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one :Item{{kind:{kind}, name:{name}}}, found {}",
        matches.len()
    );
    matches[0].id.as_str()
}

fn returns_edges(edges: &[cfdb_core::Edge]) -> Vec<&cfdb_core::Edge> {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::RETURNS)
        .collect()
}

#[test]
fn fn_returning_same_crate_struct_emits_returns_edge() {
    // Bar declared first, then `foo() -> Bar`. Same-walk resolution
    // would already work here; this is the baseline.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsbase",
        r#"pub struct Bar;

pub fn foo() -> Bar {
    Bar
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let foo_id = item_id_by_name(&nodes, "fn", "foo");
    let bar_id = item_id_by_name(&nodes, "struct", "Bar");

    let returns: Vec<&cfdb_core::Edge> = returns_edges(&edges);
    let to_bar: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == foo_id && e.dst == bar_id)
        .collect();
    assert_eq!(
        to_bar.len(),
        1,
        "expected exactly one RETURNS edge from foo to Bar, got {} (all RETURNS edges: {:?})",
        to_bar.len(),
        returns
            .iter()
            .map(|e| (e.src.as_str(), e.dst.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn fn_returning_forward_declared_type_still_emits_returns_edge() {
    // `use_foo() -> Foo` declared BEFORE `pub struct Foo {}`. Same-walk
    // forward-lookup would miss this; the post-walk pass catches it.
    // This is the slice's core invariant.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsfwd",
        r#"pub fn use_foo() -> Foo {
    Foo
}

pub struct Foo;
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let use_foo_id = item_id_by_name(&nodes, "fn", "use_foo");
    let foo_id = item_id_by_name(&nodes, "struct", "Foo");

    let returns = returns_edges(&edges);
    let to_foo: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == use_foo_id && e.dst == foo_id)
        .collect();
    assert_eq!(
        to_foo.len(),
        1,
        "post-walk resolution must emit RETURNS edge for forward-declared return type"
    );
}

#[test]
fn fn_returning_unknown_type_emits_no_returns_edge() {
    // `baz() -> CrossCrateType` — the type is not declared anywhere
    // in the walked workspace, so there is nothing to resolve to.
    // The deferred entry must be silently dropped.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsunknown",
        r#"pub fn baz() -> CrossCrateType {
    panic!("not defined here")
}
"#,
    );
    let (_nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let returns = returns_edges(&edges);
    assert!(
        returns.is_empty(),
        "no RETURNS edge should be emitted when the return type does not resolve to a walked :Item (got {} edges)",
        returns.len()
    );
}

#[test]
fn fn_returning_wrapped_same_crate_type_emits_returns_edge() {
    // `v() -> Vec<MyType>`. The outer `render_type_string` renders
    // `"Vec"`, which does not match any workspace `:Item` qname. The
    // third-tier `render_type_inner` unwrap (#239) then inspects the
    // stored `syn::Type`, matches `Vec` in `WRAPPER_TYPES`, and
    // yields the inner candidate `"MyType"` — which resolves to the
    // walked struct and emits one RETURNS edge.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnswrapped",
        r#"pub struct MyType;

pub fn v() -> Vec<MyType> {
    Vec::new()
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let v_id = item_id_by_name(&nodes, "fn", "v");
    let my_type_id = item_id_by_name(&nodes, "struct", "MyType");
    let returns = returns_edges(&edges);
    let to_my_type: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == v_id && e.dst == my_type_id)
        .collect();
    assert_eq!(
        to_my_type.len(),
        1,
        "expected exactly one RETURNS edge from v to MyType via render_type_inner unwrap, got {} (all RETURNS edges: {:?})",
        to_my_type.len(),
        returns.len()
    );
}

#[test]
fn returns_edge_emission_is_deterministic_across_two_extractions() {
    // G1 byte-stability — two extractions of the same fixture produce
    // an identical sorted RETURNS edge set.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsdet",
        r#"pub struct A;
pub struct B;
pub fn ra() -> A { A }
pub fn rb() -> B { B }
pub fn forward() -> Late { Late }
pub struct Late;
"#,
    );

    fn returns_sorted(edges: &[cfdb_core::Edge]) -> Vec<(String, String)> {
        let mut rows: Vec<_> = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::RETURNS)
            .map(|e| (e.src.clone(), e.dst.clone()))
            .collect();
        rows.sort();
        rows
    }

    let (_n1, edges_a) = extract_workspace(fixture.path()).expect("extract 1");
    let (_n2, edges_b) = extract_workspace(fixture.path()).expect("extract 2");
    let a = returns_sorted(&edges_a);
    let b = returns_sorted(&edges_b);
    assert_eq!(
        a, b,
        "two extractions must produce byte-identical RETURNS edges"
    );
    // Sanity: the fixture has 3 fns each returning a same-crate item,
    // so we expect exactly 3 RETURNS edges in the deterministic set.
    assert_eq!(
        a.len(),
        3,
        "fixture should produce 3 RETURNS edges (ra→A, rb→B, forward→Late), got {}",
        a.len()
    );
}

#[test]
fn method_returning_same_crate_struct_emits_returns_edge() {
    // Methods (impl items) take a different emission path
    // (`visit_impl_item_fn` bypasses `emit_item_with_flags`); make
    // sure the deferred-returns push fires from there too.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsmethod",
        r#"pub struct Out;

pub struct Holder;

impl Holder {
    pub fn make(&self) -> Out { Out }
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let make_id = item_id_by_name(&nodes, "method", "make");
    let out_id = item_id_by_name(&nodes, "struct", "Out");

    let returns = returns_edges(&edges);
    let make_to_out: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == make_id && e.dst == out_id)
        .collect();
    assert_eq!(
        make_to_out.len(),
        1,
        "method `make` returning Out must produce exactly one RETURNS edge"
    );
}

#[test]
fn fn_with_no_explicit_return_type_emits_no_returns_edge() {
    // `fn noop()` has `syn::ReturnType::Default` — no deferred entry,
    // no RETURNS edge.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsnone",
        r#"pub struct T;
pub fn noop() {}
"#,
    );
    let (_nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let returns = returns_edges(&edges);
    assert!(
        returns.is_empty(),
        "fn with no explicit return type must not emit RETURNS edge (got {})",
        returns.len()
    );
}

#[test]
fn fn_returning_result_emits_one_returns_edge_per_arm() {
    // `fn r() -> Result<Ok, Err>` where both `Ok` and `Err` are walked
    // in the same workspace. `render_type_string` renders `"Result"`
    // (no item match); third-tier `render_type_inner` at depth 3
    // yields both inner candidates `"Ok"` and `"Err"`. The resolver
    // must emit one RETURNS edge per resolvable arm — two edges total.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsresult",
        r#"pub struct Ok;
pub struct Err;

pub fn r() -> Result<Ok, Err> {
    Result::Ok(Ok)
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let r_id = item_id_by_name(&nodes, "fn", "r");
    let ok_id = item_id_by_name(&nodes, "struct", "Ok");
    let err_id = item_id_by_name(&nodes, "struct", "Err");
    let returns = returns_edges(&edges);
    let to_ok: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == r_id && e.dst == ok_id)
        .collect();
    let to_err: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == r_id && e.dst == err_id)
        .collect();
    assert_eq!(
        to_ok.len(),
        1,
        "expected one RETURNS edge from r to Ok arm, got {}",
        to_ok.len()
    );
    assert_eq!(
        to_err.len(),
        1,
        "expected one RETURNS edge from r to Err arm, got {}",
        to_err.len()
    );
}

#[test]
fn fn_returning_nested_wrappers_at_depth_three_emits_one_returns_edge() {
    // `fn n() -> Vec<Option<Arc<Foo>>>` where `Foo` is the leaf.
    // Outer render `"Vec"` misses; third-tier unwrap at depth 3
    // recurses Vec → Option → Arc → Foo and yields `"Foo"` among
    // candidates. Exactly one RETURNS edge to Foo.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsnested",
        r#"use std::sync::Arc;

pub struct Foo;

pub fn n() -> Vec<Option<Arc<Foo>>> {
    Vec::new()
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let n_id = item_id_by_name(&nodes, "fn", "n");
    let foo_id = item_id_by_name(&nodes, "struct", "Foo");
    let returns = returns_edges(&edges);
    let to_foo: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == n_id && e.dst == foo_id)
        .collect();
    assert_eq!(
        to_foo.len(),
        1,
        "expected exactly one RETURNS edge from n to Foo via depth-3 unwrap, got {}",
        to_foo.len()
    );
}

#[test]
fn fn_returning_user_defined_wrapper_emits_no_returns_edge() {
    // `MyBox<Foo>` — `MyBox` is not in the closed wrapper list
    // (`type_render::WRAPPER_TYPES`). `render_type_inner` refuses to
    // unwrap; the third tier also misses. No RETURNS edge.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsuserwrap",
        r#"pub struct Foo;
pub struct MyBox<T>(pub T);

pub fn u() -> MyBox<Foo> {
    MyBox(Foo)
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let u_id = item_id_by_name(&nodes, "fn", "u");
    let foo_id = item_id_by_name(&nodes, "struct", "Foo");
    let returns = returns_edges(&edges);
    let to_foo: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == u_id && e.dst == foo_id)
        .collect();
    assert_eq!(
        to_foo.len(),
        0,
        "user-defined wrapper `MyBox<Foo>` must not trigger unwrap (got {} edges)",
        to_foo.len()
    );
}

#[test]
fn fn_returning_qualified_std_vec_emits_returns_edge_via_last_segment() {
    // Ambiguity D: `std::vec::Vec<Foo>` renders outer as
    // `"std::vec::Vec"` (misses). Third-tier `render_type_inner`
    // matches by the last segment `"Vec"` — both `Vec<Foo>` and
    // `std::vec::Vec<Foo>` unwrap the same way. Emits one RETURNS
    // edge to `Foo`.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "returnsqualified",
        r#"pub struct Foo;

pub fn q() -> std::vec::Vec<Foo> {
    std::vec::Vec::new()
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let q_id = item_id_by_name(&nodes, "fn", "q");
    let foo_id = item_id_by_name(&nodes, "struct", "Foo");
    let returns = returns_edges(&edges);
    let to_foo: Vec<&&cfdb_core::Edge> = returns
        .iter()
        .filter(|e| e.src == q_id && e.dst == foo_id)
        .collect();
    assert_eq!(
        to_foo.len(),
        1,
        "qualified `std::vec::Vec<Foo>` must unwrap via last-segment to Foo, got {}",
        to_foo.len()
    );
}
