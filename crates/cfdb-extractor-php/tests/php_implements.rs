use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::EdgeLabel;
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
            fs::create_dir_all(parent).expect("mkdir -p fixture subdir");
        }
        fs::write(&path, src).expect("write php source");
    }
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
}

fn implements_edges(edges: &[Edge]) -> Vec<&Edge> {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IMPLEMENTS)
        .collect()
}

fn implements_pairs(edges: &[Edge]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = implements_edges(edges)
        .iter()
        .map(|e| (e.src.clone(), e.dst.clone()))
        .collect();
    pairs.sort();
    pairs
}

fn item_id(qname: &str) -> String {
    format!("item:{qname}")
}

fn node<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| n.id == id)
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

#[test]
fn class_implementing_three_in_workspace_interfaces_emits_three_edges() {
    let (nodes, edges) = produce_php(&[(
        "src/all.php",
        r#"<?php
namespace App;
interface I1 {}
interface I2 {}
interface I3 {}
class C implements I1, I2, I3 {}
"#,
    )]);

    let pairs = implements_pairs(&edges);
    assert_eq!(
        pairs,
        vec![
            (item_id(r"App\C"), item_id(r"App\I1")),
            (item_id(r"App\C"), item_id(r"App\I2")),
            (item_id(r"App\C"), item_id(r"App\I3")),
        ],
        "expected exactly 3 IMPLEMENTS edges App\\C → {{I1,I2,I3}}; got {pairs:?}",
    );

    for e in implements_edges(&edges) {
        assert_eq!(
            e.props.get("resolver").and_then(PropValue::as_str),
            Some("tree-sitter-php"),
            "every PHP IMPLEMENTS edge must carry resolver=tree-sitter-php; edge={e:?}",
        );
    }

    let c = node(&nodes, &item_id(r"App\C")).expect("App\\C :Item present");
    assert_eq!(prop(c, "kind"), Some("trait"));
    assert_eq!(prop(c, "php_construct"), Some("class_declaration"));
    let i1 = node(&nodes, &item_id(r"App\I1")).expect("App\\I1 :Item present");
    assert_eq!(prop(i1, "php_construct"), Some("interface_declaration"));
}

#[test]
fn extends_plus_implements_emits_only_the_implements_edge() {
    let (_nodes, edges) = produce_php(&[(
        "src/c.php",
        r#"<?php
namespace App;
class B {}
interface I {}
class C extends B implements I {}
"#,
    )]);

    assert_eq!(
        implements_pairs(&edges),
        vec![(item_id(r"App\C"), item_id(r"App\I"))],
        "extends B must NOT emit an IMPLEMENTS edge (base_clause excluded); \
         only `implements I` should",
    );
}

#[test]
fn qualified_implements_resolves_to_qualified_target() {
    let (_nodes, edges) = produce_php(&[
        (
            "src/iface.php",
            r#"<?php
namespace Ns;
interface I {}
"#,
        ),
        (
            "src/impl.php",
            r#"<?php
namespace App;
class C implements \Ns\I {}
"#,
        ),
    ]);

    assert_eq!(
        implements_pairs(&edges),
        vec![(item_id(r"App\C"), item_id(r"Ns\I"))],
        "fully-qualified `\\Ns\\I` must resolve to the qualified target qname Ns\\I",
    );
}

#[test]
fn external_interface_emits_no_edge_and_no_synthetic_item() {
    let (nodes, edges) = produce_php(&[(
        "src/c.php",
        r#"<?php
namespace App;
class C implements \Vendor\Serializable {}
"#,
    )]);

    assert!(
        implements_edges(&edges).is_empty(),
        "an external interface target must produce no IMPLEMENTS edge; got {:?}",
        implements_pairs(&edges),
    );
    assert!(
        node(&nodes, &item_id(r"Vendor\Serializable")).is_none(),
        "no synthetic :Item may be created for an external interface target",
    );
    assert!(
        nodes
            .iter()
            .all(|n| prop(n, "name") != Some("Serializable")),
        "no node carrying the external interface name may be synthesized",
    );
}

#[test]
fn transitive_interface_extends_is_not_bridged() {
    let (_nodes, edges) = produce_php(&[(
        "src/h.php",
        r#"<?php
namespace App;
interface B {}
interface A extends B {}
class C implements A {}
"#,
    )]);

    assert_eq!(
        implements_pairs(&edges),
        vec![(item_id(r"App\C"), item_id(r"App\A"))],
        "only the syntactic `C implements A` edge is recorded",
    );
    let implementors_of_b: Vec<_> = implements_edges(&edges)
        .into_iter()
        .filter(|e| e.dst == item_id(r"App\B"))
        .collect();
    assert!(
        implementors_of_b.is_empty(),
        "D3-a gap must be stable: `implementors of B` is empty until EXTENDS lands; \
         got {implementors_of_b:?}",
    );
}

#[test]
fn unqualified_interface_in_other_namespace_does_not_resolve() {
    let (_nodes, edges) = produce_php(&[
        (
            "src/iface.php",
            r#"<?php
namespace Other;
interface I {}
"#,
        ),
        (
            "src/impl.php",
            r#"<?php
namespace App;
class C implements I {}
"#,
        ),
    ]);

    assert!(
        implements_edges(&edges).is_empty(),
        "unqualified `implements I` qualifies to App\\I (current namespace), which \
         is absent — Other\\I is not pulled in (no `use`-import resolution in MVP); \
         got {:?}",
        implements_pairs(&edges),
    );
}

#[test]
fn re_extract_is_deterministic() {
    let files = &[(
        "src/m.php",
        r#"<?php
namespace App;
interface I1 {}
interface I2 {}
class A implements I1 {}
class B implements I1, I2 {}
"#,
    )];
    let run1 = produce_php(files);
    let run2 = produce_php(files);
    assert_eq!(
        format!("{run1:?}"),
        format!("{run2:?}"),
        "PhpProducer.produce must be byte-stable across re-extracts",
    );
}

#[test]
fn adding_one_implementing_class_adds_one_edge() {
    let before = produce_php(&[(
        "src/m.php",
        r#"<?php
namespace App;
interface I {}
class A implements I {}
"#,
    )]);
    let after = produce_php(&[(
        "src/m.php",
        r#"<?php
namespace App;
interface I {}
class A implements I {}
class Z implements I {}
"#,
    )]);
    assert_eq!(
        implements_edges(&before.1).len() + 1,
        implements_edges(&after.1).len(),
        "+1 in-workspace class implementing an in-workspace interface ⇒ +1 IMPLEMENTS edge",
    );
}

fn richer_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/php-richer")
}

#[test]
fn richer_fixture_implements_topology() {
    let (_nodes, edges) = PhpProducer
        .produce(&richer_fixture_root())
        .expect("produce php-richer fixture");

    assert_eq!(
        implements_pairs(&edges),
        vec![
            (item_id(r"Shop\Order"), item_id(r"Shop\Timestamped")),
            (item_id(r"Shop\Product"), item_id(r"Shop\Identifiable")),
            (item_id(r"Shop\Product"), item_id(r"Shop\Serializable")),
        ],
        "php-richer IMPLEMENTS topology (Order extends Product must not appear)",
    );
    for e in implements_edges(&edges) {
        assert_eq!(
            e.props.get("resolver").and_then(PropValue::as_str),
            Some("tree-sitter-php"),
        );
    }
}

#[test]
fn richer_fixture_is_deterministic() {
    let run1 = PhpProducer.produce(&richer_fixture_root()).expect("run1");
    let run2 = PhpProducer.produce(&richer_fixture_root()).expect("run2");
    assert_eq!(format!("{run1:?}"), format!("{run2:?}"));
}

const ERASABLE: &str =
    "<?php\nnamespace App\\Privacy;\ninterface Erasable {}\ninterface Wiper {}\n";

#[test]
fn a_use_imported_interface_yields_the_edge() {
    let (_nodes, edges) = produce_php(&[
        ("src/Privacy.php", ERASABLE),
        (
            "src/Enrolling.php",
            "<?php\nnamespace App\\Enrolment;\nuse App\\Privacy\\Erasable;\nclass Enrolling implements Erasable {}\n",
        ),
    ]);
    assert_eq!(
        implements_pairs(&edges),
        vec![(
            item_id("App\\Enrolment\\Enrolling"),
            item_id("App\\Privacy\\Erasable")
        )],
        "the idiomatic form — use import then bare name — resolves"
    );
}

#[test]
fn an_aliased_use_import_yields_the_edge_under_its_alias() {
    let (_nodes, edges) = produce_php(&[
        ("src/Privacy.php", ERASABLE),
        (
            "src/Enrolling.php",
            "<?php\nnamespace App\\Enrolment;\nuse App\\Privacy\\Wiper as Cleaner;\nclass Enrolling implements Cleaner {}\n",
        ),
    ]);
    assert_eq!(
        implements_pairs(&edges),
        vec![(
            item_id("App\\Enrolment\\Enrolling"),
            item_id("App\\Privacy\\Wiper")
        )]
    );
}

#[test]
fn a_grouped_use_import_yields_the_edge_for_each_member() {
    let (_nodes, edges) = produce_php(&[
        ("src/Privacy.php", ERASABLE),
        (
            "src/Enrolling.php",
            "<?php\nnamespace App\\Enrolment;\nuse App\\Privacy\\{Erasable, Wiper as Cleaner};\nclass Enrolling implements Erasable, Cleaner {}\n",
        ),
    ]);
    assert_eq!(
        implements_pairs(&edges),
        vec![
            (
                item_id("App\\Enrolment\\Enrolling"),
                item_id("App\\Privacy\\Erasable")
            ),
            (
                item_id("App\\Enrolment\\Enrolling"),
                item_id("App\\Privacy\\Wiper")
            ),
        ]
    );
}

#[test]
fn a_use_import_does_not_resolve_to_the_current_namespace() {
    let (nodes, edges) = produce_php(&[
        ("src/Privacy.php", ERASABLE),
        (
            "src/Enrolling.php",
            "<?php\nnamespace App\\Enrolment;\nuse App\\Privacy\\Erasable;\nclass Enrolling implements Erasable {}\n",
        ),
    ]);
    assert!(
        node(&nodes, &item_id("App\\Enrolment\\Erasable")).is_none(),
        "no placeholder is invented for the imported name in the importing namespace"
    );
    assert!(
        implements_edges(&edges)
            .iter()
            .all(|e| e.dst != item_id("App\\Enrolment\\Erasable")),
        "resolving a use-imported name to the current namespace is the defect this fixes"
    );
}

#[test]
fn an_out_of_workspace_interface_still_yields_no_edge_and_no_placeholder() {
    let (nodes, edges) = produce_php(&[(
        "src/Enrolling.php",
        "<?php\nnamespace App\\Enrolment;\nuse Symfony\\Component\\Serializer\\Serializable;\nclass Enrolling implements Serializable {}\n",
    )]);
    assert!(
        implements_edges(&edges).is_empty(),
        "closed-world: a vendor interface resolves to a qname no :Item carries"
    );
    assert!(
        node(
            &nodes,
            &item_id("Symfony\\Component\\Serializer\\Serializable")
        )
        .is_none(),
        "stubs are not arrows"
    );
}

#[test]
fn a_function_use_import_does_not_enter_the_class_table() {
    let (_nodes, edges) = produce_php(&[
        ("src/Privacy.php", ERASABLE),
        (
            "src/Enrolling.php",
            "<?php\nnamespace App\\Privacy;\nuse function App\\Helpers\\Erasable;\nclass Enrolling implements Erasable {}\n",
        ),
    ]);
    assert_eq!(
        implements_pairs(&edges),
        vec![(
            item_id("App\\Privacy\\Enrolling"),
            item_id("App\\Privacy\\Erasable")
        )],
        "a `use function` import is a different namespace and must not shadow the class name"
    );
}
