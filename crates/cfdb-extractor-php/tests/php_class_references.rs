use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::EdgeLabel;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

const TARGETS: &[(&str, &str)] = &[
    (
        "src/B/Named.php",
        "<?php\nnamespace App\\B;\nfinal class Named\n{\n    public const EURO = 'EUR';\n    public static int $count = 0;\n    public static function make(): self { return new self(); }\n}\n",
    ),
    (
        "src/B/Refused.php",
        "<?php\nnamespace App\\B;\nfinal class Refused extends \\Exception {}\n",
    ),
    (
        "src/B/Other.php",
        "<?php\nnamespace App\\B;\nfinal class Other {}\n",
    ),
    (
        "src/B/Shared.php",
        "<?php\nnamespace App\\B;\ntrait Shared {}\n",
    ),
];

fn produce_with(source: &str) -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name":"cfdb/test","type":"library","autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer.json");
    let mut files: Vec<(&str, &str)> = TARGETS.to_vec();
    files.push(("src/A/Source.php", source));
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

fn prop<'a>(edge: &'a Edge, key: &str) -> Option<&'a PropValue> {
    edge.props.get(key)
}

fn references(edges: &[Edge]) -> Vec<(String, String, String, i64)> {
    let mut out: Vec<(String, String, String, i64)> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::REFERS_TO)
        .map(|e| {
            (
                e.src.clone(),
                e.dst.clone(),
                prop(e, "how")
                    .and_then(PropValue::as_str)
                    .unwrap_or_default()
                    .to_string(),
                match prop(e, "line") {
                    Some(PropValue::Int(line)) => *line,
                    _ => -1,
                },
            )
        })
        .collect();
    out.sort();
    out
}

fn one(src: &str, dst: &str, how: &str, line: i64) -> (String, String, String, i64) {
    (
        format!("item:{src}"),
        format!("item:{dst}"),
        how.to_string(),
        line,
    )
}

fn in_method(body: &str) -> String {
    format!("<?php\nnamespace App\\A;\nfinal class Source\n{{\n    public function run(object $a): mixed\n    {{\n{body}\n    }}\n}}\n")
}

#[test]
fn a_construction_of_a_class_without_a_constructor_is_a_new_reference_and_no_call() {
    let (_, edges) = produce_with(&in_method("        return new \\App\\B\\Other();"));
    assert_eq!(
        references(&edges),
        vec![one("App\\A\\Source::run", "App\\B\\Other", "new", 7)]
    );
    assert!(
        !edges
            .iter()
            .any(|e| e.label.as_str() == EdgeLabel::CALLS && e.dst.contains("App\\B\\Other")),
        "a constructor-less class yields no CALLS"
    );
}

#[test]
fn a_class_constant_and_a_class_name_are_scope_references() {
    let (_, edges) = produce_with(&in_method(
        "        $e = \\App\\B\\Named::EURO;\n        return \\App\\B\\Other::class;",
    ));
    assert_eq!(
        references(&edges),
        vec![
            one("App\\A\\Source::run", "App\\B\\Named", "scope", 7),
            one("App\\A\\Source::run", "App\\B\\Other", "scope", 8),
        ]
    );
}

#[test]
fn a_static_property_is_a_scope_reference() {
    let (_, edges) = produce_with(&in_method("        return \\App\\B\\Named::$count;"));
    assert_eq!(
        references(&edges),
        vec![one("App\\A\\Source::run", "App\\B\\Named", "scope", 7)]
    );
}

#[test]
fn an_instance_test_is_an_instanceof_reference() {
    let (_, edges) = produce_with(&in_method("        return $a instanceof \\App\\B\\Other;"));
    assert_eq!(
        references(&edges),
        vec![one("App\\A\\Source::run", "App\\B\\Other", "instanceof", 7)]
    );
}

#[test]
fn every_caught_type_is_one_catch_reference() {
    let (_, edges) = produce_with(&in_method(
        "        try { return 1; } catch (\\App\\B\\Refused|\\App\\B\\Other $e) { return 2; }",
    ));
    assert_eq!(
        references(&edges),
        vec![
            one("App\\A\\Source::run", "App\\B\\Other", "catch", 7),
            one("App\\A\\Source::run", "App\\B\\Refused", "catch", 7),
        ]
    );
}

#[test]
fn a_closure_parameter_and_an_arrow_function_return_are_closure_type_references_of_the_method() {
    let (_, edges) = produce_with(&in_method(
        "        $f = function (\\App\\B\\Other $o) { return $o; };\n        return fn (): ?\\App\\B\\Named => null;",
    ));
    assert_eq!(
        references(&edges),
        vec![
            one("App\\A\\Source::run", "App\\B\\Named", "closure_type", 8),
            one("App\\A\\Source::run", "App\\B\\Other", "closure_type", 7),
        ]
    );
}

#[test]
fn a_trait_use_is_a_trait_use_reference_of_the_class() {
    let (_, edges) = produce_with(
        "<?php\nnamespace App\\A;\nfinal class Source\n{\n    use \\App\\B\\Shared;\n}\n",
    );
    assert_eq!(
        references(&edges),
        vec![one("App\\A\\Source", "App\\B\\Shared", "trait_use", 5)]
    );
}

#[test]
fn a_constant_value_and_a_property_default_name_their_class_from_the_constant_and_the_class() {
    let (_, edges) = produce_with(
        "<?php\nnamespace App\\A;\nfinal class Source\n{\n    public const CURRENCY = \\App\\B\\Named::EURO;\n    private string $kind = \\App\\B\\Other::class;\n}\n",
    );
    assert_eq!(
        references(&edges),
        vec![
            one("App\\A\\Source", "App\\B\\Other", "scope", 6),
            one("App\\A\\Source::CURRENCY", "App\\B\\Named", "scope", 5),
        ]
    );
}

#[test]
fn a_parameter_default_names_its_class_from_the_method() {
    let (_, edges) = produce_with(
        "<?php\nnamespace App\\A;\nfinal class Source\n{\n    public function run(string $c = \\App\\B\\Named::EURO): void {}\n}\n",
    );
    assert_eq!(
        references(&edges),
        vec![one("App\\A\\Source::run", "App\\B\\Named", "scope", 5)]
    );
}

#[test]
fn a_namespace_alias_resolves_to_the_class_it_names() {
    let (_, edges) = produce_with(
        "<?php\nnamespace App\\A;\nuse App\\B as P;\nfinal class Source\n{\n    public function run(): string { return P\\Other::class; }\n}\n",
    );
    assert_eq!(
        references(&edges),
        vec![one("App\\A\\Source::run", "App\\B\\Other", "scope", 6)]
    );
}

#[test]
fn two_sites_of_one_source_target_and_how_are_one_edge_on_the_first_line() {
    let (_, edges) = produce_with(&in_method(
        "        $x = $a instanceof \\App\\B\\Other;\n        return $a instanceof \\App\\B\\Other;",
    ));
    assert_eq!(
        references(&edges),
        vec![one("App\\A\\Source::run", "App\\B\\Other", "instanceof", 7)]
    );
}

#[test]
fn a_function_body_names_its_class_from_the_function() {
    let (_, edges) = produce_with(
        "<?php\nnamespace App\\A;\nfunction helper(object $a): bool\n{\n    return $a instanceof \\App\\B\\Other;\n}\n",
    );
    assert_eq!(
        references(&edges),
        vec![one("App\\A\\helper", "App\\B\\Other", "instanceof", 5)]
    );
}

#[test]
fn what_names_no_in_workspace_class_leaves_no_edge_and_mints_no_item() {
    let source = "<?php\nnamespace App\\A;\nfinal class Source\n{\n    /** @param \\App\\B\\Other $a */\n    public function run(object $a, string $class): mixed\n    {\n        $s = new static();\n        $p = self::class;\n        $d = new $class();\n        $v = new \\Vendor\\Thing();\n        $n = 'App\\\\B\\\\Other';\n        return $a instanceof $class;\n    }\n}\n";
    let (nodes, edges) = produce_with(source);
    assert_eq!(references(&edges), vec![]);
    assert!(
        !nodes
            .iter()
            .any(|n| n.label.as_str() == "Item" && n.id.contains("Vendor\\Thing")),
        "no item is minted for a vendor class"
    );
}

#[test]
fn a_source_naming_its_own_class_yields_no_edge() {
    let (_, edges) = produce_with(&in_method("        return \\App\\A\\Source::class;"));
    assert_eq!(references(&edges), vec![]);
}

#[test]
fn every_refers_to_edge_carries_the_tree_sitter_php_resolver() {
    let (_, edges) = produce_with(&in_method("        return new \\App\\B\\Other();"));
    let resolvers: Vec<Option<&str>> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::REFERS_TO)
        .map(|e| prop(e, "resolver").and_then(PropValue::as_str))
        .collect();
    assert_eq!(resolvers, vec![Some("tree-sitter-php")]);
}
