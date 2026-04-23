//! `TYPE_OF` edge emission tests (#220, RFC-037 §3.4).
//!
//! After #220, every `:Field` or `:Param` whose rendered type string
//! resolves to an emitted `:Item` qname in the same workspace produces
//! one `TYPE_OF` edge from the source `:Field` / `:Param` to the
//! referenced `:Item`. Resolution is post-walk, so a field whose type
//! names a struct declared later in the same file still emits the
//! edge. Resolution shares the RETURNS resolver's exact-match +
//! unique-last-segment-fallback policy (see
//! `lib.rs::resolve_deferred_type_of`).
//!
//! **Scope limits pinned here (RFC-037 §6 non-goals):**
//!
//! - Generic-stripping: `render_type_string` renders `Vec<T>` as
//!   `"Vec"`, `Option<T>` as `"Option"` — so wrapper-wrapped same-crate
//!   types do not emit `TYPE_OF` today. The
//!   `field_wrapped_same_crate_type_emits_no_type_of_edge` test pins
//!   this; when a future `render_type_inner` unwrapper lands, the test
//!   flips and the limitation comment in
//!   `lib.rs::resolve_deferred_type_of` must be deleted in the same PR.
//! - Variant-level `TYPE_OF` is out of scope for this slice — variant
//!   payloads are walked into separate `:Field` nodes that queue their
//!   own `TYPE_OF` entries, which is sufficient for current queries.
//!
//! The fixture harness mirrors `returns_emission.rs`: a real cargo
//! workspace is written into a tempdir and run through the full
//! `extract_workspace` pipeline, so assertions reflect the observable
//! extractor output end-to-end.

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

/// Find the `:Item` node id for the given `kind` + `name` prop pair.
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

/// Find the `:Field` node id for a given `(parent_qname, field_name)`
/// pair. Panics if zero or more than one match.
fn field_id_by<'a>(nodes: &'a [cfdb_core::Node], parent_qname: &str, field_name: &str) -> &'a str {
    let matches: Vec<&cfdb_core::Node> = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::FIELD
                && matches!(n.props.get("parent_qname"), Some(PropValue::Str(p)) if p == parent_qname)
                && matches!(n.props.get("name"), Some(PropValue::Str(s)) if s == field_name)
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one :Field{{parent_qname:{parent_qname}, name:{field_name}}}, found {}",
        matches.len()
    );
    matches[0].id.as_str()
}

/// Find the `:Param` node id for a given `(parent_qname, index)` pair.
/// Panics if zero or more than one match.
fn param_id_by<'a>(nodes: &'a [cfdb_core::Node], parent_qname: &str, index: i64) -> &'a str {
    let matches: Vec<&cfdb_core::Node> = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::PARAM
                && matches!(n.props.get("parent_qname"), Some(PropValue::Str(p)) if p == parent_qname)
                && matches!(n.props.get("index"), Some(PropValue::Int(i)) if *i == index)
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one :Param{{parent_qname:{parent_qname}, index:{index}}}, found {}",
        matches.len()
    );
    matches[0].id.as_str()
}

fn type_of_edges(edges: &[cfdb_core::Edge]) -> Vec<&cfdb_core::Edge> {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::TYPE_OF)
        .collect()
}

#[test]
fn field_referencing_same_crate_struct_emits_type_of_edge() {
    // Baseline: `struct A;` declared first, then `struct Foo { bar: A }`.
    // Same-walk resolution would already work here — this pins the
    // simplest case.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofbase",
        r#"pub struct A;

pub struct Foo {
    pub bar: A,
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let a_id = item_id_by_name(&nodes, "struct", "A");
    let bar_id = field_id_by(&nodes, "typeofbase::Foo", "bar");

    let type_ofs = type_of_edges(&edges);
    let matching: Vec<&&cfdb_core::Edge> = type_ofs
        .iter()
        .filter(|e| e.src == bar_id && e.dst == a_id)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one TYPE_OF edge from :Field(Foo.bar) to :Item(A), got {} (all TYPE_OF edges: {:?})",
        matching.len(),
        type_ofs
            .iter()
            .map(|e| (e.src.as_str(), e.dst.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn field_referencing_forward_declared_struct_still_emits_type_of_edge() {
    // `struct Bar { a: A }` declared BEFORE `pub struct A;`. Same-walk
    // forward-lookup would miss this; the post-walk pass catches it.
    // This is the slice's core invariant.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeoffwd",
        r#"pub struct Bar {
    pub a: A,
}

pub struct A;
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let a_id = item_id_by_name(&nodes, "struct", "A");
    let field_id = field_id_by(&nodes, "typeoffwd::Bar", "a");

    let type_ofs = type_of_edges(&edges);
    let matching: Vec<&&cfdb_core::Edge> = type_ofs
        .iter()
        .filter(|e| e.src == field_id && e.dst == a_id)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "post-walk resolution must emit TYPE_OF edge for forward-declared field type"
    );
}

#[test]
fn field_with_primitive_type_emits_no_type_of_edge() {
    // `struct Foo(i32)` — `i32` is not a walked `:Item` qname, so no
    // TYPE_OF edge should be emitted. The deferred entry is silently
    // dropped by `resolve_deferred_type_of`.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofprim",
        r#"pub struct Foo(pub i32);
"#,
    );
    let (_nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let type_ofs = type_of_edges(&edges);
    assert!(
        type_ofs.is_empty(),
        "no TYPE_OF edge should be emitted for primitive-typed fields (got {} edges: {:?})",
        type_ofs.len(),
        type_ofs
            .iter()
            .map(|e| (e.src.as_str(), e.dst.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn field_wrapped_same_crate_type_emits_no_type_of_edge() {
    // `struct V(Vec<MyType>)`. `render_type_string` strips generic
    // arguments — rendered string is `"Vec"`, which never matches a
    // workspace `:Item` qname. This test pins the documented
    // limitation (RFC-037 §6 non-goals) so the day someone adds a
    // `render_type_inner` unwrapper, this test FLIPS — at which point
    // the limitation comment in `lib.rs::resolve_deferred_type_of`
    // must be deleted in the same PR.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofwrapped",
        r#"pub struct MyType;

pub struct V(pub Vec<MyType>);
"#,
    );
    let (_nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let type_ofs = type_of_edges(&edges);
    assert!(
        type_ofs.is_empty(),
        "documented limitation: render_type_string strips generics, so Vec<MyType> currently \
         does not emit TYPE_OF (got {} edges; if this fires, the unwrapper has been added — \
         update the limitation comment in lib.rs::resolve_deferred_type_of)",
        type_ofs.len()
    );
}

#[test]
fn param_referencing_same_crate_struct_emits_type_of_edge() {
    // `fn foo(a: A) {}` — the :Param(0) for `a` must emit a TYPE_OF
    // edge to :Item(A).
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofparam",
        r#"pub struct A;

pub fn foo(a: A) {
    let _ = a;
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let a_id = item_id_by_name(&nodes, "struct", "A");
    let param_id = param_id_by(&nodes, "typeofparam::foo", 0);

    let type_ofs = type_of_edges(&edges);
    let matching: Vec<&&cfdb_core::Edge> = type_ofs
        .iter()
        .filter(|e| e.src == param_id && e.dst == a_id)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one TYPE_OF edge from :Param(foo#0) to :Item(A), got {} (all TYPE_OF edges: {:?})",
        matching.len(),
        type_ofs
            .iter()
            .map(|e| (e.src.as_str(), e.dst.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn type_of_edge_emission_is_deterministic_across_two_extractions() {
    // G1 byte-stability — two extractions of the same fixture produce
    // an identical sorted TYPE_OF edge set.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofdet",
        r#"pub struct A;
pub struct B;
pub struct Holder {
    pub a: A,
    pub b: B,
}
pub fn use_a(x: A) { let _ = x; }
pub struct Forward {
    pub late: Late,
}
pub struct Late;
"#,
    );

    fn type_ofs_sorted(edges: &[cfdb_core::Edge]) -> Vec<(String, String)> {
        let mut rows: Vec<_> = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::TYPE_OF)
            .map(|e| (e.src.clone(), e.dst.clone()))
            .collect();
        rows.sort();
        rows
    }

    let (_n1, edges_a) = extract_workspace(fixture.path()).expect("extract 1");
    let (_n2, edges_b) = extract_workspace(fixture.path()).expect("extract 2");
    let a = type_ofs_sorted(&edges_a);
    let b = type_ofs_sorted(&edges_b);
    assert_eq!(
        a, b,
        "two extractions must produce byte-identical TYPE_OF edges"
    );
    // Sanity: the fixture has
    //   - Holder.a : A   → TYPE_OF
    //   - Holder.b : B   → TYPE_OF
    //   - use_a param 0  : A → TYPE_OF
    //   - Forward.late   : Late → TYPE_OF (forward-declared)
    // = 4 TYPE_OF edges
    assert_eq!(
        a.len(),
        4,
        "fixture should produce 4 TYPE_OF edges (Holder.a→A, Holder.b→B, \
         use_a#0→A, Forward.late→Late), got {}",
        a.len()
    );
}
