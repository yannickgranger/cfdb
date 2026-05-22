//! CLEAN-3 architecture test for cfdb-core (#3628).
//!
//! cfdb-core is the hub of the cfdb workspace. RFC-029 §8 mandates that the
//! dependency arrow points inward: cfdb-core MUST NOT depend on any concrete
//! store, parser, extractor, or wire-form crate. The forbidden crates are
//! reversed dependencies that would create a dependency cycle and let
//! infrastructure types leak into the foundation layer.
//!
//! This test parses cfdb-core's own `Cargo.toml` at compile time and asserts
//! that no forbidden crate appears in `[dependencies]`. The allow/forbid lists
//! are NOT inlined here — they live in the inert workspace-level declaration
//! `.cfdb/workspace-dep-rules.toml` (RFC-044 §3.3 / #422), the single source
//! of truth shared by all five per-crate dep-rule tests.

use std::collections::BTreeSet;

const CARGO_TOML: &str = include_str!("../Cargo.toml");

/// Inert workspace dep-direction declaration — single source of truth for the
/// allow/forbid graph (RFC-044 §3.3 / #422). Consumed via `include_str!`; the
/// per-crate reader below is intentionally self-contained (no shared Rust
/// crate, no new dev-dep — clean-arch R1 / no-monolith), the same accepted
/// duplication as `parse_dependency_names()`.
const DEP_RULES: &str = include_str!("../../../.cfdb/workspace-dep-rules.toml");

/// This crate's section name in the inert rules file.
const CRATE_SECTION: &str = "cfdb-core";

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
fn workspace_dep_rules_section_loaded() {
    // Non-vacuity guard: if the `include_str!` path breaks or the section name
    // drifts, both lists come back empty and every other assertion below would
    // pass vacuously. This test fails loudly instead.
    let (allowed, forbidden) = dep_rules_for(CRATE_SECTION);
    assert!(
        !allowed.is_empty() && !forbidden.is_empty(),
        "no `[{CRATE_SECTION}]` allow/forbid rows parsed from \
         .cfdb/workspace-dep-rules.toml — check the include_str! path and section name"
    );
    assert!(
        allowed.contains("serde") && forbidden.contains("cfdb-cli"),
        "expected sentinel rows missing — rules file shape changed unexpectedly"
    );
}

#[test]
fn cfdb_core_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let (_, forbidden_rules) = dep_rules_for(CRATE_SECTION);
    let forbidden: Vec<&String> = forbidden_rules
        .iter()
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
    let (allowed, _) = dep_rules_for(CRATE_SECTION);
    let unknown: Vec<&String> = deps.iter().filter(|d| !allowed.contains(*d)).collect();

    assert!(
        unknown.is_empty(),
        "cfdb-core/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {allowed:?}\n\
         Adding a new dependency to cfdb-core is a deliberate architectural choice. \
         Update the [cfdb-core] section of .cfdb/workspace-dep-rules.toml AND document \
         why the crate is hub-foundational in a comment there."
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
        deps.len() >= 3,
        "expected ≥3 dependencies in cfdb-core/Cargo.toml, parsed: {deps:?}"
    );
}
