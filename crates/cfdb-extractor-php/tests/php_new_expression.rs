use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

const COMPOSER: &str = r#"{
  "name": "cfdb/test",
  "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const COURSE_WITH_CONSTRUCTOR: &str = r#"<?php

namespace App;

class Course
{
    public function __construct()
    {
    }
}
"#;

const COURSE_WITHOUT_CONSTRUCTOR: &str = r#"<?php

namespace App;

class Bare
{
}
"#;

const CALLER: &str = r#"<?php

namespace App;

use App\Course as C;

class Caller extends Base
{
    public function __construct()
    {
    }

    public function build(): void
    {
        new \DateTimeImmutable('now');
        new Course();
        new Bare();
        new C();
        new self();
        new static();
        new parent();
        new class {
        };
        $cls = 'App\Course';
        new $cls();
        $f = null;
        new ($f->t())();
        date('Y');
    }
}
"#;

fn produce() -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("composer.json"), COMPOSER).expect("write composer.json");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir -p src");
    fs::write(dir.path().join("src/Course.php"), COURSE_WITH_CONSTRUCTOR).expect("write Course");
    fs::write(dir.path().join("src/Bare.php"), COURSE_WITHOUT_CONSTRUCTOR).expect("write Bare");
    fs::write(dir.path().join("src/Caller.php"), CALLER).expect("write Caller");
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
}

fn call_sites(nodes: &[Node]) -> Vec<(&str, &str, bool)> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == "CallSite")
        .filter(|n| {
            n.props.get("caller_qname").and_then(PropValue::as_str) == Some("App\\Caller::build")
        })
        .map(|n| {
            (
                n.props
                    .get("callee_path")
                    .and_then(PropValue::as_str)
                    .expect("callee_path"),
                n.props
                    .get("kind")
                    .and_then(PropValue::as_str)
                    .expect("kind"),
                matches!(n.props.get("callee_resolved"), Some(PropValue::Bool(true))),
            )
        })
        .collect()
}

#[test]
fn every_named_class_position_is_one_construction_site_and_no_other_position_is() {
    let (nodes, _) = produce();
    let mut sites = call_sites(&nodes);
    sites.sort();

    assert_eq!(
        sites,
        vec![
            ("Bare", "new", false),
            ("C", "new", true),
            ("Course", "new", true),
            ("\\DateTimeImmutable", "new", false),
            ("date", "call", false),
            ("parent", "new", false),
            ("self", "new", true),
            ("static", "new", true),
            ("t", "call", false),
        ],
        "a construction of a named class is a call site carrying the name as written. \
         `new class {{}}`, `new $cls()` and `new ($f->t())()` name no class and yield no \
         construction, the same silence every unhandled shape already gets — the `t` row is \
         the member call INSIDE the third of those, which was always a call site and stays one."
    );
}

#[test]
fn a_construction_resolves_to_the_class_constructor_and_a_sibling_call_is_unchanged() {
    let (nodes, edges) = produce();

    let calls: Vec<(&str, &str)> = edges
        .iter()
        .filter(|e| e.label.as_str() == "CALLS")
        .filter(|e| e.src == "item:App\\Caller::build")
        .map(|e| (e.src.as_str(), e.dst.as_str()))
        .collect();

    assert!(
        calls.contains(&("item:App\\Caller::build", "item:App\\Course::__construct")),
        "a construction whose class declares a constructor CALLS that constructor: {calls:?}"
    );
    assert!(
        !calls.iter().any(|(_, dst)| dst.contains("Bare")),
        "a class declaring no constructor resolves to nothing and gets no CALLS: {calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|(_, dst)| dst.contains("DateTimeImmutable")),
        "a vendor class is not in the graph, so the site stands alone: {calls:?}"
    );

    let sites = call_sites(&nodes);
    assert!(
        sites.contains(&("date", "call", false)),
        "the fourth kind member does not disturb the first: {sites:?}"
    );
}

#[test]
fn new_self_and_new_static_resolve_to_the_enclosing_class_not_to_a_namespaced_self() {
    let (_, edges) = produce();
    let to_construct: Vec<&str> = edges
        .iter()
        .filter(|e| e.label.as_str() == "CALLS")
        .map(|e| e.dst.as_str())
        .filter(|dst| dst.ends_with("__construct"))
        .collect();

    assert!(
        to_construct.contains(&"item:App\\Caller::__construct"),
        "`new self()` and `new static()` resolve to the enclosing class's constructor; \
         routing them through `qualify` would give `App\\self`, a qname that can never match: {to_construct:?}"
    );
    assert!(
        !to_construct.iter().any(|dst| dst.contains("\\self")
            || dst.contains("\\static")
            || dst.contains("parent::")),
        "no relative keyword survives into a resolved target: {to_construct:?}"
    );
}
