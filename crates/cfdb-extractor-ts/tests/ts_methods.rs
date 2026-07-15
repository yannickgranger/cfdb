//! `TypeScriptProducer` method-level `:Item` emission (RFC-045 45-D0 / #464).
//!
//! The producer descends a `class_body` and emits a method `:Item{kind:"fn"}`
//! per `method_definition` (regular/get/set/async/generator/static) and per
//! arrow-valued `public_field_definition` (`foo = () => {}`). Abstract /
//! signature-only members and non-arrow fields are skipped. The method qname
//! is `{crate}::{module}::{Class}::{method}`.

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

/// `Class::method` strings for every method `:Item` (a `kind:"fn"` item whose
/// qname carries the 4-segment `crate::module::Class::method` shape, i.e. has
/// the class infix — top-level functions have only 3 segments). Returned
/// crate-name-independent and sorted.
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

/// A class with two methods → two method `:Item`s with the `::Class::method`
/// qname, `kind:"fn"`, and IN_CRATE/IN_MODULE containment.
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
    // containment edges present for the method id.
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

/// An arrow-assigned field (`foo = () => {}`) is a method; a plain field
/// (`x = 42`) is not.
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

/// A getter and setter for the same property collapse to a single method
/// `:Item` (first wins) — both share the `::Class::prop` qname.
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

/// `async`, generator, and `static` methods are all `method_definition` → all
/// emitted.
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

/// An abstract method signature (no body) is skipped; a concrete method in the
/// same abstract class is emitted.
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

/// Interface members (`method_signature` in an `interface_body`) are NOT
/// methods — the producer descends `class_body` only.
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

/// Access modifiers map to `:Item.visibility`.
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

/// Re-extracting the same workspace is byte-stable (determinism — §4 I3).
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

// ---------------------------------------------------------------------------
// Self-dogfood: the on-disk ts-richer fixture (now also has methods)
// ---------------------------------------------------------------------------

fn richer_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ts-richer")
}

/// A method named the same as an interface must NOT shadow it: `implements`
/// resolution targets only type-kind items, so the `Handler` interface still
/// resolves even though `Worker` has a `Handler()` method (regression guard
/// for the 45-D0 / 45-B name-map interaction).
#[test]
fn method_named_like_interface_does_not_break_implements() {
    let (nodes, edges) = produce_one(
        r#"export interface Handler {}
export class Worker implements Handler {
    Handler(): void {}
}
"#,
    );
    // The method :Item exists...
    assert!(method_members(&nodes).contains(&"Worker::Handler".to_string()));
    // ...but the IMPLEMENTS edge Worker → Handler (interface) still resolves.
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

/// The ts-richer fixture's classes (Product, Order) now emit their methods.
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
