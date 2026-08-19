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

fn argument_nodes(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ARGUMENT)
        .collect()
}

fn has_arg_edges(edges: &[Edge]) -> Vec<&Edge> {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::HAS_ARG)
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

#[test]
fn expr_call_emits_one_argument_with_method_call_kind() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "argtest",
        r#"pub struct Connection;
pub struct RedisInventory;
impl RedisInventory {
    pub fn new(_c: Connection) -> Self { RedisInventory }
}
pub fn create_inventory(conn: Connection) -> RedisInventory {
    RedisInventory::new(conn.clone())
}
"#,
    );

    let (nodes, edges) = extract_workspace(fx.path()).expect("extract");

    let args = argument_nodes(&nodes);
    assert!(
        !args.is_empty(),
        "expected at least one :Argument node, got none"
    );

    let method_call_arg = args
        .iter()
        .find(|n| {
            matches!(
                n.props.get("kind"),
                Some(PropValue::Str(k)) if k == "method_call"
            ) && matches!(n.props.get("position"), Some(PropValue::Int(0)))
        })
        .unwrap_or_else(|| {
            panic!(
                "expected an :Argument with kind=method_call and position=0; \
                 got: {:?}",
                args.iter()
                    .map(|n| (&n.id, n.props.get("kind"), n.props.get("position")))
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(prop_int(&method_call_arg.props, "position"), 0);
    assert_eq!(prop_str(&method_call_arg.props, "kind"), "method_call");

    let src_text = prop_str(&method_call_arg.props, "source_text");
    assert!(
        src_text.contains("clone"),
        "source_text should contain 'clone'; got {src_text:?}"
    );

    assert!(
        prop_int(&method_call_arg.props, "line") > 0,
        "line should be > 0"
    );

    assert!(
        prop_int(&method_call_arg.props, "col") > 0,
        "col should be > 0 (1-indexed)"
    );

    assert!(
        method_call_arg.id.starts_with("arg:"),
        "argument id must start with 'arg:'; got {:?}",
        method_call_arg.id
    );

    let has_arg = has_arg_edges(&edges);
    assert!(
        !has_arg.is_empty(),
        "expected at least one HAS_ARG edge, got none"
    );
    let edge_to_arg = has_arg
        .iter()
        .find(|e| e.dst == method_call_arg.id)
        .unwrap_or_else(|| {
            panic!(
                "no HAS_ARG edge pointing to argument {:?}",
                method_call_arg.id
            )
        });
    assert!(
        edge_to_arg.src.starts_with("callsite:"),
        "HAS_ARG source must start with 'callsite:'; got {:?}",
        edge_to_arg.src
    );
}

#[test]
fn expr_method_call_emits_receiver_at_position_zero() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "recvtest",
        r#"pub struct Connection;
impl Connection {
    pub fn clone(&self) -> Self { Connection }
}
pub fn dup_conn(conn: Connection) -> Connection {
    conn.clone()
}
"#,
    );

    let (nodes, _edges) = extract_workspace(fx.path()).expect("extract");

    let args = argument_nodes(&nodes);

    let receiver_arg = args
        .iter()
        .find(|n| {
            matches!(
                n.props.get("kind"),
                Some(PropValue::Str(k)) if k == "path"
            ) && matches!(n.props.get("position"), Some(PropValue::Int(0)))
        })
        .unwrap_or_else(|| {
            panic!(
                "expected an :Argument with kind=path and position=0 (receiver); \
                 got: {:?}",
                args.iter()
                    .map(|n| (&n.id, n.props.get("kind"), n.props.get("position")))
                    .collect::<Vec<_>>()
            )
        });

    let src = prop_str(&receiver_arg.props, "source_text");
    assert!(
        src.contains("conn"),
        "receiver source_text should contain 'conn'; got {src:?}"
    );
    assert_eq!(prop_int(&receiver_arg.props, "position"), 0);
}

#[test]
fn argument_ids_share_callsite_prefix() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(
        fx.path(),
        "prefixtest",
        r#"pub struct S;
impl S {
    pub fn multi(_a: i32, _b: i32) -> i32 { 0 }
}
pub fn caller() -> i32 {
    S::multi(1, 2)
}
"#,
    );

    let (nodes, _edges) = extract_workspace(fx.path()).expect("extract");

    let args = argument_nodes(&nodes);
    for arg in &args {
        assert!(
            arg.id.starts_with("arg:callsite:"),
            "argument id {0:?} must start with 'arg:callsite:'; \
             all :Argument ids must share the callsite id prefix so lifecycle \
             coupling (RFC-043 §4 Invariant 8) holds",
            arg.id,
        );
    }
}
