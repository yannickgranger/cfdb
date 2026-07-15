//! `PhpProducer` `:CallSite` + `INVOKES_AT` + resolved `CALLS`
//! (RFC-045 slice 45-C / issue #465).
//!
//! Exercises the recursive body-walk (RFC-045 §3.4): every call expression
//! in a fn/method body emits a `:CallSite` (full Rust-parity prop set,
//! `resolver = "tree-sitter-php"`) and an `INVOKES_AT` edge `:Item{caller}
//! -> :CallSite`. A `CALLS` edge `:Item{caller} -> :Item{callee}` is emitted
//! only when the callee resolves to an in-workspace `:Item` per the §3.4
//! scope table. `new MyClass()` is not a call site. "Callers of X" is
//! answered via `:CallSite.callee_path` + `INVOKES_AT`.

use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Build a single-file workspace whose one file is `body` and return facts.
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

/// CallSite by its `callee_path` prop (asserts exactly one match).
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

/// `(src_id, dst_id)` of every CALLS edge (Item→Item), sorted.
fn calls_pairs(edges: &[Edge]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = edges_of(edges, EdgeLabel::CALLS)
        .iter()
        .map(|e| (e.src.clone(), e.dst.clone()))
        .collect();
    pairs.sort();
    pairs
}

/// `true`/`false` value of a `:CallSite`'s `callee_resolved` bool prop.
fn resolved(n: &Node) -> Option<bool> {
    n.props.get("callee_resolved").and_then(PropValue::as_bool)
}

// ---------------------------------------------------------------------------
// Full prop set + INVOKES_AT direction
// ---------------------------------------------------------------------------

/// A `:CallSite` carries the full Rust-parity 9-prop set, and `INVOKES_AT`
/// flows `:Item{caller} -> :CallSite` (the corrected direction).
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

    // INVOKES_AT: Item(App\run) -> CallSite(this).
    let invokes: Vec<&Edge> = edges_of(&edges, EdgeLabel::INVOKES_AT);
    assert_eq!(invokes.len(), 1, "one INVOKES_AT for the single call");
    assert_eq!(
        invokes[0].src,
        item_id(r"App\run"),
        "INVOKES_AT src is the caller :Item"
    );
    assert_eq!(invokes[0].dst, cs.id, "INVOKES_AT dst is the :CallSite");
}

// ---------------------------------------------------------------------------
// §3.4 scope table — resolution per call form
// ---------------------------------------------------------------------------

/// Free function: `helper()` resolves to `<ns>\helper`; `missing()` does not.
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
    // Only the resolved call yields a CALLS edge.
    assert_eq!(
        calls_pairs(&edges),
        vec![(item_id(r"App\run"), item_id(r"App\helper"))],
    );
}

/// `\Ns\foo()` (qualified) resolves to `Ns\foo` (leading `\` stripped);
/// `callee_last_segment` is `foo`.
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

/// `C::bar()` (static call) resolves to `<ns>\C::bar`.
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

/// `self::`/`static::` bind to the enclosing class (resolved); `parent::`
/// is unresolved (no superclass edge this RFC).
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
    // self::a() and static::a() both → callee_path "App\Svc::a", resolved.
    // (Two calls, same callee_path → occurrence counter gives distinct ids.)
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
    // distinct ids despite identical callee_path.
    assert_ne!(svc_a[0].id, svc_a[1].id);

    // parent::a() → unresolved, callee_path "parent::a".
    let parent = call_site_by_path(&nodes, "parent::a");
    assert_eq!(resolved(parent), Some(false));

    // CALLS: two edges, both App\Svc::b -> App\Svc::a (self + static); none for parent.
    assert_eq!(
        calls_pairs(&edges),
        vec![
            (item_id(r"App\Svc::b"), item_id(r"App\Svc::a")),
            (item_id(r"App\Svc::b"), item_id(r"App\Svc::a")),
        ],
    );
}

/// Instance / nullsafe / dynamic dispatch → method name only, never
/// resolved, never a `CALLS` edge.
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

// ---------------------------------------------------------------------------
// Structural cases
// ---------------------------------------------------------------------------

/// Nested call `foo(bar())` emits 2 call sites (outer + inner).
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

/// Two calls to the same callee in one body get distinct ids (per-caller,
/// per-callee_path occurrence counter).
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

/// `new MyClass()` (object_creation_expression) is NOT a call site (§6
/// non-goal); a call inside its arguments still is.
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

/// Re-extracting the same tree is byte-stable (determinism — §4 I3).
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

// ---------------------------------------------------------------------------
// Self-dogfood: the on-disk php-calls fixture (resolvable + dynamic calls)
// ---------------------------------------------------------------------------

fn calls_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/php-calls")
}

/// Extract the in-tree `php-calls` fixture and assert the call-graph shape:
/// `Calculator::compute` makes 5 calls — `self::add`, `Calculator::add`,
/// `helper(...)` (all in-workspace → 3 `CALLS`), `$this->add(...)` and
/// `missing(...)` (unresolved → no `CALLS`). Five `:CallSite`s, five
/// `INVOKES_AT` (Item→CallSite), three `CALLS`.
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

    // INVOKES_AT is Item->CallSite, all sourced at the caller :Item.
    for e in edges_of(&edges, EdgeLabel::INVOKES_AT) {
        assert_eq!(e.src, item_id(r"App\Calculator::compute"));
        assert!(
            nodes
                .iter()
                .any(|n| n.id == e.dst && n.label.as_str() == Label::CALL_SITE),
            "INVOKES_AT dst must be a :CallSite",
        );
    }

    // Three resolved CALLS: ->Calculator::add (self:: + Calculator::add) + ->helper.
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

    // "callers of helper" via callee_path + INVOKES_AT resolves to compute().
    let helper_cs = call_site_by_path(&nodes, "helper");
    assert_eq!(resolved(helper_cs), Some(true));
    assert_eq!(
        prop(helper_cs, "caller_qname"),
        Some(r"App\Calculator::compute")
    );
}

/// The php-calls fixture re-extracts byte-stably.
#[test]
fn calls_fixture_is_deterministic() {
    let run1 = PhpProducer.produce(&calls_fixture_root()).expect("run1");
    let run2 = PhpProducer.produce(&calls_fixture_root()).expect("run2");
    assert_eq!(format!("{run1:?}"), format!("{run2:?}"));
}
