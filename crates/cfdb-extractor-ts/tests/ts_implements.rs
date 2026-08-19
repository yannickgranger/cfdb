use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor_ts::TypeScriptProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

fn produce_ts(files: &[(&str, &str)]) -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("package.json"), r#"{"name":"t"}"#).expect("package.json");
    fs::write(dir.path().join("tsconfig.json"), "{}").expect("tsconfig.json");
    for (rel, src) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir -p");
        }
        fs::write(&path, src).expect("write ts source");
    }
    TypeScriptProducer
        .produce(dir.path())
        .expect("TypeScriptProducer.produce")
}

fn produce_one(body: &str) -> (Vec<Node>, Vec<Edge>) {
    produce_ts(&[("src/m.ts", body)])
}

fn item_name<'a>(nodes: &'a [Node], id: &str) -> Option<&'a str> {
    nodes
        .iter()
        .find(|n| n.id == id)
        .and_then(|n| n.props.get("name"))
        .and_then(PropValue::as_str)
}

fn implements_name_pairs(nodes: &[Node], edges: &[Edge]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IMPLEMENTS)
        .map(|e| {
            (
                item_name(nodes, &e.src).unwrap_or("?").to_string(),
                item_name(nodes, &e.dst).unwrap_or("?").to_string(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

fn implements_edges(edges: &[Edge]) -> Vec<&Edge> {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IMPLEMENTS)
        .collect()
}

fn item_by_name<'a>(nodes: &'a [Node], name: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| {
        n.label.as_str() == Label::ITEM
            && n.props.get("name").and_then(PropValue::as_str) == Some(name)
    })
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

#[test]
fn extends_plus_implements_emits_only_two_implements_edges() {
    let (nodes, edges) = produce_one(
        r#"interface I1 {}
interface I2 {}
class Base {}
class C extends Base implements I1, I2 {}
"#,
    );
    assert_eq!(
        implements_name_pairs(&nodes, &edges),
        vec![
            ("C".to_string(), "I1".to_string()),
            ("C".to_string(), "I2".to_string()),
        ],
        "exactly C→I1, C→I2; `extends Base` must NOT appear",
    );

    for e in implements_edges(&edges) {
        assert_eq!(
            e.props.get("resolver").and_then(PropValue::as_str),
            Some("tree-sitter-typescript"),
        );
    }
    let c = item_by_name(&nodes, "C").expect("class C :Item");
    assert_eq!(prop(c, "kind"), Some("struct"));
    assert_eq!(prop(c, "ts_construct"), Some("class_declaration"));
    let i1 = item_by_name(&nodes, "I1").expect("interface I1 :Item");
    assert_eq!(prop(i1, "kind"), Some("trait"));
    assert_eq!(prop(i1, "ts_construct"), Some("interface_declaration"));
}

#[test]
fn generic_implements_uses_full_text_and_does_not_match_bare_name() {
    let (nodes, edges) = produce_one(
        r#"interface Generic<T> {}
class C implements Generic<T> {}
"#,
    );
    assert!(item_by_name(&nodes, "Generic").is_some());
    assert!(
        implements_edges(&edges).is_empty(),
        "`Generic<T>` (full text) must not resolve to the bare `Generic` name; got {:?}",
        implements_name_pairs(&nodes, &edges),
    );
}

#[test]
fn nested_type_identifier_does_not_resolve() {
    let (nodes, edges) = produce_one(
        r#"interface I2 {}
class C implements ns.I2 {}
"#,
    );
    assert!(
        implements_edges(&edges).is_empty(),
        "`ns.I2` must not match the bare interface `I2` (closed-world); got {:?}",
        implements_name_pairs(&nodes, &edges),
    );
}

#[test]
fn intersection_type_falls_back_to_raw_text_and_drops() {
    let (nodes, edges) = produce_one(
        r#"interface A {}
interface B {}
class C implements A & B {}
"#,
    );
    assert!(
        implements_edges(&edges).is_empty(),
        "`A & B` raw text matches no single bare name; got {:?}",
        implements_name_pairs(&nodes, &edges),
    );
}

#[test]
fn external_interface_emits_no_edge_and_no_synthetic_item() {
    let (nodes, edges) = produce_one(
        r#"class C implements Serializable {}
"#,
    );
    assert!(implements_edges(&edges).is_empty());
    assert!(
        item_by_name(&nodes, "Serializable").is_none(),
        "no synthetic :Item may be created for an external interface",
    );
}

#[test]
fn transitive_interface_extends_is_not_bridged() {
    let (nodes, edges) = produce_one(
        r#"interface B {}
interface A extends B {}
class C implements A {}
"#,
    );
    assert_eq!(
        implements_name_pairs(&nodes, &edges),
        vec![("C".to_string(), "A".to_string())],
        "only the syntactic `C implements A`; nothing to B",
    );
    let implementors_of_b: Vec<_> = implements_edges(&edges)
        .into_iter()
        .filter(|e| item_name(&nodes, &e.dst) == Some("B"))
        .collect();
    assert!(implementors_of_b.is_empty(), "D3-a gap must be stable");
}

#[test]
fn cross_file_simple_name_resolves() {
    let (nodes, edges) = produce_ts(&[
        ("src/iface.ts", "export interface Handler {}\n"),
        ("src/impl.ts", "export class Worker implements Handler {}\n"),
    ]);
    assert_eq!(
        implements_name_pairs(&nodes, &edges),
        vec![("Worker".to_string(), "Handler".to_string())],
    );
}

#[test]
fn ambiguous_name_is_dropped() {
    let (nodes, edges) = produce_ts(&[
        ("src/a.ts", "export interface Dup {}\n"),
        (
            "src/b.ts",
            "export interface Dup {}\nexport class C implements Dup {}\n",
        ),
    ]);
    assert!(
        implements_edges(&edges).is_empty(),
        "ambiguous name must not emit a guessed edge; got {:?}",
        implements_name_pairs(&nodes, &edges),
    );
}

#[test]
fn re_extract_is_deterministic() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("package.json"), r#"{"name":"t"}"#).unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/m.ts"),
        "interface I1 {}\ninterface I2 {}\nclass A implements I1 {}\nclass B implements I1, I2 {}\n",
    )
    .unwrap();
    let run1 = TypeScriptProducer.produce(dir.path()).unwrap();
    let run2 = TypeScriptProducer.produce(dir.path()).unwrap();
    assert_eq!(format!("{run1:?}"), format!("{run2:?}"));
}

#[test]
fn adding_one_implementing_class_adds_one_edge() {
    let before = produce_one("interface I {}\nclass A implements I {}\n");
    let after = produce_one("interface I {}\nclass A implements I {}\nclass Z implements I {}\n");
    assert_eq!(
        implements_edges(&before.1).len() + 1,
        implements_edges(&after.1).len(),
    );
}

fn richer_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ts-richer")
}

#[test]
fn richer_fixture_implements_topology() {
    let (nodes, edges) = TypeScriptProducer
        .produce(&richer_fixture_root())
        .expect("produce ts-richer fixture");
    assert_eq!(
        implements_name_pairs(&nodes, &edges),
        vec![
            ("Order".to_string(), "Timestamped".to_string()),
            ("Product".to_string(), "Identifiable".to_string()),
            ("Product".to_string(), "Serializable".to_string()),
        ],
    );
    for e in implements_edges(&edges) {
        assert_eq!(
            e.props.get("resolver").and_then(PropValue::as_str),
            Some("tree-sitter-typescript"),
        );
    }
}

#[test]
fn richer_fixture_is_deterministic() {
    let r1 = TypeScriptProducer.produce(&richer_fixture_root()).unwrap();
    let r2 = TypeScriptProducer.produce(&richer_fixture_root()).unwrap();
    assert_eq!(format!("{r1:?}"), format!("{r2:?}"));
}
