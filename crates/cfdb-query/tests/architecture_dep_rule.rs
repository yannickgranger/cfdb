//! CLEAN-3 architecture test for cfdb-query (#21).
//!
//! cfdb-query is the parser + builder layer. RFC-029 §8 mandates that the
//! dependency arrow points inward: cfdb-query depends on cfdb-core to
//! produce a `Query` AST and MUST NOT depend on any sibling adapter or
//! entry-point crate. Listing a sibling here would either create a cycle
//! or couple unrelated adapters together.
//!
//! This test parses cfdb-query's own `Cargo.toml` at compile time and
//! asserts that no forbidden crate appears in `[dependencies]`. Adding to
//! the allowlist is a deliberate architectural choice.

use std::collections::BTreeSet;

const CARGO_TOML: &str = include_str!("../Cargo.toml");

/// The complete allowed dependency set for cfdb-query.
const ALLOWED_DEPS: &[&str] = &[
    // The hub — cfdb-query produces `Query` AST nodes defined in cfdb-core.
    "cfdb-core",
    // Serde derives on parser error types + builder AST round-trips.
    "serde",
    "serde_json",
    "thiserror",
    // Parser combinator library (RFC-029 §10.2).
    "chumsky",
    // Pre-pass comment/keyword scanner state machine.
    "regex",
    // `SkillRoutingTable` parses the external `.cfdb/skill-routing.toml`
    // policy file (issue #48 / RFC-cfdb-v0.2-addendum §A2.3). Parser-layer
    // by construction — text in → typed policy AST out — the same shape as
    // `chumsky`-based Cypher parsing. Load-bearing for the classifier's DIP
    // invariant (council BLOCK-1, solid-architect): class → skill routing
    // MUST live outside the graph schema, so the decoder lives with the
    // consumer surface (alongside `DebtClass` / `Finding`) rather than in
    // `cfdb-cli`, keeping `/operate-module` and `/boy-scout
    // --from-inventory` independent of any particular CLI.
    "toml",
];

/// Crates that MUST NEVER appear in cfdb-query's `[dependencies]` section.
/// Adapter siblings depend on `cfdb-core`, not on each other; adding any of
/// these creates lateral coupling across the adapter fringe.
const FORBIDDEN_DEPS: &[&str] = &[
    // Store adapter — different layer, must not bind query parsing to a
    // specific backend.
    "cfdb-petgraph",
    "cfdb-store-petgraph",
    "cfdb-store-lbug",
    // Source extractor — belongs on a different axis (code → facts).
    "cfdb-extractor",
    "cfdb-hir-extractor",
    // Recall gate and CLI — these depend on cfdb-query, never the reverse.
    "cfdb-recall",
    "cfdb-cli",
    "cfdb-http",
    // Heavy extractor crates have no business in the parser layer.
    "syn",
    "proc-macro2",
    "quote",
    "ra-ap-hir",
    "ra-ap-syntax",
    // Async runtimes and HTTP layers belong in entry-point crates only.
    "tokio",
    "axum",
    "hyper",
    "reqwest",
];

fn parse_dependency_names() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_deps_section = false;

    for raw_line in CARGO_TOML.lines() {
        let line = raw_line.trim();

        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            in_deps_section = line == "[dependencies]";
            continue;
        }

        if !in_deps_section {
            continue;
        }

        if let Some(eq_idx) = line.find('=') {
            let key = line[..eq_idx].trim();
            let crate_name = key.split('.').next().unwrap_or(key).trim();
            if !crate_name.is_empty() {
                names.insert(crate_name.to_string());
            }
        }
    }

    names
}

#[test]
fn cfdb_query_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let forbidden: Vec<&str> = FORBIDDEN_DEPS
        .iter()
        .copied()
        .filter(|name| deps.contains(*name))
        .collect();

    assert!(
        forbidden.is_empty(),
        "cfdb-query/Cargo.toml [dependencies] contains forbidden crates: {forbidden:?}\n\
         Parser layer must depend only on cfdb-core plus parser utilities \
         (RFC-029 §8 / CLEAN-3).\n\
         Found dependency set: {deps:?}"
    );
}

#[test]
fn cfdb_query_dependencies_are_all_whitelisted() {
    let deps = parse_dependency_names();
    let allowed: BTreeSet<&str> = ALLOWED_DEPS.iter().copied().collect();
    let unknown: Vec<&String> = deps
        .iter()
        .filter(|d| !allowed.contains(d.as_str()))
        .collect();

    assert!(
        unknown.is_empty(),
        "cfdb-query/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {ALLOWED_DEPS:?}\n\
         Update ALLOWED_DEPS in this test AND justify why the crate is parser-layer in a comment."
    );
}

#[test]
fn cfdb_query_depends_on_cfdb_core() {
    let deps = parse_dependency_names();
    assert!(
        deps.contains("cfdb-core"),
        "cfdb-query must depend on cfdb-core (Query AST types live there)"
    );
}
