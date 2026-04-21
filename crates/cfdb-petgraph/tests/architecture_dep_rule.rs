//! CLEAN-3 architecture test for cfdb-petgraph (#21).
//!
//! cfdb-petgraph is a concrete `StoreBackend` implementation. RFC-029 §8
//! mandates that backend adapters depend ONLY on cfdb-core (the trait +
//! data model) and their own infrastructure crate (petgraph). Coupling to
//! other adapters (parser, extractor) or entry points (CLI, recall) is
//! forbidden — it would turn the backend fringe into a bundle.

use std::collections::BTreeSet;

const CARGO_TOML: &str = include_str!("../Cargo.toml");

/// The complete allowed dependency set for cfdb-petgraph.
const ALLOWED_DEPS: &[&str] = &[
    // The hub — cfdb-petgraph implements `StoreBackend` defined in cfdb-core.
    "cfdb-core",
    // The backend itself.
    "petgraph",
    // Serialization for fact round-trip + property bags.
    "serde",
    "serde_json",
    "thiserror",
    // Predicate evaluator regex support (e.g. `regexp_extract`).
    "regex",
    // Stable-order binding tables needed for determinism invariant G1.
    "indexmap",
    // Backend-layer enrichment I/O: `enrich_git_history` walks HEAD via libgit2
    // to populate `:Item.git_*` attrs (#105 / slice 43-B). Optional; gated
    // behind the `git-enrich` feature so default builds stay libgit2-free.
    // Stays backend-side because the pass MUTATES KeyspaceState (the backend's
    // owned data), not the port trait surface.
    "git2",
    // Shared concept-override loader + heuristic (#108 / slice 43-E). The
    // backend's `enrich_bounded_context` pass calls `compute_bounded_context`
    // from this crate to patch `:Item.bounded_context` when
    // `.cfdb/concepts/*.toml` changed between extractions. Single resolution
    // point — the extract-time path in cfdb-extractor uses the same function.
    "cfdb-concepts",
];

/// Crates that MUST NEVER appear in cfdb-petgraph's `[dependencies]` section.
const FORBIDDEN_DEPS: &[&str] = &[
    // Parser layer — query text parsing is not the backend's concern.
    "cfdb-query",
    // Source extractor — different axis (code → facts, not query → result).
    "cfdb-extractor",
    "cfdb-hir-extractor",
    // Sibling backends — adapters don't know about each other.
    "cfdb-store-petgraph",
    "cfdb-store-lbug",
    // Entry points.
    "cfdb-recall",
    "cfdb-cli",
    "cfdb-http",
    // Heavy crates that belong in their respective adapter crates.
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
fn cfdb_petgraph_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let forbidden: Vec<&str> = FORBIDDEN_DEPS
        .iter()
        .copied()
        .filter(|name| deps.contains(*name))
        .collect();

    assert!(
        forbidden.is_empty(),
        "cfdb-petgraph/Cargo.toml [dependencies] contains forbidden crates: {forbidden:?}\n\
         Backend adapters must depend only on cfdb-core and their own \
         infrastructure crate (RFC-029 §8 / CLEAN-3).\n\
         Found dependency set: {deps:?}"
    );
}

#[test]
fn cfdb_petgraph_dependencies_are_all_whitelisted() {
    let deps = parse_dependency_names();
    let allowed: BTreeSet<&str> = ALLOWED_DEPS.iter().copied().collect();
    let unknown: Vec<&String> = deps
        .iter()
        .filter(|d| !allowed.contains(d.as_str()))
        .collect();

    assert!(
        unknown.is_empty(),
        "cfdb-petgraph/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {ALLOWED_DEPS:?}\n\
         Update ALLOWED_DEPS in this test AND justify why the crate is backend-layer in a comment."
    );
}

#[test]
fn cfdb_petgraph_depends_on_cfdb_core_and_petgraph() {
    let deps = parse_dependency_names();
    assert!(
        deps.contains("cfdb-core"),
        "cfdb-petgraph must depend on cfdb-core (StoreBackend trait lives there)"
    );
    assert!(
        deps.contains("petgraph"),
        "cfdb-petgraph must depend on petgraph (the backend)"
    );
}
