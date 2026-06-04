//! `PhpProducer` `IMPLEMENTS` edges (RFC-045 slice 45-A / issue #462).
//!
//! Exercises the two-pass interface-implementation resolution described in
//! RFC-045 §3.2: a PHP `class C implements I1, I2` emits one `IMPLEMENTS`
//! edge per interface, **but only when the target interface resolves to an
//! in-workspace `:Item`** (closed-world — external interfaces produce no
//! edge and no synthetic node). `extends` (`base_clause`) is intentionally
//! NOT an `IMPLEMENTS` edge (inheritance is deferred — §3.3 D3-a).
//!
//! Every PHP `IMPLEMENTS` edge carries `resolver = "tree-sitter-php"`.
//! The implementing class is `:Item{php_construct:"class_declaration"}`,
//! the target interface `:Item{php_construct:"interface_declaration"}`.

use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::EdgeLabel;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an ephemeral Composer project from `(relative_path, php_source)`
/// pairs, run `PhpProducer::produce`, and return the `(nodes, edges)`. A
/// minimal `composer.json` is written at the root so `detect()` would pass
/// and the walker treats the dir as a PHP workspace.
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

/// All `IMPLEMENTS` edges in declaration-independent (sorted) order.
fn implements_edges(edges: &[Edge]) -> Vec<&Edge> {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IMPLEMENTS)
        .collect()
}

/// `(src, dst)` id pairs of every `IMPLEMENTS` edge, sorted for stable
/// comparison.
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

/// Look up a node by id.
fn node<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| n.id == id)
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

// ---------------------------------------------------------------------------
// Unit cases (issue #462 Tests block)
// ---------------------------------------------------------------------------

/// `class C implements I1, I2, I3` (all in-workspace) → exactly 3
/// `IMPLEMENTS` edges, one per interface, all sourced at `C`.
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

    // resolver discriminator on every edge.
    for e in implements_edges(&edges) {
        assert_eq!(
            e.props.get("resolver").and_then(PropValue::as_str),
            Some("tree-sitter-php"),
            "every PHP IMPLEMENTS edge must carry resolver=tree-sitter-php; edge={e:?}",
        );
    }

    // Source/target php_construct disambiguation (RFC-045 §3.2 — both are
    // kind:"trait", php_construct distinguishes class source from iface target).
    let c = node(&nodes, &item_id(r"App\C")).expect("App\\C :Item present");
    assert_eq!(prop(c, "kind"), Some("trait"));
    assert_eq!(prop(c, "php_construct"), Some("class_declaration"));
    let i1 = node(&nodes, &item_id(r"App\I1")).expect("App\\I1 :Item present");
    assert_eq!(prop(i1, "php_construct"), Some("interface_declaration"));
}

/// `class C extends B implements I` → exactly 1 `IMPLEMENTS` (to `I`); the
/// `extends B` (`base_clause`) produces NO edge (inheritance deferred).
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

/// Fully-qualified `implements \Ns\I` where `Ns\I` is in-workspace →
/// target qname is the qualified `Ns\I` (leading `\` stripped).
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

/// External interface (not in-workspace) → ZERO `IMPLEMENTS` edges AND zero
/// synthetic `:Item` for the absent target (closed-world + stubs-not-arrows).
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
    // No placeholder node was invented for the unresolved target.
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

/// D3-a negative assertion: `interface A extends B {} class C implements A {}`
/// → "implementors of B" is empty (transitive implements is NOT recorded).
/// `interface A extends B` is a `base_clause`, so it emits no edge; only the
/// syntactically-declared `C implements A` is recorded.
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

    // Exactly the syntactic edge C → A; nothing to B.
    assert_eq!(
        implements_pairs(&edges),
        vec![(item_id(r"App\C"), item_id(r"App\A"))],
        "only the syntactic `C implements A` edge is recorded",
    );
    // "implementors of B" — no IMPLEMENTS edge targets B.
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

/// Unqualified `implements I` where `I` is NOT declared in the current
/// namespace → unresolved (no edge). The MVP does not track `use` imports,
/// so an interface declared in a different namespace and pulled in via
/// `use` does not resolve (documented closed-world limitation).
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

/// Re-extracting the same tree is byte-stable (determinism invariant —
/// RFC-045 §4 I3). The full sorted `(Vec<Node>, Vec<Edge>)` must be
/// identical across two runs.
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

/// Adding one more in-workspace class implementing an in-workspace interface
/// adds exactly one `IMPLEMENTS` edge (RFC-045 §4 I3 monotonic increment).
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

// ---------------------------------------------------------------------------
// Self-dogfood: the on-disk richer multi-interface fixture
// ---------------------------------------------------------------------------

fn richer_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/php-richer")
}

/// Extract the in-tree `php-richer` fixture and assert the expected
/// `IMPLEMENTS` topology: Product→{Identifiable,Serializable}, Order→Timestamped
/// (Order extends Product is a base_clause → no edge).
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

/// The richer fixture re-extracts byte-stably.
#[test]
fn richer_fixture_is_deterministic() {
    let run1 = PhpProducer.produce(&richer_fixture_root()).expect("run1");
    let run2 = PhpProducer.produce(&richer_fixture_root()).expect("run2");
    assert_eq!(format!("{run1:?}"), format!("{run2:?}"));
}
