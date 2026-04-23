//! `:Variant` + `HAS_VARIANT` + variant-field emission tests (#218,
//! RFC-037 §3.3).
//!
//! Before #218 the extractor emitted the enum as `:Item{kind:enum}` and
//! stopped — `node.variants` was not walked. No `:Variant` nodes, no
//! `HAS_VARIANT` edges, no variant-field `:Field` nodes. These tests
//! assert that the producer now walks every variant and emits:
//!
//! - one `:Variant` node per variant with `{index, name, parent_qname,
//!   payload_kind}` props (all four authoritative per
//!   `variant_node_descriptor` at `cfdb-core/src/schema/describe/nodes.rs`);
//! - one `HAS_VARIANT` edge from the enum's `:Item` to each variant;
//! - variant payload `:Field` nodes with `HAS_FIELD` src = the variant's
//!   `:Variant` node id (not the enum's `:Item` id — the descriptor's
//!   `from:` widened to `[:Item, :Variant]`);
//! - tuple-variant fields use synthetic `_0`, `_1`, ... names and
//!   parallel the tuple-struct fields shipped in the same slice.
//!
//! The harness mirrors `field_emission.rs` and `param_emission.rs`: a
//! real cargo workspace in a tempdir + `extract_workspace` end-to-end.

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
fn unit_tuple_and_struct_variants_all_emit_variant_nodes() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "variantmix",
        r#"pub enum Shape {
    Nothing,
    Point(i32, i32),
    Named { x: i32, y: i32 },
}
"#,
    );
    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");

    let variant_label = Label::new(Label::VARIANT);
    let variants: Vec<_> = nodes.iter().filter(|n| n.label == variant_label).collect();
    assert_eq!(
        variants.len(),
        3,
        "three enum variants must emit three :Variant nodes"
    );

    // The three variants ship in source order (index 0, 1, 2) with the
    // matching payload_kind discriminator.
    let mut by_index: Vec<(i64, String, String)> = variants
        .iter()
        .map(|n| {
            (
                prop_int(&n.props, "index"),
                prop_str(&n.props, "name").to_string(),
                prop_str(&n.props, "payload_kind").to_string(),
            )
        })
        .collect();
    by_index.sort_by_key(|(i, _, _)| *i);
    assert_eq!(
        by_index,
        vec![
            (0, "Nothing".to_string(), "unit".to_string()),
            (1, "Point".to_string(), "tuple".to_string()),
            (2, "Named".to_string(), "struct".to_string()),
        ],
        "variants must emit in declaration order with matching payload_kind"
    );

    // Every :Variant carries parent_qname = the enum's qname (NOT the
    // variant's own qname — that is the `:Field.parent_qname` for
    // payload fields).
    for v in &variants {
        assert_eq!(
            prop_str(&v.props, "parent_qname"),
            "variantmix::Shape",
            ":Variant.parent_qname must be the owning enum's qname"
        );
    }
}

#[test]
fn enum_with_variants_emits_has_variant_edges_from_item_to_variant() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "hasvariant",
        r#"pub enum E {
    A,
    B,
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let has_variant = EdgeLabel::new(EdgeLabel::HAS_VARIANT);
    let has_variant_edges: Vec<_> = edges.iter().filter(|e| e.label == has_variant).collect();
    assert_eq!(
        has_variant_edges.len(),
        2,
        "one HAS_VARIANT edge per variant"
    );

    let item_label = Label::new(Label::ITEM);
    let enum_item = nodes
        .iter()
        .find(|n| n.label == item_label && prop_str(&n.props, "qname") == "hasvariant::E")
        .expect(":Item for enum E must exist");

    for e in &has_variant_edges {
        assert_eq!(
            e.src, enum_item.id,
            "HAS_VARIANT src must be the enum's :Item id"
        );
    }
}

#[test]
fn struct_variant_payload_emits_fields_with_variant_as_has_field_src() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "variantstruct",
        r#"pub enum E {
    WithFields { a: i32, b: String },
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let field_label = Label::new(Label::FIELD);
    let variant_label = Label::new(Label::VARIANT);

    let variant = nodes
        .iter()
        .find(|n| n.label == variant_label)
        .expect(":Variant must exist");
    let variant_id = variant.id.clone();

    // Fields scoped to this variant have parent_qname = Enum::Variant.
    let variant_fields: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.label == field_label
                && prop_str(&n.props, "parent_qname") == "variantstruct::E::WithFields"
        })
        .collect();
    assert_eq!(
        variant_fields.len(),
        2,
        "struct variant with 2 fields emits 2 :Field nodes"
    );

    // Field names + indices match the source.
    let mut by_index: Vec<(i64, String)> = variant_fields
        .iter()
        .map(|n| {
            (
                prop_int(&n.props, "index"),
                prop_str(&n.props, "name").to_string(),
            )
        })
        .collect();
    by_index.sort_by_key(|(i, _)| *i);
    assert_eq!(by_index, vec![(0, "a".to_string()), (1, "b".to_string())]);

    // HAS_FIELD edges for these fields must have src = the :Variant id
    // (not the enum's :Item id). This is the descriptor-widening
    // invariant — `HAS_FIELD.from` now includes `:Variant`.
    let has_field = EdgeLabel::new(EdgeLabel::HAS_FIELD);
    let variant_field_ids: std::collections::BTreeSet<String> =
        variant_fields.iter().map(|n| n.id.clone()).collect();
    let field_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.label == has_field && variant_field_ids.contains(&e.dst))
        .collect();
    for e in &field_edges {
        assert_eq!(
            e.src, variant_id,
            "HAS_FIELD on variant field must have src = :Variant id"
        );
    }
}

#[test]
fn tuple_variant_payload_emits_indexed_fields_with_underscore_names() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "varianttuple",
        r#"pub enum T {
    Pair(i32, String, bool),
}
"#,
    );
    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");

    let field_label = Label::new(Label::FIELD);
    let variant_fields: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.label == field_label && prop_str(&n.props, "parent_qname") == "varianttuple::T::Pair"
        })
        .collect();
    assert_eq!(
        variant_fields.len(),
        3,
        "3-tuple variant emits 3 :Field nodes"
    );

    let mut names: Vec<(i64, String)> = variant_fields
        .iter()
        .map(|n| {
            (
                prop_int(&n.props, "index"),
                prop_str(&n.props, "name").to_string(),
            )
        })
        .collect();
    names.sort_by_key(|(i, _)| *i);
    assert_eq!(
        names,
        vec![
            (0, "_0".to_string()),
            (1, "_1".to_string()),
            (2, "_2".to_string()),
        ],
        "tuple variant fields use _0, _1, _2 convention"
    );
}

#[test]
fn unit_variant_emits_no_has_field_edges() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "variantunit",
        r#"pub enum U {
    Only,
}
"#,
    );
    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let field_label = Label::new(Label::FIELD);
    let variant_scoped_fields: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.label == field_label
                && prop_str(&n.props, "parent_qname").starts_with("variantunit::U::")
        })
        .collect();
    assert!(
        variant_scoped_fields.is_empty(),
        "unit variant payload has no fields"
    );

    // The :Variant node itself does exist, with payload_kind=unit.
    let variant = nodes
        .iter()
        .find(|n| n.label == Label::new(Label::VARIANT))
        .expect(":Variant node must exist for a unit variant too");
    assert_eq!(prop_str(&variant.props, "payload_kind"), "unit");

    // No HAS_FIELD edge sourced at the variant (or indeed anywhere —
    // unit variants declare no fields).
    let has_field = EdgeLabel::new(EdgeLabel::HAS_FIELD);
    assert!(
        edges
            .iter()
            .all(|e| e.label != has_field || e.src != variant.id),
        "no HAS_FIELD edge has src = unit-variant's :Variant id"
    );
}

#[test]
fn tuple_struct_now_emits_field_nodes_with_underscore_names() {
    // Regression: before #218, `visit_item_struct` only handled
    // `Fields::Named`. Tuple structs (`struct Foo(i32, String)`)
    // emitted the :Item but no :Field nodes. `emit_field_list` now
    // handles `Fields::Unnamed` uniformly for both structs and
    // variants.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "tuplestruct",
        r#"pub struct Wrap(pub i32, pub String);
"#,
    );
    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");

    let field_label = Label::new(Label::FIELD);
    let struct_fields: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.label == field_label && prop_str(&n.props, "parent_qname") == "tuplestruct::Wrap"
        })
        .collect();
    assert_eq!(
        struct_fields.len(),
        2,
        "tuple struct with 2 elements emits 2 :Field nodes"
    );
    let mut names: Vec<(i64, String)> = struct_fields
        .iter()
        .map(|n| {
            (
                prop_int(&n.props, "index"),
                prop_str(&n.props, "name").to_string(),
            )
        })
        .collect();
    names.sort_by_key(|(i, _)| *i);
    assert_eq!(
        names,
        vec![(0, "_0".to_string()), (1, "_1".to_string())],
        "tuple-struct fields use _0, _1 convention"
    );
}

#[test]
fn variant_node_ids_follow_canonical_formula() {
    // The `variant_node_id(enum_qname, index)` formula lands in
    // `cfdb-core::qname` (#215). Assert the walker routes through it
    // and does NOT hand-forge a parallel id shape.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "variantid",
        r#"pub enum E { A, B }
"#,
    );
    let (nodes, _edges) = extract_workspace(fixture.path()).expect("extract");

    let variant_label = Label::new(Label::VARIANT);
    let mut ids: Vec<String> = nodes
        .iter()
        .filter(|n| n.label == variant_label)
        .map(|n| n.id.clone())
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "variant:variantid::E#0".to_string(),
            "variant:variantid::E#1".to_string(),
        ]
    );
}
