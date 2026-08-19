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
fn field_wrapped_same_crate_type_emits_type_of_edge() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofwrapped",
        r#"pub struct MyType;

pub struct V(pub Vec<MyType>);
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let my_type_id = item_id_by_name(&nodes, "struct", "MyType");
    let type_ofs = type_of_edges(&edges);
    let to_my_type: Vec<&&cfdb_core::Edge> =
        type_ofs.iter().filter(|e| e.dst == my_type_id).collect();
    assert_eq!(
        to_my_type.len(),
        1,
        "expected exactly one TYPE_OF edge from V's tuple field to MyType via render_type_inner unwrap, got {} (all TYPE_OF edges: {:?})",
        to_my_type.len(),
        type_ofs.len()
    );
}

#[test]
fn param_with_wrapped_same_crate_type_emits_type_of_edge() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofparamwrapped",
        r#"pub struct Foo;

pub fn handle(x: Option<Foo>) {
    let _ = x;
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let foo_id = item_id_by_name(&nodes, "struct", "Foo");
    let param_id = param_id_by(&nodes, "typeofparamwrapped::handle", 0);
    let type_ofs = type_of_edges(&edges);
    let matching: Vec<&&cfdb_core::Edge> = type_ofs
        .iter()
        .filter(|e| e.src == param_id && e.dst == foo_id)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected one TYPE_OF edge from :Param(handle#0) to :Item(Foo) via Option unwrap, got {}",
        matching.len()
    );
}

#[test]
fn field_with_nested_wrappers_at_depth_three_emits_type_of_edge() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofnested",
        r#"use std::sync::Arc;

pub struct Foo;

pub struct V {
    pub inner: Vec<Option<Arc<Foo>>>,
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let foo_id = item_id_by_name(&nodes, "struct", "Foo");
    let field_id = field_id_by(&nodes, "typeofnested::V", "inner");
    let type_ofs = type_of_edges(&edges);
    let matching: Vec<&&cfdb_core::Edge> = type_ofs
        .iter()
        .filter(|e| e.src == field_id && e.dst == foo_id)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected one TYPE_OF edge from :Field(V.inner) to :Item(Foo) via depth-3 unwrap, got {}",
        matching.len()
    );
}

#[test]
fn field_with_result_wrapper_emits_one_type_of_edge_per_arm() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typeofresult",
        r#"pub struct Ok;
pub struct Err;

pub struct R {
    pub slot: Result<Ok, Err>,
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");
    let ok_id = item_id_by_name(&nodes, "struct", "Ok");
    let err_id = item_id_by_name(&nodes, "struct", "Err");
    let field_id = field_id_by(&nodes, "typeofresult::R", "slot");
    let type_ofs = type_of_edges(&edges);
    let to_ok: Vec<&&cfdb_core::Edge> = type_ofs
        .iter()
        .filter(|e| e.src == field_id && e.dst == ok_id)
        .collect();
    let to_err: Vec<&&cfdb_core::Edge> = type_ofs
        .iter()
        .filter(|e| e.src == field_id && e.dst == err_id)
        .collect();
    assert_eq!(to_ok.len(), 1, "expected one TYPE_OF to Ok arm");
    assert_eq!(to_err.len(), 1, "expected one TYPE_OF to Err arm");
}

#[test]
fn param_referencing_same_crate_struct_emits_type_of_edge() {
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
    assert_eq!(
        a.len(),
        4,
        "fixture should produce 4 TYPE_OF edges (Holder.a→A, Holder.b→B, \
         use_a#0→A, Forward.late→Late), got {}",
        a.len()
    );
}
