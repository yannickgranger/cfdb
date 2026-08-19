use std::collections::BTreeSet;
use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor_ts::TypeScriptProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

fn produce(body: &str) -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("package.json"), r#"{"name":"t"}"#).unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/m.ts"), body).unwrap();
    TypeScriptProducer.produce(dir.path()).expect("produce")
}

fn in_method(stmts: &str) -> (Vec<Node>, Vec<Edge>) {
    produce(&format!(
        "export class C {{\n    run(): void {{\n{stmts}\n    }}\n}}\n"
    ))
}

fn call_sites(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .collect()
}

fn callee_paths(nodes: &[Node]) -> BTreeSet<String> {
    call_sites(nodes)
        .iter()
        .filter_map(|n| n.props.get("callee_path").and_then(PropValue::as_str))
        .map(str::to_string)
        .collect()
}

fn prop<'a>(n: &'a Node, k: &str) -> Option<&'a str> {
    n.props.get(k).and_then(PropValue::as_str)
}

fn calls(edges: &[Edge]) -> usize {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
        .count()
}

#[test]
fn callee_path_shapes_per_table() {
    let (nodes, _e) = in_method(
        r#"        foo();
        obj.foo();
        this.m();
        super.m();
        obj?.m();
        a()();
        (() => {})();
        tag`x`;
        new MyClass();"#,
    );
    let paths = callee_paths(&nodes);
    for expected in [
        "foo",
        "obj.foo",
        "this.m",
        "super.m",
        "obj?.m",
        "a()",
        "a",
        "(() => {})",
        "tag",
    ] {
        assert!(
            paths.contains(expected),
            "missing callee_path {expected:?}; got {paths:?}",
        );
    }
    assert!(
        !paths.iter().any(|p| p.contains("MyClass")),
        "`new MyClass()` must not emit a :CallSite; got {paths:?}",
    );
}

#[test]
fn emits_zero_calls_edges() {
    let (_n, edges) = in_method("        foo(); obj.bar(); this.baz();");
    assert_eq!(
        calls(&edges),
        0,
        "TS emits no CALLS (callee_resolved=false)"
    );
}

#[test]
fn full_prop_set_and_invokes_at_direction() {
    let (nodes, edges) = in_method("        helper();");
    let cs = call_sites(&nodes);
    assert_eq!(cs.len(), 1);
    let cs = cs[0];
    assert!(
        prop(cs, "caller_qname").is_some_and(|q| q.ends_with("::C::run")),
        "caller_qname anchored to the method qname; got {:?}",
        prop(cs, "caller_qname"),
    );
    assert_eq!(prop(cs, "callee_path"), Some("helper"));
    assert_eq!(prop(cs, "callee_last_segment"), Some("helper"));
    assert_eq!(prop(cs, "kind"), Some("call"));
    assert_eq!(prop(cs, "file"), Some("src/m.ts"));
    assert!(cs.props.get("line").and_then(PropValue::as_i64).is_some());
    assert_eq!(
        cs.props.get("is_test").and_then(PropValue::as_bool),
        Some(false)
    );
    assert_eq!(prop(cs, "resolver"), Some("tree-sitter-typescript"));
    assert_eq!(
        cs.props.get("callee_resolved").and_then(PropValue::as_bool),
        Some(false),
    );

    let invokes: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::INVOKES_AT)
        .collect();
    assert_eq!(invokes.len(), 1);
    assert!(invokes[0].src.starts_with("item:") && invokes[0].src.ends_with("::C::run"));
    assert_eq!(invokes[0].dst, cs.id, "INVOKES_AT dst is the :CallSite");
}

#[test]
fn callee_last_segment_strips_member_path() {
    let (nodes, _e) = in_method("        obj.foo(); this.bar();");
    let seg = |path: &str| {
        call_sites(&nodes)
            .into_iter()
            .find(|n| prop(n, "callee_path") == Some(path))
            .and_then(|n| prop(n, "callee_last_segment").map(str::to_string))
    };
    assert_eq!(seg("obj.foo").as_deref(), Some("foo"));
    assert_eq!(seg("this.bar").as_deref(), Some("bar"));
}

#[test]
fn repeated_calls_get_distinct_ids() {
    let (nodes, _e) = in_method("        foo(); foo();");
    let foo: Vec<&Node> = call_sites(&nodes)
        .into_iter()
        .filter(|n| prop(n, "callee_path") == Some("foo"))
        .collect();
    assert_eq!(foo.len(), 2);
    assert_ne!(foo[0].id, foo[1].id);
    assert!(foo.iter().any(|n| n.id.ends_with(":0")));
    assert!(foo.iter().any(|n| n.id.ends_with(":1")));
}

#[test]
fn calls_inside_arrow_body_attributed_to_method() {
    let (nodes, _e) = in_method("        arr.map(x => bar(x));");
    let paths = callee_paths(&nodes);
    assert!(
        paths.contains("arr.map") && paths.contains("bar"),
        "got {paths:?}"
    );
    assert!(call_sites(&nodes)
        .iter()
        .all(|n| prop(n, "caller_qname").is_some_and(|q| q.ends_with("::C::run"))));
}

#[test]
fn top_level_function_body_is_walked() {
    let (nodes, _e) = produce("export function main(): void {\n    boot();\n}\n");
    let cs = call_sites(&nodes);
    assert_eq!(cs.len(), 1);
    assert_eq!(prop(cs[0], "callee_path"), Some("boot"));
    assert!(prop(cs[0], "caller_qname").is_some_and(|q| q.ends_with("::main")));
}

#[test]
fn richer_fixture_has_call_sites_and_zero_calls() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ts-richer");
    let (nodes, edges) = TypeScriptProducer
        .produce(&root)
        .expect("produce ts-richer");
    let paths = callee_paths(&nodes);
    assert!(
        paths.contains("this.id"),
        "Product::toJSON calls this.id(); got {paths:?}",
    );
    assert!(call_sites(&nodes).iter().all(|n| n
        .props
        .get("callee_resolved")
        .and_then(PropValue::as_bool)
        == Some(false)));
    assert_eq!(calls(&edges), 0, "ts-richer emits zero CALLS");
}

#[test]
fn re_extract_is_deterministic() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"t"}"#).unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/m.ts"),
        "export class C { run(): void { foo(); foo(); this.m(); } }\n",
    )
    .unwrap();
    let r1 = TypeScriptProducer.produce(dir.path()).unwrap();
    let r2 = TypeScriptProducer.produce(dir.path()).unwrap();
    assert_eq!(format!("{r1:?}"), format!("{r2:?}"));
}
