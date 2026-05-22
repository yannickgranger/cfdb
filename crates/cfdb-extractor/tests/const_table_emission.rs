//! `:ConstTable` + `HAS_CONST_TABLE` end-to-end emission tests
//! (RFC-040 slice 3/5, issue #325).
//!
//! Drive the full `extract_workspace` pipeline against synthetic cargo
//! workspaces and assert on observable extractor output: that recognized
//! consts produce one `:ConstTable` node with the documented prop shape,
//! that the parent `:Item -[:HAS_CONST_TABLE]-> :ConstTable` edge is
//! emitted exactly once, and that non-recognized consts produce no
//! `:ConstTable` node (only the parent `:Item`).

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;
use tempfile::tempdir;

fn write_fixture_file(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().expect("fixture path has parent")).expect("mkdir -p");
    std::fs::write(p, contents).expect("write fixture");
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

fn const_tables(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CONST_TABLE)
        .collect()
}

fn has_const_table_edges(edges: &[Edge]) -> Vec<&Edge> {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::HAS_CONST_TABLE)
        .collect()
}

fn const_items(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && matches!(n.props.get("kind"), Some(PropValue::Str(s)) if s == "const")
        })
        .collect()
}

fn prop_str<'a>(props: &'a BTreeMap<String, PropValue>, key: &str) -> &'a str {
    match props
        .get(key)
        .unwrap_or_else(|| panic!("prop {key} missing"))
    {
        PropValue::Str(s) => s.as_str(),
        other => panic!("expected Str for {key}, got {other:?}"),
    }
}

fn prop_int(props: &BTreeMap<String, PropValue>, key: &str) -> i64 {
    match props
        .get(key)
        .unwrap_or_else(|| panic!("prop {key} missing"))
    {
        PropValue::Int(n) => *n,
        other => panic!("expected Int for {key}, got {other:?}"),
    }
}

fn prop_bool(props: &BTreeMap<String, PropValue>, key: &str) -> bool {
    match props
        .get(key)
        .unwrap_or_else(|| panic!("prop {key} missing"))
    {
        PropValue::Bool(b) => *b,
        other => panic!("expected Bool for {key}, got {other:?}"),
    }
}

// -- Positive: recognized consts --------------------------------------------

#[test]
fn pub_const_str_slice_emits_one_const_table_with_full_prop_shape() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "ctf",
        r#"pub const CURRENCIES: &[&str] = &["USD", "EUR", "GBP"];
"#,
    );

    let (nodes, edges) = extract_workspace(fx.path()).expect("extract");

    let cts = const_tables(&nodes);
    assert_eq!(
        cts.len(),
        1,
        "expected 1 :ConstTable node, got {}: {:?}",
        cts.len(),
        cts.iter().map(|n| &n.id).collect::<Vec<_>>()
    );

    let ct = cts[0];
    assert_eq!(ct.id, "const_table:ctf::CURRENCIES");
    assert_eq!(prop_str(&ct.props, "qname"), "ctf::CURRENCIES");
    assert_eq!(prop_str(&ct.props, "name"), "CURRENCIES");
    assert_eq!(prop_str(&ct.props, "crate"), "ctf");
    // module_qpath matches the :Item.module_qpath convention — the
    // fully-qualified path of the enclosing module, which at the crate
    // root is just the crate name.
    assert_eq!(prop_str(&ct.props, "module_qpath"), "ctf");
    assert_eq!(prop_str(&ct.props, "element_type"), "str");
    assert_eq!(prop_int(&ct.props, "entry_count"), 3);
    assert!(!prop_bool(&ct.props, "is_test"));

    // entries_normalized is sorted; entries_sample preserves declaration order.
    assert_eq!(
        prop_str(&ct.props, "entries_normalized"),
        r#"["EUR","GBP","USD"]"#,
    );
    assert_eq!(
        prop_str(&ct.props, "entries_sample"),
        r#"["USD","EUR","GBP"]"#,
    );

    // entries_hash is lowercase hex, length 64.
    let h = prop_str(&ct.props, "entries_hash");
    assert_eq!(h.len(), 64, "sha256 hex is 64 chars; got {}", h.len());
    assert!(h
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));

    // Exactly one HAS_CONST_TABLE edge from the parent :Item.
    let hcte = has_const_table_edges(&edges);
    assert_eq!(hcte.len(), 1);
    assert_eq!(hcte[0].src, "item:ctf::CURRENCIES");
    assert_eq!(hcte[0].dst, "const_table:ctf::CURRENCIES");
}

#[test]
fn pub_const_u32_array_emits_one_const_table_with_numeric_props() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "ctf",
        r#"pub const PORTS: [u32; 3] = [443, 80, 8080];
"#,
    );

    let (nodes, edges) = extract_workspace(fx.path()).expect("extract");

    let cts = const_tables(&nodes);
    assert_eq!(cts.len(), 1);
    let ct = cts[0];
    assert_eq!(prop_str(&ct.props, "element_type"), "u32");
    assert_eq!(prop_int(&ct.props, "entry_count"), 3);
    assert_eq!(prop_str(&ct.props, "entries_normalized"), "[80,443,8080]");
    assert_eq!(prop_str(&ct.props, "entries_sample"), "[443,80,8080]");

    let hcte = has_const_table_edges(&edges);
    assert_eq!(hcte.len(), 1);
}

#[test]
fn module_qpath_propagates_to_const_table_qname() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "ctf",
        r#"pub mod normalize {
    pub const Z_PREFIX: &[&str] = &["ZUSD", "ZEUR"];
}
"#,
    );

    let (nodes, _edges) = extract_workspace(fx.path()).expect("extract");
    let cts = const_tables(&nodes);
    assert_eq!(cts.len(), 1);
    let ct = cts[0];
    assert_eq!(prop_str(&ct.props, "qname"), "ctf::normalize::Z_PREFIX");
    assert_eq!(prop_str(&ct.props, "module_qpath"), "ctf::normalize");
}

// -- Cardinality invariant (R2 ddd-specialist N3) ----------------------------

#[test]
fn each_const_item_has_at_most_one_const_table_child() {
    // Mix recognized + non-recognized consts — every :Item{kind="const"}
    // must have either zero or one outgoing HAS_CONST_TABLE edge.
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "ctf",
        r#"pub const RECOGNIZED: &[&str] = &["a", "b"];
pub const SCALAR: u32 = 7;
pub const NUMERIC_TABLE: &[i64] = &[1, 2, 3];
pub const CUSTOM_TYPE_TABLE: &[CustomType] = &[];
pub struct CustomType;
"#,
    );

    let (nodes, edges) = extract_workspace(fx.path()).expect("extract");
    let consts = const_items(&nodes);
    assert_eq!(
        consts.len(),
        4,
        "expected 4 :Item{{kind=const}} nodes, got {}",
        consts.len(),
    );

    let hcte = has_const_table_edges(&edges);
    // Two recognized const tables (RECOGNIZED, NUMERIC_TABLE) → exactly two edges.
    assert_eq!(hcte.len(), 2);

    // Per-const cardinality check — each const_item :Item has zero or one
    // HAS_CONST_TABLE outgoing edge.
    for item in &consts {
        let outgoing = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::HAS_CONST_TABLE && e.src == item.id)
            .count();
        assert!(
            outgoing <= 1,
            "{:?} has {} HAS_CONST_TABLE edges; descriptor invariant says ≤ 1",
            item.id,
            outgoing,
        );
    }
}

// -- Negative: non-recognized consts ----------------------------------------

#[test]
fn scalar_const_emits_no_const_table() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "ctf",
        r#"pub const ANSWER: u32 = 42;
"#,
    );
    let (nodes, edges) = extract_workspace(fx.path()).expect("extract");
    assert!(const_tables(&nodes).is_empty());
    assert!(has_const_table_edges(&edges).is_empty());
    // The parent :Item must still exist.
    assert_eq!(const_items(&nodes).len(), 1);
}

#[test]
fn slice_of_custom_type_emits_no_const_table() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "ctf",
        r#"pub struct Token;
pub const TOKENS: &[Token] = &[];
"#,
    );
    let (nodes, edges) = extract_workspace(fx.path()).expect("extract");
    assert!(const_tables(&nodes).is_empty());
    assert!(has_const_table_edges(&edges).is_empty());
}

// -- is_test propagation -----------------------------------------------------

#[test]
fn const_table_inside_cfg_test_module_carries_is_test_true() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "ctf",
        r#"pub const PROD: &[&str] = &["a"];

#[cfg(test)]
mod tests {
    pub const FIXTURE: &[&str] = &["x", "y"];
}
"#,
    );
    let (nodes, _edges) = extract_workspace(fx.path()).expect("extract");
    let cts = const_tables(&nodes);
    assert_eq!(cts.len(), 2);

    let prod = cts
        .iter()
        .find(|n| prop_str(&n.props, "name") == "PROD")
        .expect("PROD :ConstTable");
    assert!(!prop_bool(&prod.props, "is_test"));

    let fixture = cts
        .iter()
        .find(|n| prop_str(&n.props, "name") == "FIXTURE")
        .expect("FIXTURE :ConstTable");
    assert!(prop_bool(&fixture.props, "is_test"));
}
