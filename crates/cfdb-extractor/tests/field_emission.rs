//! `:Field` node attribute alignment tests (#217, RFC-037 §3.5).
//!
//! The `:Field` descriptor at `cfdb-core/src/schema/describe/nodes.rs`
//! declares five authoritative props: `{index, name, parent_qname,
//! type_normalized, type_path}`. Before #217 the emitter shipped only
//! three (`{name, parent_qname, type_qname}`) and `type_qname` is not
//! part of the descriptor. These tests assert that every emitted
//! `:Field` node carries exactly the descriptor's five props and that
//! `index` follows declaration order.
//!
//! The fixture harness mirrors `param_emission.rs`: a real cargo
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

#[test]
fn struct_with_three_named_fields_emits_three_field_nodes_and_has_field_edges() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "fieldfixture",
        r#"pub struct Foo {
    pub a: i32,
    pub b: String,
    pub c: bool,
}
"#,
    );

    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let fields: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::FIELD)
        .collect();
    assert_eq!(
        fields.len(),
        3,
        "expected 3 :Field nodes for struct Foo {{ a, b, c }}, got {}: {:?}",
        fields.len(),
        fields.iter().map(|n| &n.id).collect::<Vec<_>>()
    );

    let has_field_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::HAS_FIELD)
        .collect();
    assert_eq!(
        has_field_edges.len(),
        3,
        "expected 3 HAS_FIELD edges, got {}",
        has_field_edges.len()
    );
}

#[test]
fn field_nodes_carry_descriptor_five_props_only() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "descriptorfixture",
        r#"pub struct Foo {
    pub a: i32,
    pub b: String,
    pub c: bool,
}
"#,
    );

    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");

    let fields: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::FIELD)
        .collect();
    assert_eq!(fields.len(), 3);

    for f in &fields {
        // Every descriptor-declared prop must be present.
        assert!(
            f.props.contains_key("index"),
            "descriptor prop `index` missing on {:?}",
            f.id
        );
        assert!(
            f.props.contains_key("name"),
            "descriptor prop `name` missing on {:?}",
            f.id
        );
        assert!(
            f.props.contains_key("parent_qname"),
            "descriptor prop `parent_qname` missing on {:?}",
            f.id
        );
        assert!(
            f.props.contains_key("type_normalized"),
            "descriptor prop `type_normalized` missing on {:?}",
            f.id
        );
        assert!(
            f.props.contains_key("type_path"),
            "descriptor prop `type_path` missing on {:?}",
            f.id
        );

        // Legacy prop must be gone — the descriptor does not declare it.
        assert!(
            !f.props.contains_key("type_qname"),
            "legacy `type_qname` prop must be removed from :Field emission (found on {:?})",
            f.id
        );

        // No props outside the descriptor set.
        let expected: std::collections::BTreeSet<&str> = [
            "index",
            "name",
            "parent_qname",
            "type_normalized",
            "type_path",
        ]
        .into_iter()
        .collect();
        for key in f.props.keys() {
            assert!(
                expected.contains(key.as_str()),
                "unexpected prop `{key}` on :Field node {:?}",
                f.id
            );
        }
    }
}

#[test]
fn field_index_matches_declaration_order() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "indexfixture",
        r#"pub struct Foo {
    pub a: i32,
    pub b: String,
    pub c: bool,
}
"#,
    );

    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");

    let fields: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::FIELD)
        .collect();
    assert_eq!(fields.len(), 3);

    let mut by_index: Vec<_> = fields
        .iter()
        .map(|n| {
            (
                prop_int(&n.props, "index"),
                prop_str(&n.props, "name").to_string(),
            )
        })
        .collect();
    by_index.sort_by_key(|(i, _)| *i);
    assert_eq!(
        by_index,
        vec![
            (0, "a".to_string()),
            (1, "b".to_string()),
            (2, "c".to_string()),
        ],
        "field `index` must match declaration order"
    );
}

#[test]
fn field_type_normalized_and_type_path_both_populated_with_rendered_type() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "typefixture",
        r#"pub struct Foo {
    pub a: i32,
    pub b: String,
    pub c: bool,
}
"#,
    );

    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");

    let fields: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::FIELD)
        .collect();
    assert_eq!(fields.len(), 3);

    for f in &fields {
        let tn = prop_str(&f.props, "type_normalized");
        let tp = prop_str(&f.props, "type_path");
        assert!(!tn.is_empty(), "type_normalized populated on {:?}", f.id);
        assert!(!tp.is_empty(), "type_path populated on {:?}", f.id);
        // The split becomes meaningful only when `render_type_inner`
        // lands (RFC-037 §6 non-goals). Today both columns carry the
        // same rendered type string.
        assert_eq!(
            tn, tp,
            "type_normalized and type_path currently carry the same \
             rendered type string; the split is semantic preparation only"
        );
    }

    // Spot-check the concrete values — extractor renders primitives
    // as their spelled token and `String` as its bare identifier.
    let by_name: std::collections::BTreeMap<String, &&cfdb_core::Node> = fields
        .iter()
        .map(|n| (prop_str(&n.props, "name").to_string(), n))
        .collect();
    assert_eq!(prop_str(&by_name["a"].props, "type_path"), "i32");
    assert_eq!(prop_str(&by_name["b"].props, "type_path"), "String");
    assert_eq!(prop_str(&by_name["c"].props, "type_path"), "bool");
}

#[test]
fn has_field_edge_points_from_parent_item_to_field_node() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "edgefixture",
        r#"pub struct Foo {
    pub a: i32,
}
"#,
    );

    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let item_struct = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && matches!(n.props.get("kind"), Some(PropValue::Str(s)) if s == "struct")
                && matches!(n.props.get("name"), Some(PropValue::Str(s)) if s == "Foo")
        })
        .expect(":Item{kind:struct, name:Foo} present");
    let field_node = nodes
        .iter()
        .find(|n| n.label.as_str() == Label::FIELD)
        .expect(":Field node present");

    let edge = edges
        .iter()
        .find(|e| e.label.as_str() == EdgeLabel::HAS_FIELD)
        .expect("HAS_FIELD edge present");
    assert_eq!(
        edge.src, item_struct.id,
        "HAS_FIELD src must be parent :Item{{kind:struct}} node id"
    );
    assert_eq!(
        edge.dst, field_node.id,
        "HAS_FIELD dst must be :Field node id"
    );
}
