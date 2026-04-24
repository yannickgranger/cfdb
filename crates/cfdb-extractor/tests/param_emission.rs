//! `:Param` node + `HAS_PARAM` edge emission tests (#209, RFC-036 §3.1).
//!
//! Before #209, function parameters were encoded only as a single
//! `:Item.signature` string attribute. After #209, each fn/method emits
//! one `:Param` node per argument (including the receiver for methods)
//! and one `HAS_PARAM` edge from the enclosing `:Item{kind:Fn|method}`
//! to each `:Param` node.
//!
//! These tests build synthetic fixtures in `tempdir`, run `extract_workspace`,
//! and assert on the resulting fact set.

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

fn prop_str<'a>(props: &'a std::collections::BTreeMap<String, PropValue>, key: &str) -> &'a str {
    match props.get(key).expect("prop present") {
        PropValue::Str(s) => s.as_str(),
        other => panic!("expected Str for key {key}, got {other:?}"),
    }
}

fn prop_int(props: &std::collections::BTreeMap<String, PropValue>, key: &str) -> i64 {
    match props.get(key).expect("prop present") {
        PropValue::Int(n) => *n,
        other => panic!("expected Int for key {key}, got {other:?}"),
    }
}

fn prop_bool(props: &std::collections::BTreeMap<String, PropValue>, key: &str) -> bool {
    match props.get(key).expect("prop present") {
        PropValue::Bool(b) => *b,
        other => panic!("expected Bool for key {key}, got {other:?}"),
    }
}

#[test]
fn free_fn_with_three_params_emits_three_param_nodes_and_has_param_edges() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "paramfixture",
        r#"pub fn greet(name: String, times: u32, loud: bool) {
    let _ = (name, times, loud);
}
"#,
    );

    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let params: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::PARAM)
        .collect();
    assert_eq!(
        params.len(),
        3,
        "expected 3 :Param nodes for fn(name, times, loud), got {}: {:?}",
        params.len(),
        params.iter().map(|n| &n.id).collect::<Vec<_>>()
    );

    let has_param_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::HAS_PARAM)
        .collect();
    assert_eq!(
        has_param_edges.len(),
        3,
        "expected 3 HAS_PARAM edges, got {}",
        has_param_edges.len()
    );

    let mut by_index: Vec<_> = params
        .iter()
        .map(|n| (prop_int(&n.props, "index"), prop_str(&n.props, "name")))
        .collect();
    by_index.sort_by_key(|(i, _)| *i);
    assert_eq!(by_index, vec![(0, "name"), (1, "times"), (2, "loud")]);

    for p in &params {
        assert!(
            !prop_bool(&p.props, "is_self"),
            "free-fn params are never self"
        );
        assert!(
            !prop_str(&p.props, "parent_qname").is_empty(),
            "parent_qname populated"
        );
        assert!(
            !prop_str(&p.props, "type_path").is_empty(),
            "type_path populated"
        );
        assert!(
            !prop_str(&p.props, "type_normalized").is_empty(),
            "type_normalized populated"
        );
    }
}

#[test]
fn method_with_self_and_two_params_emits_three_param_nodes() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "methodfixture",
        r#"pub struct Counter;

impl Counter {
    pub fn bump(&mut self, by: u32, label: &str) {
        let _ = (by, label);
    }
}
"#,
    );

    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");

    let params: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::PARAM)
        .collect();
    assert_eq!(
        params.len(),
        3,
        "expected 3 :Param nodes for (&mut self, by, label), got {}",
        params.len()
    );

    let self_params: Vec<_> = params
        .iter()
        .filter(|n| prop_bool(&n.props, "is_self"))
        .collect();
    assert_eq!(self_params.len(), 1, "exactly one self param");
    let self_p = self_params[0];
    assert_eq!(prop_int(&self_p.props, "index"), 0);
    let self_ty = prop_str(&self_p.props, "type_path");
    assert!(
        self_ty.contains("Self"),
        "self receiver type_path mentions Self (got {self_ty:?})"
    );

    let non_self: Vec<_> = params
        .iter()
        .filter(|n| !prop_bool(&n.props, "is_self"))
        .collect();
    assert_eq!(non_self.len(), 2);
}

#[test]
fn fn_with_zero_params_emits_no_param_nodes() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "noargfixture",
        r#"pub fn noop() {}
"#,
    );
    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");
    let params: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::PARAM)
        .collect();
    assert!(
        params.is_empty(),
        "fn with zero params must emit no :Param nodes (got {})",
        params.len()
    );
}

#[test]
fn wildcard_pattern_param_emits_param_with_empty_name() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "wildcardfixture",
        r#"pub fn ignore_first(_: i32, keep: String) {
    let _ = keep;
}
"#,
    );
    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");
    let params: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::PARAM)
        .collect();
    assert_eq!(params.len(), 2);

    let mut by_index: Vec<_> = params
        .iter()
        .map(|n| (prop_int(&n.props, "index"), prop_str(&n.props, "name")))
        .collect();
    by_index.sort_by_key(|(i, _)| *i);
    assert_eq!(by_index[0], (0, ""));
    assert_eq!(by_index[1], (1, "keep"));
}

#[test]
fn has_param_edges_point_from_item_fn_to_param_nodes() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "edgefixture",
        r#"pub fn hello(name: String) {
    let _ = name;
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let item_fn = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && matches!(n.props.get("kind"), Some(PropValue::Str(s)) if s == "fn")
                && matches!(n.props.get("name"), Some(PropValue::Str(s)) if s == "hello")
        })
        .expect(":Item{kind:fn, name:hello} present");
    let param_node = nodes
        .iter()
        .find(|n| n.label.as_str() == Label::PARAM)
        .expect(":Param node present");

    let edge = edges
        .iter()
        .find(|e| e.label.as_str() == EdgeLabel::HAS_PARAM)
        .expect("HAS_PARAM edge present");
    assert_eq!(
        edge.src, item_fn.id,
        "HAS_PARAM src must be :Item{{kind:fn}} node id"
    );
    assert_eq!(
        edge.dst, param_node.id,
        "HAS_PARAM dst must be :Param node id"
    );
}

#[test]
fn param_emission_is_deterministic_across_two_extractions() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "detfixture",
        r#"pub fn a(x: i32, y: i32) {
    let _ = (x, y);
}
pub fn b(s: String) {
    let _ = s;
}
pub struct T;
impl T {
    pub fn m(&self, z: u8) { let _ = z; }
}
"#,
    );

    fn params_sorted(nodes: &[cfdb_core::Node]) -> Vec<(String, i64, String, bool)> {
        let mut rows: Vec<_> = nodes
            .iter()
            .filter(|n| n.label.as_str() == Label::PARAM)
            .map(|n| {
                (
                    n.id.clone(),
                    prop_int(&n.props, "index"),
                    prop_str(&n.props, "name").to_string(),
                    prop_bool(&n.props, "is_self"),
                )
            })
            .collect();
        rows.sort();
        rows
    }

    let (nodes_a, _edges_a) = extract_workspace(fixture.path()).expect("extract 1");
    let (nodes_b, _edges_b) = extract_workspace(fixture.path()).expect("extract 2");
    assert_eq!(
        params_sorted(&nodes_a),
        params_sorted(&nodes_b),
        "two extractions produce byte-identical :Param rows when sorted"
    );
}
