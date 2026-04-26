//! CLEAN-3 architecture test for cfdb-extractor (#21).
//!
//! cfdb-extractor walks a Rust workspace via `syn` + `cargo_metadata` and
//! emits `Node`/`Edge` facts. RFC-029 §8 mandates it depends ONLY on
//! cfdb-core plus source-analysis tooling. Coupling to the store backend
//! (cfdb-petgraph), the parser (cfdb-query), or the entry points
//! (cfdb-cli, cfdb-recall) is forbidden — the extractor produces facts;
//! how they are stored or queried is a downstream concern.

use std::collections::BTreeSet;

const CARGO_TOML: &str = include_str!("../Cargo.toml");

/// The complete allowed dependency set for cfdb-extractor.
const ALLOWED_DEPS: &[&str] = &[
    // The hub — cfdb-extractor produces `Node`/`Edge` types defined in cfdb-core.
    "cfdb-core",
    // Shared `.cfdb/concepts/*.toml` loader + crate→bounded-context resolver
    // (Issue #3 extraction). Pure-library crate, zero heavy deps —
    // cfdb-query will also depend on it (Conformist pattern).
    "cfdb-concepts",
    // Rust source AST visitor.
    "syn",
    // Source-line spans — `span-locations` feature on `proc-macro2`
    // makes `Span::start().line` available so the extractor reports
    // real `:Item.line` / `:CallSite.line` instead of 0 (#273 / F-005).
    // proc-macro2 is already a transitive dep of `syn`; we name it
    // directly to opt into the feature flag, which is source-analysis
    // tooling — same layer as `syn`.
    "proc-macro2",
    // Workspace/crate metadata resolution.
    "cargo_metadata",
    // Concept override config (`.cfdb/concepts/*.toml`).
    "toml",
    // Serialization for emit surface.
    "serde",
    // RFC-040 §3.1 / slice 3 (#325) — `:ConstTable.entries_normalized` and
    // `entries_sample` are wire-stable JSON arrays of either strings (with
    // full JSON escaping) or integers up to `u64::MAX`. Hand-rolling the
    // escape rules duplicates serde_json's tested implementation; the dep
    // is workspace-known. Source-analysis-adjacent: the extractor is the
    // single producer of these wire props, no other layer is involved.
    "serde_json",
    // RFC-040 §3.1 / slice 3 (#325) — `:ConstTable.entries_hash` is sha256
    // hex over the canonical-sorted entry sequence. Pure cryptographic
    // primitive, no transitive runtime cost; the workspace dep table
    // already carries it for `cfdb-cli`'s persistence layer (R2 absorbed
    // rust-systems B2 — sha2 was incorrectly assumed transitive through
    // `cfdb-cli` in the original draft, but cfdb-cli depends on
    // cfdb-extractor and not vice-versa).
    "sha2",
    "thiserror",
];

/// Crates that MUST NEVER appear in cfdb-extractor's `[dependencies]` section.
const FORBIDDEN_DEPS: &[&str] = &[
    // Parser layer — query text parsing is not the extractor's concern.
    "cfdb-query",
    // Store backends — cfdb-extractor emits facts; it does not store them.
    "cfdb-petgraph",
    "cfdb-store-petgraph",
    "cfdb-store-lbug",
    // Sibling extractor — the HIR extractor variant is a parallel adapter,
    // not a base.
    "cfdb-hir-extractor",
    // Entry points depend on extractors, never the reverse.
    "cfdb-recall",
    "cfdb-cli",
    "cfdb-http",
    // Parser combinator — the extractor doesn't parse Cypher.
    "chumsky",
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
fn cfdb_extractor_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let forbidden: Vec<&str> = FORBIDDEN_DEPS
        .iter()
        .copied()
        .filter(|name| deps.contains(*name))
        .collect();

    assert!(
        forbidden.is_empty(),
        "cfdb-extractor/Cargo.toml [dependencies] contains forbidden crates: {forbidden:?}\n\
         Extractors must depend only on cfdb-core and source-analysis tooling \
         (RFC-029 §8 / CLEAN-3).\n\
         Found dependency set: {deps:?}"
    );
}

#[test]
fn cfdb_extractor_dependencies_are_all_whitelisted() {
    let deps = parse_dependency_names();
    let allowed: BTreeSet<&str> = ALLOWED_DEPS.iter().copied().collect();
    let unknown: Vec<&String> = deps
        .iter()
        .filter(|d| !allowed.contains(d.as_str()))
        .collect();

    assert!(
        unknown.is_empty(),
        "cfdb-extractor/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {ALLOWED_DEPS:?}\n\
         Update ALLOWED_DEPS in this test AND justify why the crate is extractor-layer in a comment."
    );
}

#[test]
fn cfdb_extractor_depends_on_cfdb_core_and_syn() {
    let deps = parse_dependency_names();
    assert!(
        deps.contains("cfdb-core"),
        "cfdb-extractor must depend on cfdb-core (Node/Edge types live there)"
    );
    assert!(
        deps.contains("syn"),
        "cfdb-extractor must depend on syn (the AST visitor)"
    );
}
