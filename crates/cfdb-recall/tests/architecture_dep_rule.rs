//! CLEAN-3 architecture test for cfdb-recall (#21).
//!
//! cfdb-recall is the recall gate comparing cfdb-extractor's emitted facts
//! to `rustdoc --output-format=json` ground truth. RFC-029 §13 places it
//! one step above the extractor layer — it legitimately depends on both
//! cfdb-core (for shared types) and cfdb-extractor (the subject under
//! test). It must NOT depend on a store backend or on a parser layer;
//! recall measurement is orthogonal to query execution.

use std::collections::BTreeSet;

const CARGO_TOML: &str = include_str!("../Cargo.toml");

/// The complete allowed dependency set for cfdb-recall.
const ALLOWED_DEPS: &[&str] = &[
    // The hub — shared Node/Edge/Keyspace types.
    "cfdb-core",
    // The subject under test — the recall gate measures extractor output
    // against rustdoc ground truth.
    "cfdb-extractor",
    // Serialization for report emission and audit-list parsing.
    "serde",
    "serde_json",
    "thiserror",
    // Parsed ground-truth data model — kept ungated because
    // `project_rustdoc_paths` is a pure function in the public library
    // API and consumers need `Crate`/`ItemKind` in their signatures.
    "rustdoc-types",
    // Optional deps — gated behind the `runner` feature so slim library
    // consumers do not pay their compile cost. The Cargo.toml parser in
    // this test captures names from `[dependencies]` whether or not the
    // entry carries `optional = true`, so these still belong in the
    // allowlist.
    "clap",
    "rustdoc-json",
];

/// Crates that MUST NEVER appear in cfdb-recall's `[dependencies]` section.
const FORBIDDEN_DEPS: &[&str] = &[
    // Parser layer — recall measurement does not execute queries.
    "cfdb-query",
    // Store backends — recall is a pure data comparison; no graph store
    // is involved.
    "cfdb-petgraph",
    "cfdb-store-petgraph",
    "cfdb-store-lbug",
    // Sibling extractor variants — cfdb-recall operates on the canonical
    // extractor only.
    "cfdb-hir-extractor",
    // Entry-point CLI — cfdb-cli depends on cfdb-recall (or will), not
    // the reverse.
    "cfdb-cli",
    "cfdb-http",
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
fn cfdb_recall_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let forbidden: Vec<&str> = FORBIDDEN_DEPS
        .iter()
        .copied()
        .filter(|name| deps.contains(*name))
        .collect();

    assert!(
        forbidden.is_empty(),
        "cfdb-recall/Cargo.toml [dependencies] contains forbidden crates: {forbidden:?}\n\
         Recall gate may depend only on cfdb-core, cfdb-extractor, and rustdoc tooling \
         (RFC-029 §13 / CLEAN-3).\n\
         Found dependency set: {deps:?}"
    );
}

#[test]
fn cfdb_recall_dependencies_are_all_whitelisted() {
    let deps = parse_dependency_names();
    let allowed: BTreeSet<&str> = ALLOWED_DEPS.iter().copied().collect();
    let unknown: Vec<&String> = deps
        .iter()
        .filter(|d| !allowed.contains(d.as_str()))
        .collect();

    assert!(
        unknown.is_empty(),
        "cfdb-recall/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {ALLOWED_DEPS:?}\n\
         Update ALLOWED_DEPS in this test AND justify why the crate is recall-layer in a comment."
    );
}

#[test]
fn cfdb_recall_depends_on_cfdb_core_and_cfdb_extractor() {
    let deps = parse_dependency_names();
    assert!(
        deps.contains("cfdb-core"),
        "cfdb-recall must depend on cfdb-core (shared types)"
    );
    assert!(
        deps.contains("cfdb-extractor"),
        "cfdb-recall must depend on cfdb-extractor (subject under test)"
    );
}
