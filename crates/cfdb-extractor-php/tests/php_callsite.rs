use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

fn produce_php(files: &[(&str, &str)]) -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name":"cfdb/test","type":"library"}"#,
    )
    .expect("write composer.json");
    for (rel, src) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir -p");
        }
        fs::write(&path, src).expect("write php source");
    }
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
}

fn produce_one(body: &str) -> (Vec<Node>, Vec<Edge>) {
    produce_php(&[("src/m.php", body)])
}

fn call_sites(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .collect()
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

fn call_site_by_path<'a>(nodes: &'a [Node], callee_path: &str) -> &'a Node {
    let matches: Vec<&Node> = call_sites(nodes)
        .into_iter()
        .filter(|n| prop(n, "callee_path") == Some(callee_path))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one :CallSite with callee_path={callee_path:?}, got {}",
        matches.len(),
    );
    matches[0]
}

fn edges_of<'a>(edges: &'a [Edge], label: &str) -> Vec<&'a Edge> {
    edges.iter().filter(|e| e.label.as_str() == label).collect()
}

fn item_id(qname: &str) -> String {
    format!("item:{qname}")
}

fn calls_pairs(edges: &[Edge]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = edges_of(edges, EdgeLabel::CALLS)
        .iter()
        .map(|e| (e.src.clone(), e.dst.clone()))
        .collect();
    pairs.sort();
    pairs
}

fn resolved(n: &Node) -> Option<bool> {
    n.props.get("callee_resolved").and_then(PropValue::as_bool)
}

#[test]
fn call_site_full_prop_set_and_invokes_at_direction() {
    let (nodes, edges) = produce_one(
        r#"<?php
namespace App;
function helper(): void {}
function run(): void {
    helper();
}
"#,
    );

    let cs = call_site_by_path(&nodes, "helper");
    assert_eq!(prop(cs, "caller_qname"), Some(r"App\run"));
    assert_eq!(prop(cs, "callee_path"), Some("helper"));
    assert_eq!(prop(cs, "callee_last_segment"), Some("helper"));
    assert_eq!(prop(cs, "kind"), Some("call"));
    assert_eq!(prop(cs, "file"), Some("src/m.php"));
    assert_eq!(
        cs.props.get("line").and_then(PropValue::as_i64),
        Some(5),
        "helper() is on line 5"
    );
    assert_eq!(
        cs.props.get("is_test").and_then(PropValue::as_bool),
        Some(false)
    );
    assert_eq!(prop(cs, "resolver"), Some("tree-sitter-php"));
    assert_eq!(
        cs.props.get("callee_resolved").and_then(PropValue::as_bool),
        Some(true),
        "App\\helper is in-workspace → resolved"
    );

    let invokes: Vec<&Edge> = edges_of(&edges, EdgeLabel::INVOKES_AT);
    assert_eq!(invokes.len(), 1, "one INVOKES_AT for the single call");
    assert_eq!(
        invokes[0].src,
        item_id(r"App\run"),
        "INVOKES_AT src is the caller :Item"
    );
    assert_eq!(invokes[0].dst, cs.id, "INVOKES_AT dst is the :CallSite");
}

#[test]
fn free_function_calls_resolve_against_current_namespace() {
    let (nodes, edges) = produce_one(
        r#"<?php
namespace App;
function helper(): void {}
function run(): void {
    helper();
    missing();
}
"#,
    );
    assert_eq!(resolved(call_site_by_path(&nodes, "helper")), Some(true));
    assert_eq!(resolved(call_site_by_path(&nodes, "missing")), Some(false));
    assert_eq!(
        calls_pairs(&edges),
        vec![(item_id(r"App\run"), item_id(r"App\helper"))],
    );
}

#[test]
fn qualified_function_call_resolves_absolute() {
    let (nodes, edges) = produce_php(&[
        (
            "src/ns.php",
            r#"<?php
namespace Ns;
function foo(): void {}
"#,
        ),
        (
            "src/app.php",
            r#"<?php
namespace App;
function run(): void {
    \Ns\foo();
}
"#,
        ),
    ]);
    let cs = call_site_by_path(&nodes, r"\Ns\foo");
    assert_eq!(prop(cs, "callee_last_segment"), Some("foo"));
    assert_eq!(
        cs.props.get("callee_resolved").and_then(PropValue::as_bool),
        Some(true)
    );
    assert_eq!(
        calls_pairs(&edges),
        vec![(item_id(r"App\run"), item_id(r"Ns\foo"))],
    );
}

#[test]
fn scoped_static_call_resolves_class_method() {
    let (nodes, edges) = produce_one(
        r#"<?php
namespace App;
class C {
    public function bar(): void {}
}
function run(): void {
    C::bar();
    D::bar();
}
"#,
    );
    assert_eq!(resolved(call_site_by_path(&nodes, "C::bar")), Some(true));
    assert_eq!(resolved(call_site_by_path(&nodes, "D::bar")), Some(false));
    assert_eq!(
        calls_pairs(&edges),
        vec![(item_id(r"App\run"), item_id(r"App\C::bar"))],
    );
}

#[test]
fn relative_scope_calls_self_static_parent() {
    let (nodes, edges) = produce_one(
        r#"<?php
namespace App;
class Svc {
    public function a(): void {}
    public function b(): void {
        self::a();
        static::a();
        parent::a();
    }
}
"#,
    );
    let svc_a: Vec<&Node> = call_sites(&nodes)
        .into_iter()
        .filter(|n| prop(n, "callee_path") == Some(r"App\Svc::a"))
        .collect();
    assert_eq!(
        svc_a.len(),
        2,
        "self::a() + static::a() → 2 call sites to App\\Svc::a"
    );
    assert!(svc_a.iter().all(|n| resolved(n) == Some(true)));
    assert_ne!(svc_a[0].id, svc_a[1].id);

    let parent = call_site_by_path(&nodes, "parent::a");
    assert_eq!(resolved(parent), Some(false));

    assert_eq!(
        calls_pairs(&edges),
        vec![
            (item_id(r"App\Svc::b"), item_id(r"App\Svc::a")),
            (item_id(r"App\Svc::b"), item_id(r"App\Svc::a")),
        ],
    );
}

#[test]
fn member_nullsafe_and_dynamic_calls_are_unresolved() {
    let (nodes, edges) = produce_one(
        r#"<?php
namespace App;
function run($x, $cls): void {
    $x->foo();
    $x?->bar();
    $cls::baz();
}
"#,
    );
    for path in ["foo", "bar", "baz"] {
        assert_eq!(
            resolved(call_site_by_path(&nodes, path)),
            Some(false),
            "{path} is dynamic/instance dispatch → unresolved"
        );
    }
    assert!(
        edges_of(&edges, EdgeLabel::CALLS).is_empty(),
        "no CALLS for instance/dynamic dispatch"
    );
}

#[test]
fn nested_calls_emit_two_call_sites() {
    let (nodes, _edges) = produce_one(
        r#"<?php
namespace App;
function run(): void {
    foo(bar());
}
"#,
    );
    let paths: std::collections::BTreeSet<&str> = call_sites(&nodes)
        .iter()
        .filter_map(|n| prop(n, "callee_path"))
        .collect();
    assert_eq!(
        paths,
        ["bar", "foo"].into_iter().collect(),
        "foo(bar()) → call sites for both foo and bar"
    );
}

#[test]
fn repeated_calls_get_distinct_ids() {
    let (nodes, _edges) = produce_one(
        r#"<?php
namespace App;
function run(): void {
    foo();
    foo();
}
"#,
    );
    let foo: Vec<&Node> = call_sites(&nodes)
        .into_iter()
        .filter(|n| prop(n, "callee_path") == Some("foo"))
        .collect();
    assert_eq!(foo.len(), 2);
    assert_ne!(
        foo[0].id, foo[1].id,
        "occurrence counter must disambiguate the ids"
    );
    assert!(foo.iter().any(|n| n.id.ends_with(":0")));
    assert!(foo.iter().any(|n| n.id.ends_with(":1")));
}

#[test]
fn object_creation_is_not_a_call_site() {
    let (nodes, _edges) = produce_one(
        r#"<?php
namespace App;
function run(): void {
    new MyClass();
    new Wrapper(inner());
}
"#,
    );
    let paths: Vec<&str> = call_sites(&nodes)
        .iter()
        .filter_map(|n| prop(n, "callee_path"))
        .collect();
    assert_eq!(
        paths,
        vec!["inner"],
        "no CallSite for `new ...`; only the `inner()` argument call",
    );
}

#[test]
fn re_extract_is_deterministic() {
    let files = &[(
        "src/m.php",
        r#"<?php
namespace App;
class C { public function bar(): void {} }
function run(): void {
    C::bar();
    foo();
    foo();
    $x->dyn();
}
"#,
    )];
    let run1 = produce_php(files);
    let run2 = produce_php(files);
    assert_eq!(format!("{run1:?}"), format!("{run2:?}"));
}

fn calls_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/php-calls")
}

#[test]
fn calls_fixture_call_graph_shape() {
    let (nodes, edges) = PhpProducer
        .produce(&calls_fixture_root())
        .expect("produce php-calls fixture");

    assert_eq!(
        call_sites(&nodes).len(),
        5,
        "5 call expressions in compute()"
    );
    assert_eq!(edges_of(&edges, EdgeLabel::INVOKES_AT).len(), 5);

    for e in edges_of(&edges, EdgeLabel::INVOKES_AT) {
        assert_eq!(e.src, item_id(r"App\Calculator::compute"));
        assert!(
            nodes
                .iter()
                .any(|n| n.id == e.dst && n.label.as_str() == Label::CALL_SITE),
            "INVOKES_AT dst must be a :CallSite",
        );
    }

    assert_eq!(
        calls_pairs(&edges),
        vec![
            (
                item_id(r"App\Calculator::compute"),
                item_id(r"App\Calculator::add")
            ),
            (
                item_id(r"App\Calculator::compute"),
                item_id(r"App\Calculator::add")
            ),
            (item_id(r"App\Calculator::compute"), item_id(r"App\helper")),
        ],
    );

    let helper_cs = call_site_by_path(&nodes, "helper");
    assert_eq!(resolved(helper_cs), Some(true));
    assert_eq!(
        prop(helper_cs, "caller_qname"),
        Some(r"App\Calculator::compute")
    );
}

#[test]
fn calls_fixture_is_deterministic() {
    let run1 = PhpProducer.produce(&calls_fixture_root()).expect("run1");
    let run2 = PhpProducer.produce(&calls_fixture_root()).expect("run2");
    assert_eq!(format!("{run1:?}"), format!("{run2:?}"));
}

#[test]
fn a_scoped_call_through_a_use_import_resolves_to_the_imported_class() {
    let (nodes, edges) = produce_php(&[
        (
            "src/Helpers.php",
            "<?php\nnamespace App\\Support;\nclass Clock { public static function now() {} }\n",
        ),
        (
            "src/Enrolling.php",
            "<?php\nnamespace App\\Enrolment;\nuse App\\Support\\Clock;\nclass Enrolling { public function at() { Clock::now(); } }\n",
        ),
    ]);
    let site = call_site_by_path(&nodes, "Clock::now");
    assert_eq!(
        resolved(site),
        Some(true),
        "the scoped callee goes through the same import table before the CALLS decision"
    );
    assert_eq!(
        calls_pairs(&edges),
        vec![(
            item_id("App\\Enrolment\\Enrolling::at"),
            item_id("App\\Support\\Clock::now")
        )]
    );
}

#[test]
fn a_scoped_call_to_an_out_of_workspace_class_still_resolves_to_nothing() {
    let (nodes, edges) = produce_php(&[(
        "src/Enrolling.php",
        "<?php\nnamespace App\\Enrolment;\nuse Symfony\\Component\\Clock\\Clock;\nclass Enrolling { public function at() { Clock::now(); } }\n",
    )]);
    let site = call_site_by_path(&nodes, "Clock::now");
    assert_eq!(resolved(site), Some(false));
    assert!(
        calls_pairs(&edges).is_empty(),
        "closed-world holds: the import resolves the name, the workspace decides the edge"
    );
}
