use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor_ts::TypeScriptProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

fn produce_one(body: &str) -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("package.json"), r#"{"name":"t"}"#).unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/m.ts"), body).unwrap();
    TypeScriptProducer.produce(dir.path()).expect("produce")
}

fn method_members(nodes: &[Node]) -> Vec<String> {
    let mut out: Vec<String> = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("kind").and_then(PropValue::as_str) == Some("fn")
        })
        .filter_map(|n| n.props.get("qname").and_then(PropValue::as_str))
        .filter_map(|q| {
            let parts: Vec<&str> = q.split("::").collect();
            (parts.len() == 4).then(|| format!("{}::{}", parts[2], parts[3]))
        })
        .collect();
    out.sort();
    out
}

fn method_item<'a>(nodes: &'a [Node], class_method: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| {
        n.props
            .get("qname")
            .and_then(PropValue::as_str)
            .is_some_and(|q| q.ends_with(&format!("::{class_method}")))
    })
}

#[test]
fn class_with_two_methods_emits_two_method_items() {
    let (nodes, edges) = produce_one(
        r#"export class Calc {
    add(a: number, b: number): number { return a + b; }
    sub(a: number, b: number): number { return a - b; }
}
"#,
    );
    assert_eq!(
        method_members(&nodes),
        vec!["Calc::add".to_string(), "Calc::sub".to_string()],
    );
    let add = method_item(&nodes, "Calc::add").expect("Calc::add method :Item");
    assert_eq!(
        add.props.get("kind").and_then(PropValue::as_str),
        Some("fn")
    );
    let in_crate = edges
        .iter()
        .filter(|e| e.label.as_str() == "IN_CRATE" && e.src == add.id)
        .count();
    let in_module = edges
        .iter()
        .filter(|e| e.label.as_str() == "IN_MODULE" && e.src == add.id)
        .count();
    assert_eq!(in_crate, 1, "method :Item has one IN_CRATE edge");
    assert_eq!(in_module, 1, "method :Item has one IN_MODULE edge");
}

#[test]
fn arrow_field_is_a_method_plain_field_is_not() {
    let (nodes, _edges) = produce_one(
        r#"export class C {
    handler = () => {};
    count = 42;
}
"#,
    );
    assert_eq!(method_members(&nodes), vec!["C::handler".to_string()]);
}

#[test]
fn getter_and_setter_collapse_to_one_method() {
    let (nodes, _edges) = produce_one(
        r#"export class Box {
    get value(): number { return 1; }
    set value(v: number) {}
}
"#,
    );
    assert_eq!(method_members(&nodes), vec!["Box::value".to_string()]);
}

#[test]
fn async_generator_static_methods_are_emitted() {
    let (nodes, _edges) = produce_one(
        r#"export class Svc {
    async load(): Promise<void> {}
    *items(): Iterator<number> { yield 1; }
    static make(): Svc { return new Svc(); }
}
"#,
    );
    assert_eq!(
        method_members(&nodes),
        vec![
            "Svc::items".to_string(),
            "Svc::load".to_string(),
            "Svc::make".to_string(),
        ],
    );
}

#[test]
fn abstract_signature_skipped_concrete_emitted() {
    let (nodes, _edges) = produce_one(
        r#"export abstract class Base {
    abstract handle(): void;
    run(): void {}
}
"#,
    );
    assert_eq!(method_members(&nodes), vec!["Base::run".to_string()]);
}

#[test]
fn interface_method_signatures_are_not_emitted() {
    let (nodes, _edges) = produce_one(
        r#"export interface Handler {
    handle(event: string): void;
}
"#,
    );
    assert!(
        method_members(&nodes).is_empty(),
        "interface signatures are not method :Items; got {:?}",
        method_members(&nodes),
    );
}

#[test]
fn access_modifiers_map_to_visibility() {
    let (nodes, _edges) = produce_one(
        r#"export class C {
    pub(): void {}
    private secret(): void {}
    protected guarded(): void {}
}
"#,
    );
    let vis = |cm: &str| {
        method_item(&nodes, cm)
            .and_then(|n| n.props.get("visibility"))
            .and_then(PropValue::as_str)
            .map(str::to_string)
    };
    assert_eq!(vis("C::pub").as_deref(), Some("public"));
    assert_eq!(vis("C::secret").as_deref(), Some("private"));
    assert_eq!(vis("C::guarded").as_deref(), Some("protected"));
}

#[test]
fn re_extract_is_deterministic() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("package.json"), r#"{"name":"t"}"#).unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/m.ts"),
        "export class C { a(): void {} b = () => {}; get p(): number { return 1; } }\n",
    )
    .unwrap();
    let r1 = TypeScriptProducer.produce(dir.path()).unwrap();
    let r2 = TypeScriptProducer.produce(dir.path()).unwrap();
    assert_eq!(format!("{r1:?}"), format!("{r2:?}"));
}

fn richer_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ts-richer")
}

#[test]
fn method_named_like_interface_does_not_break_implements() {
    let (nodes, edges) = produce_one(
        r#"export interface Handler {}
export class Worker implements Handler {
    Handler(): void {}
}
"#,
    );
    assert!(method_members(&nodes).contains(&"Worker::Handler".to_string()));
    let impls: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IMPLEMENTS)
        .collect();
    assert_eq!(impls.len(), 1, "Worker implements Handler must resolve");
    assert!(
        impls[0].dst.ends_with("::Handler") && !impls[0].dst.contains("Worker::Handler"),
        "IMPLEMENTS target must be the interface, not the method; got {:?}",
        impls[0].dst,
    );
}

#[test]
fn richer_fixture_emits_class_methods() {
    let (nodes, _edges) = TypeScriptProducer
        .produce(&richer_fixture_root())
        .expect("produce ts-richer");
    let members = method_members(&nodes);
    assert!(
        members.contains(&"Product::id".to_string())
            && members.contains(&"Product::toJSON".to_string())
            && members.contains(&"Order::createdAt".to_string()),
        "expected Product/Order methods; got {members:?}",
    );
}
