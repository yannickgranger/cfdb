//! CLEAN-3 architecture test for cfdb-classify.
//!
//! cfdb-classify is the judgment layer: it depends on cfdb-core (schema
//! vocabulary), cfdb-query (parser, diff envelope) and cfdb-eval (the
//! evaluator engine) — never on cfdb-petgraph or any other concrete storage
//! engine, and never on an entry point. The engine reaches a keyspace only
//! through the `GraphBackend` port.

use std::collections::BTreeSet;

const CARGO_TOML: &str = include_str!("../Cargo.toml");

/// Inert workspace dep-direction declaration — single source of truth for the
/// allow/forbid graph (RFC-044 §3.3 / #422). Consumed via `include_str!`; the
/// per-crate reader below is intentionally self-contained (no shared Rust
/// crate, no new dev-dep — clean-arch R1 / no-monolith), the same accepted
/// duplication as `parse_dependency_names()`.
const DEP_RULES: &str = include_str!("../../../.cfdb/workspace-dep-rules.toml");

/// This crate's section name in the inert rules file.
const CRATE_SECTION: &str = "cfdb-classify";

/// Read the `allowed` and `forbidden` arrays for `crate_name` from `DEP_RULES`.
/// Line-oriented reader over hand-authored TOML (one quoted entry per array
/// line); returns `(allowed, forbidden)`.
fn dep_rules_for(crate_name: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let header = format!("[{crate_name}]");
    let mut allowed = BTreeSet::new();
    let mut forbidden = BTreeSet::new();
    let mut in_section = false;
    // 0 = neither array, 1 = inside `allowed`, 2 = inside `forbidden`.
    let mut bucket = 0u8;

    for raw in DEP_RULES.lines() {
        let line = raw.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_section = line == header;
            bucket = 0;
            continue;
        }
        if !in_section {
            continue;
        }
        if line.starts_with("allowed") {
            bucket = 1;
            continue;
        }
        if line.starts_with("forbidden") {
            bucket = 2;
            continue;
        }
        if line.starts_with(']') {
            bucket = 0;
            continue;
        }
        if let (Some(start), Some(end)) = (line.find('"'), line.rfind('"')) {
            if end > start {
                let name = line[start + 1..end].to_string();
                match bucket {
                    1 => {
                        allowed.insert(name);
                    }
                    2 => {
                        forbidden.insert(name);
                    }
                    _ => {}
                }
            }
        }
    }

    (allowed, forbidden)
}

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
fn workspace_dep_rules_section_loaded() {
    // Non-vacuity guard: a broken include_str! path or drifted section name
    // would yield empty lists and make every assertion below pass vacuously.
    let (allowed, forbidden) = dep_rules_for(CRATE_SECTION);
    assert!(
        !allowed.is_empty() && !forbidden.is_empty(),
        "no `[{CRATE_SECTION}]` allow/forbid rows parsed from \
         .cfdb/workspace-dep-rules.toml — check the include_str! path and section name"
    );
    assert!(
        allowed.contains("cfdb-core") && forbidden.contains("cfdb-petgraph"),
        "expected sentinel rows missing — rules file shape changed unexpectedly"
    );
}

#[test]
fn cfdb_classify_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let (_, forbidden_rules) = dep_rules_for(CRATE_SECTION);
    let forbidden: Vec<&String> = forbidden_rules
        .iter()
        .filter(|name| deps.contains(*name))
        .collect();

    assert!(
        forbidden.is_empty(),
        "cfdb-classify/Cargo.toml [dependencies] contains forbidden crates: {forbidden:?}\n\
         cfdb-classify reaches storage only through cfdb-core's GraphBackend port; \
         it never names a concrete storage engine or an entry point.\n\
         Found dependency set: {deps:?}"
    );
}

#[test]
fn cfdb_classify_dependencies_are_all_whitelisted() {
    let deps = parse_dependency_names();
    let (allowed, _) = dep_rules_for(CRATE_SECTION);
    let unknown: Vec<&String> = deps.iter().filter(|d| !allowed.contains(*d)).collect();

    assert!(
        unknown.is_empty(),
        "cfdb-classify/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {allowed:?}\n\
         Update the [cfdb-classify] section of .cfdb/workspace-dep-rules.toml AND \
         justify why the crate is evaluator-layer in a comment there."
    );
}

#[test]
fn cfdb_classify_depends_on_cfdb_core() {
    let deps = parse_dependency_names();
    assert!(
        deps.contains("cfdb-core"),
        "cfdb-classify must depend on cfdb-core (schema vocabulary and the GraphBackend port live there)"
    );
}
