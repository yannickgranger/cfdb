//! `TypeScriptProducer` (RFC-041 Phase 3 / issue #265) integration tests.
//!
//! Pin the TypeScript-side surface against the closed `:Item.kind` set
//! that `cfdb-core::schema::labels` declares. The MVP AC bar (issue
//! #265) requires at least one `interface` (→ `:Item.kind="trait"`),
//! one `type alias` (→ `:Item.kind="type"`), and one exported
//! `function` (→ `:Item.kind="fn"`) on the synthetic Next.js-shaped
//! fixture under `tests/fixtures/ts-minimal/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cfdb_core::fact::Node;
use cfdb_core::schema::Label;
use cfdb_extractor_ts::TypeScriptProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("ts-minimal")
}

fn item_kind(node: &Node) -> Option<&str> {
    if node.label.as_str() != Label::ITEM {
        return None;
    }
    node.props.get("kind").and_then(|v| v.as_str())
}

fn count_items_with_kind(nodes: &[Node], kind: &str) -> usize {
    nodes.iter().filter(|n| item_kind(n) == Some(kind)).count()
}

#[test]
fn name_returns_typescript_identifier() {
    assert_eq!(
        TypeScriptProducer.name(),
        "typescript",
        "TypeScriptProducer.name() must be the canonical CLI dispatch identifier (RFC-041 §3.4)"
    );
}

#[test]
fn detect_returns_true_when_tsconfig_and_package_json_present() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("tsconfig.json"), "{}").expect("write tsconfig.json");
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"x","version":"0.0.1"}"#,
    )
    .expect("write package.json");

    assert!(
        TypeScriptProducer.detect(dir.path()),
        "detect() must return true when both tsconfig.json AND package.json are present"
    );
}

#[test]
fn detect_returns_false_when_only_package_json_present() {
    let dir = TempDir::new().expect("tempdir");
    // package.json alone matches plain Node.js / JS — it's not a
    // TypeScript-specific signal. The producer must reject this so
    // the dispatcher does not invoke `produce()` on a non-TS tree.
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"x","version":"0.0.1"}"#,
    )
    .expect("write package.json");

    assert!(
        !TypeScriptProducer.detect(dir.path()),
        "detect() must return false on a package.json-only directory \
         (TypeScript requires the tsconfig.json marker)"
    );
}

#[test]
fn produce_emits_at_least_one_interface_one_type_alias_one_exported_function() {
    let (nodes, edges) = TypeScriptProducer
        .produce(&fixture_root())
        .expect("produce on ts-minimal fixture");

    // AC bar (issue #265): at least one of each of the three mapped
    // TypeScript construct kinds. Counts use `>=` so the assertion
    // survives when the fixture is later extended.
    let interfaces = count_items_with_kind(&nodes, "trait");
    let type_aliases = count_items_with_kind(&nodes, "type");
    let exported_fns = count_items_with_kind(&nodes, "fn");

    assert!(
        interfaces >= 1,
        "expected ≥ 1 :Item{{kind:\"trait\"}} (TS interface) — got {interfaces}; \
         emitted nodes: {:?}",
        nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
    );
    assert!(
        type_aliases >= 1,
        "expected ≥ 1 :Item{{kind:\"type\"}} (TS type alias) — got {type_aliases}"
    );
    assert!(
        exported_fns >= 1,
        "expected ≥ 1 :Item{{kind:\"fn\"}} (TS exported function) — got {exported_fns}"
    );

    // Structural cross-check: every emitted :Item must carry both an
    // IN_CRATE edge to the synthetic :Crate node AND an IN_MODULE edge
    // to the file's :Module node. A producer that emits an :Item but
    // forgets one of the structural edges silently breaks downstream
    // Cypher walks (#267 / RFC-041 §4 schema invariant).
    for item in nodes.iter().filter(|n| n.label.as_str() == Label::ITEM) {
        let in_crate_count = edges
            .iter()
            .filter(|e| e.src == item.id && e.label.as_str() == "IN_CRATE")
            .count();
        let in_module_count = edges
            .iter()
            .filter(|e| e.src == item.id && e.label.as_str() == "IN_MODULE")
            .count();
        assert_eq!(
            in_crate_count, 1,
            ":Item {} must have exactly one IN_CRATE edge",
            item.id
        );
        assert_eq!(
            in_module_count, 1,
            ":Item {} must have exactly one IN_MODULE edge",
            item.id
        );
    }
}

#[test]
fn produce_under_5s_on_fixture() {
    let start = Instant::now();
    let (nodes, edges) = TypeScriptProducer
        .produce(&fixture_root())
        .expect("produce on ts-minimal fixture");
    let elapsed = start.elapsed();

    // Wall-clock ceiling per issue #265 AC. The fixture is 3
    // declarations in one file — the real budget is milliseconds, but
    // the assertion uses 5s so a slow CI runner with a cold tree-sitter
    // load doesn't trip false positives.
    assert!(
        elapsed < Duration::from_secs(5),
        "produce() on ts-minimal fixture must complete in <5s — took {elapsed:?}"
    );
    assert!(
        !nodes.is_empty() && !edges.is_empty(),
        "fixture must yield non-empty nodes + edges"
    );
}
