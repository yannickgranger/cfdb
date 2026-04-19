//! CLEAN-3 architecture test for cfdb-core (#3628).
//!
//! cfdb-core is the hub of the cfdb workspace. RFC-029 §8 mandates that the
//! dependency arrow points inward: cfdb-core MUST NOT depend on any concrete
//! store, parser, extractor, or wire-form crate. The forbidden crates are
//! reversed dependencies that would create a dependency cycle and let
//! infrastructure types leak into the foundation layer.
//!
//! This test parses cfdb-core's own `Cargo.toml` at compile time and asserts
//! that no forbidden crate appears in `[dependencies]`. The whitelist is the
//! complete allowed set — anything new must be added explicitly with a
//! comment justifying why it is hub-foundational.

use std::collections::BTreeSet;

const CARGO_TOML: &str = include_str!("../Cargo.toml");

/// The complete allowed dependency set for cfdb-core. Adding to this list is
/// a deliberate architectural choice and requires updating both the comment
/// here and RFC-029 §8 if the additional crate is foundational.
const ALLOWED_DEPS: &[&str] = &[
    // Serde derives — every domain type is serialisable so cfdb-cli, RPC
    // wrappers, and snapshot tooling can round-trip facts and reports.
    "serde",
    // Property-bag values (PropValue::Json) and arbitrary Cypher params need
    // a dynamic JSON value type. Pulling in serde_json here keeps it out of
    // every downstream crate's signatures.
    "serde_json",
    // Error type derives. Foundation crates always need an error story.
    "thiserror",
    // Stable-order hash maps for query parameter bindings — required for
    // determinism invariant G1 (byte-identical canonical dump across runs).
    "indexmap",
];

/// Crates that MUST NEVER appear in cfdb-core's `[dependencies]` section.
/// Each one represents a layer that depends on cfdb-core, so listing it here
/// would create a dependency cycle. The list is exhaustive for v0.1 and
/// should grow whenever a new sibling crate is added to the cfdb workspace.
const FORBIDDEN_DEPS: &[&str] = &[
    // Concrete store implementations — they impl `StoreBackend`, so cfdb-core
    // depending on them would invert Clean Architecture.
    "cfdb-petgraph",
    "cfdb-store-petgraph",
    "cfdb-store-lbug",
    // Parser layer — converts text into `Query` AST defined in cfdb-core.
    // The arrow points cfdb-query → cfdb-core, never the other way.
    "cfdb-query",
    // Extractor layer — produces `Node`/`Edge` facts defined in cfdb-core.
    "cfdb-extractor",
    "cfdb-hir-extractor",
    // Wire-form layer — clap / axum surface that consumes the trait.
    "cfdb-cli",
    "cfdb-http",
    // Heavy parser/IR crates that have NO business in a foundation crate.
    // syn pulls in proc-macro2 + quote + unicode-ident; ra-ap-* pulls in the
    // entire rust-analyzer HIR. Both belong in their respective extractor
    // crates only.
    "syn",
    "proc-macro2",
    "quote",
    "ra-ap-hir",
    "ra-ap-syntax",
    "ra-ap-ide",
    "ra-ap-ide-db",
    "ra-ap-base-db",
    // Async runtimes and HTTP layers belong in cfdb-cli / cfdb-http only.
    "tokio",
    "axum",
    "hyper",
    "reqwest",
];

/// Parse the `[dependencies]` section of cfdb-core's Cargo.toml and return
/// the set of dependency names. Inline-table form (`name = { ... }`) and
/// shorthand form (`name = "..."` / `name.workspace = true`) are both
/// supported — that is the only syntax used in this workspace.
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
            // Strip dotted-key suffixes like `serde.workspace`.
            let crate_name = key.split('.').next().unwrap_or(key).trim();
            if !crate_name.is_empty() {
                names.insert(crate_name.to_string());
            }
        }
    }

    names
}

#[test]
fn cfdb_core_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let forbidden: Vec<&str> = FORBIDDEN_DEPS
        .iter()
        .copied()
        .filter(|name| deps.contains(*name))
        .collect();

    assert!(
        forbidden.is_empty(),
        "cfdb-core/Cargo.toml [dependencies] contains forbidden crates: {forbidden:?}\n\
         These crates depend on cfdb-core and must not appear here (RFC-029 §8 / CLEAN-3).\n\
         Found dependency set: {deps:?}"
    );
}

#[test]
fn cfdb_core_dependencies_are_all_whitelisted() {
    let deps = parse_dependency_names();
    let allowed: BTreeSet<&str> = ALLOWED_DEPS.iter().copied().collect();
    let unknown: Vec<&String> = deps
        .iter()
        .filter(|d| !allowed.contains(d.as_str()))
        .collect();

    assert!(
        unknown.is_empty(),
        "cfdb-core/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {ALLOWED_DEPS:?}\n\
         Adding a new dependency to cfdb-core is a deliberate architectural choice. \
         Update ALLOWED_DEPS in this test AND document why the crate is hub-foundational \
         in the comment above the constant."
    );
}

#[test]
fn cfdb_core_keeps_serde_thiserror_minimum() {
    // Sanity: the foundation layer must always carry serde + thiserror.
    // If either disappears, the API contract breaks for every caller.
    let deps = parse_dependency_names();
    assert!(deps.contains("serde"), "cfdb-core must depend on serde");
    assert!(
        deps.contains("thiserror"),
        "cfdb-core must depend on thiserror"
    );
}

#[test]
fn parser_finds_all_current_dependencies() {
    // Self-test: if the Cargo.toml shrinks below this floor, the parser is
    // probably broken (returning an empty set) rather than the file being
    // empty. This guards against silent test passes.
    let deps = parse_dependency_names();
    assert!(
        deps.len() >= 4,
        "expected ≥4 dependencies in cfdb-core/Cargo.toml, parsed: {deps:?}"
    );
}
