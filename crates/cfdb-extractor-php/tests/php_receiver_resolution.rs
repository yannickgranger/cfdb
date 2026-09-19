use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::EdgeLabel;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

fn produce_php(files: &[(&str, &str)]) -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name":"cfdb/test","type":"library","autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer.json");
    for (rel, src) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir -p fixture subdir");
        }
        fs::write(&path, src).expect("write php source");
    }
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
}

fn item_id(qname: &str) -> String {
    format!("item:{qname}")
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

fn call_site_by_path<'a>(nodes: &'a [Node], callee_path: &str) -> &'a Node {
    let matches: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == "CallSite" && prop(n, "callee_path") == Some(callee_path))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one :CallSite with callee_path={callee_path:?}, got {}",
        matches.len(),
    );
    matches[0]
}

fn callee_resolved(n: &Node) -> Option<bool> {
    n.props.get("callee_resolved").and_then(PropValue::as_bool)
}

fn calls_edge<'a>(edges: &'a [Edge], src: &str, dst: &str) -> Option<&'a Edge> {
    edges.iter().find(|e| {
        e.label.as_str() == EdgeLabel::CALLS && e.src == item_id(src) && e.dst == item_id(dst)
    })
}

fn edge_resolved(e: &Edge) -> Option<bool> {
    e.props.get("resolved").and_then(PropValue::as_bool)
}

#[test]
fn this_dispatches_to_a_method_declared_on_the_enclosing_class() {
    let (nodes, edges) = produce_php(&[(
        "src/Calculator.php",
        r#"<?php
namespace App;
class Calculator {
    public function add(int $a, int $b): int { return $a + $b; }
    public function compute(): int { return $this->add(1, 2); }
}
"#,
    )]);
    let cs = call_site_by_path(&nodes, "add");
    assert_eq!(callee_resolved(cs), Some(true));
    let edge = calls_edge(&edges, r"App\Calculator::compute", r"App\Calculator::add")
        .expect("CALLS edge from compute to add");
    assert_eq!(edge_resolved(edge), Some(true));
}

#[test]
fn this_dispatches_to_a_method_declared_on_an_extends_ancestor() {
    let (nodes, edges) = produce_php(&[(
        "src/Types.php",
        r#"<?php
namespace App;
class Base {
    public function add(int $a, int $b): int { return $a + $b; }
}
class Calculator extends Base {
    public function compute(): int { return $this->add(1, 2); }
}
"#,
    )]);
    let cs = call_site_by_path(&nodes, "add");
    assert_eq!(callee_resolved(cs), Some(true));
    let edge = calls_edge(&edges, r"App\Calculator::compute", r"App\Base::add")
        .expect("CALLS edge from compute to the ancestor's add");
    assert_eq!(edge_resolved(edge), Some(true));
}

#[test]
fn this_calling_an_undeclared_method_is_unresolved() {
    let (nodes, edges) = produce_php(&[(
        "src/Calculator.php",
        r#"<?php
namespace App;
class Calculator {
    public function compute(): int { return $this->missing(1, 2); }
}
"#,
    )]);
    let cs = call_site_by_path(&nodes, "missing");
    assert_eq!(callee_resolved(cs), Some(false));
    assert!(edges.iter().all(|e| e.label.as_str() != EdgeLabel::CALLS));
}

#[test]
fn this_property_dispatches_through_a_constructor_promoted_field() {
    let (nodes, edges) = produce_php(&[(
        "src/Service.php",
        r#"<?php
namespace App;
class Port {
    public function send(): void {}
}
class Service {
    public function __construct(private Port $port) {}
    public function run(): void { $this->port->send(); }
}
"#,
    )]);
    let cs = call_site_by_path(&nodes, "send");
    assert_eq!(callee_resolved(cs), Some(true));
    let edge = calls_edge(&edges, r"App\Service::run", r"App\Port::send")
        .expect("CALLS edge from run to Port::send through the promoted field");
    assert_eq!(edge_resolved(edge), Some(true));
}

#[test]
fn this_property_dispatches_through_a_nullable_field_ignoring_the_null_arm() {
    let (nodes, edges) = produce_php(&[(
        "src/Service.php",
        r#"<?php
namespace App;
class Port {
    public function send(): void {}
}
class Service {
    private ?Port $port = null;
    public function run(): void { $this->port->send(); }
}
"#,
    )]);
    let cs = call_site_by_path(&nodes, "send");
    assert_eq!(callee_resolved(cs), Some(true));
    calls_edge(&edges, r"App\Service::run", r"App\Port::send")
        .expect("CALLS edge from run to Port::send through the nullable field");
}

#[test]
fn this_property_typed_as_a_union_of_two_classes_is_unresolved() {
    let (nodes, edges) = produce_php(&[(
        "src/Service.php",
        r#"<?php
namespace App;
class Port {
    public function send(): void {}
}
class Other {
    public function send(): void {}
}
class Service {
    private Port|Other $port;
    public function run(): void { $this->port->send(); }
}
"#,
    )]);
    let cs = call_site_by_path(&nodes, "send");
    assert_eq!(callee_resolved(cs), Some(false));
    assert!(edges.iter().all(|e| e.label.as_str() != EdgeLabel::CALLS));
}

#[test]
fn this_property_typed_to_an_out_of_workspace_vendor_class_is_unresolved() {
    let (nodes, edges) = produce_php(&[(
        "src/Service.php",
        r#"<?php
namespace App;
use Vendor\Logger;
class Service {
    private Logger $logger;
    public function run(): void { $this->logger->info("x"); }
}
"#,
    )]);
    let cs = call_site_by_path(&nodes, "info");
    assert_eq!(callee_resolved(cs), Some(false));
    assert!(edges.iter().all(|e| e.label.as_str() != EdgeLabel::CALLS));
}

#[test]
fn a_local_variable_receiver_is_unresolved_receiver_type_inference_is_out_of_scope() {
    let (nodes, edges) = produce_php(&[(
        "src/Service.php",
        r#"<?php
namespace App;
class Port {
    public function send(): void {}
}
class Service {
    public function run(): void {
        $port = new Port();
        $port->send();
    }
}
"#,
    )]);
    let cs = call_site_by_path(&nodes, "send");
    assert_eq!(callee_resolved(cs), Some(false));
    assert!(calls_edge(&edges, r"App\Service::run", r"App\Port::send").is_none());
}
