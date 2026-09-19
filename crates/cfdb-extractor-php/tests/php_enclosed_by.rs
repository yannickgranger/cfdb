use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

const COMPOSER: &str = r#"{
  "name": "cfdb/test",
  "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const SOURCE: &str = r#"<?php

namespace App;

class Fixture
{
    public function run($w): void
    {
        $w->inOneWrite(function () {
            $this->a();
        });

        $w->inOneWrite(fn () => $this->a());

        $w->inOneWrite($this->a());

        $w->inOneWrite(function () {
            $w->inOneWrite(function () {
                $this->a();
            });
        });

        $w->inOneWrite(function () {
            $f = function () {
                $this->a();
            };
        });
    }

    private function a(): void
    {
    }

    public function chained(): void
    {
        new self()->m(function () {
            $this->a();
        });
    }

    public function m($cb): void
    {
    }
}
"#;

fn produce() -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("composer.json"), COMPOSER).expect("write composer.json");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir -p src");
    fs::write(dir.path().join("src/Fixture.php"), SOURCE).expect("write Fixture.php");
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
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

fn line(n: &Node) -> i64 {
    n.props
        .get("line")
        .and_then(PropValue::as_i64)
        .expect("line")
}

fn call_site_at<'a>(nodes: &'a [Node], callee_path: &str, at_line: i64) -> &'a Node {
    let matches: Vec<&Node> = call_sites(nodes)
        .into_iter()
        .filter(|n| prop(n, "callee_path") == Some(callee_path) && line(n) == at_line)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one :CallSite callee_path={callee_path:?} at line {at_line}, found lines {:?}",
        call_sites(nodes)
            .iter()
            .filter(|n| prop(n, "callee_path") == Some(callee_path))
            .map(|n| line(n))
            .collect::<Vec<_>>()
    );
    matches[0]
}

fn enclosed_by<'a>(edges: &[Edge], nodes: &'a [Node], call_site: &Node) -> Option<&'a Node> {
    let targets: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::ENCLOSED_BY && e.src == call_site.id)
        .collect();
    assert!(
        targets.len() <= 1,
        "a :CallSite carries at most one ENCLOSED_BY edge (nearest only), got {}",
        targets.len()
    );
    let edge = targets.first()?;
    nodes.iter().find(|n| n.id == edge.dst)
}

fn owning_call_site_id<'a>(edges: &'a [Edge], argument: &Node) -> &'a str {
    edges
        .iter()
        .find(|e| e.label.as_str() == EdgeLabel::HAS_ARG && e.dst == argument.id)
        .map(|e| e.src.as_str())
        .expect("every :Argument is owned by exactly one :CallSite through HAS_ARG")
}

#[test]
fn a_call_site_inside_an_anonymous_function_argument_is_enclosed_by_that_argument() {
    let (nodes, edges) = produce();
    let inner = call_site_at(&nodes, "a", 10);
    let outer = call_site_at(&nodes, "inOneWrite", 9);

    let arg = enclosed_by(&edges, &nodes, inner)
        .expect("`$this->a()` inside `function () { ... }` carries ENCLOSED_BY");
    assert_eq!(
        owning_call_site_id(&edges, arg),
        outer.id,
        "the enclosing argument belongs to the `inOneWrite` call site the closure was passed to"
    );
    assert_eq!(
        arg.props.get("position").and_then(PropValue::as_i64),
        Some(1),
        "the closure is the first written argument on a member call, position 1 (0 is the receiver)"
    );
}

#[test]
fn a_call_site_inside_an_arrow_function_argument_is_enclosed_by_that_argument() {
    let (nodes, edges) = produce();
    let inner = call_site_at(&nodes, "a", 13);
    let outer = call_site_at(&nodes, "inOneWrite", 13);

    let arg = enclosed_by(&edges, &nodes, inner)
        .expect("`$this->a()` inside `fn () => ...` carries ENCLOSED_BY exactly as an anonymous_function argument does");
    assert_eq!(owning_call_site_id(&edges, arg), outer.id);
}

#[test]
fn an_eagerly_evaluated_argument_carries_no_enclosed_by_edge() {
    let (nodes, edges) = produce();
    let inner = call_site_at(&nodes, "a", 15);

    assert!(
        enclosed_by(&edges, &nodes, inner).is_none(),
        "`inOneWrite($this->a())` evaluates `a()` before `inOneWrite` runs — the call is not inside \
         the write, so no ENCLOSED_BY edge is emitted"
    );
}

#[test]
fn a_closure_nested_in_a_closure_argument_encloses_only_to_the_inner_one() {
    let (nodes, edges) = produce();
    let outer_write = call_site_at(&nodes, "inOneWrite", 17);
    let inner_write = call_site_at(&nodes, "inOneWrite", 18);
    let this_a = call_site_at(&nodes, "a", 19);

    let inner_write_enclosing = enclosed_by(&edges, &nodes, inner_write)
        .expect("the inner `inOneWrite(...)` call site is itself lexically inside the outer closure argument");
    assert_eq!(
        owning_call_site_id(&edges, inner_write_enclosing),
        outer_write.id,
        "the outer relationship is reached through the inner call site's own ENCLOSED_BY, not duplicated onto `a()`"
    );

    let this_a_enclosing = enclosed_by(&edges, &nodes, this_a)
        .expect("`$this->a()` is enclosed by the nearest closure argument");
    assert_eq!(
        owning_call_site_id(&edges, this_a_enclosing),
        inner_write.id,
        "nearest only: `$this->a()` is enclosed by the INNER inOneWrite's argument, not the outer's"
    );
}

#[test]
fn a_closure_assigned_to_a_variable_inside_an_argument_closure_resets_enclosure() {
    let (nodes, edges) = produce();
    let this_a = call_site_at(&nodes, "a", 25);

    assert!(
        enclosed_by(&edges, &nodes, this_a).is_none(),
        "`$f = function () {{ $this->a(); }};` is not itself an argument's own expression, so a call \
         site inside it carries no ENCLOSED_BY even though the assignment sits inside an outer \
         argument closure — it may run anywhere"
    );
}

#[test]
fn a_construction_used_as_a_chained_receiver_carries_no_enclosed_by_and_its_chained_argument_still_does(
) {
    let (nodes, edges) = produce();
    let construction = call_site_at(&nodes, "self", 36);
    let chained_call = call_site_at(&nodes, "m", 36);
    let inner = call_site_at(&nodes, "a", 37);

    assert_eq!(
        prop(construction, "kind"),
        Some("new"),
        "`new self()` is a construction call site, not a member call"
    );
    assert!(
        enclosed_by(&edges, &nodes, construction).is_none(),
        "the construction is the receiver of the chained `->m(...)` call, not the contents of an \
         argument closure — PHP 8.4's parenless `new self()->m(...)` visits the receiver through \
         the same general per-child loop as any other member-call receiver, unaffected by the \
         argument-dispatch that only fires inside `->m(...)`'s own `arguments`"
    );
    assert!(
        enclosed_by(&edges, &nodes, chained_call).is_none(),
        "`m(...)` itself sits at the top of `chained()`'s body, inside no argument closure"
    );

    let arg = enclosed_by(&edges, &nodes, inner).expect(
        "`$this->a()` inside the closure argument of the chained `->m(...)` call carries \
         ENCLOSED_BY exactly as it would if the receiver were a plain variable",
    );
    assert_eq!(
        owning_call_site_id(&edges, arg),
        chained_call.id,
        "the enclosing argument belongs to `m`'s call site, not to the `new self()` construction \
         beside it"
    );
    assert_eq!(
        prop(inner, "caller_qname"),
        Some(r"App\Fixture::chained"),
        "caller_qname still names the enclosing method, same as every other case in this file"
    );
}

#[test]
fn caller_qname_is_unaffected_by_enclosure() {
    let (nodes, _edges) = produce();
    for at_line in [10, 13, 15, 19, 25] {
        let cs = call_site_at(&nodes, "a", at_line);
        assert_eq!(
            prop(cs, "caller_qname"),
            Some(r"App\Fixture::run"),
            "line {at_line}: ENCLOSED_BY is the only new information this slice adds — \
             caller_qname keeps naming the enclosing method regardless of how many closures deep \
             the call site sits"
        );
    }
}
