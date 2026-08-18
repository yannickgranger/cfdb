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
/// Inert workspace dep-direction declaration — single source of truth for the
/// allow/forbid graph (RFC-044 §3.3 / #422). Consumed via `include_str!`; the
/// per-crate reader below is intentionally self-contained (no shared Rust
/// crate, no new dev-dep — clean-arch R1 / no-monolith), the same accepted
/// duplication as `parse_dependency_names()`.
const DEP_RULES: &str = include_str!("../../../.cfdb/workspace-dep-rules.toml");

/// This crate's section name in the inert rules file.
const CRATE_SECTION: &str = "cfdb-query";

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
        allowed.contains("cfdb-core") && forbidden.contains("cfdb-cli"),
        "expected sentinel rows missing — rules file shape changed unexpectedly"
    );
}

#[test]
fn cfdb_query_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let (_, forbidden_rules) = dep_rules_for(CRATE_SECTION);
    let forbidden: Vec<&String> = forbidden_rules
        .iter()
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
    let (allowed, _) = dep_rules_for(CRATE_SECTION);
    let unknown: Vec<&String> = deps.iter().filter(|d| !allowed.contains(*d)).collect();

    assert!(
        unknown.is_empty(),
        "cfdb-query/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {allowed:?}\n\
         Update the [cfdb-query] section of .cfdb/workspace-dep-rules.toml AND \
         justify why the crate is parser-layer in a comment there."
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

/// The taxonomy moved out of this crate into cfdb-classify; the parser must
/// never grow an edge back to it in any Cargo section — the direction is
/// parser → AST → judgments, never reverse.
#[test]
fn cfdb_query_never_links_cfdb_classify_in_any_section() {
    let offending: Vec<&str> = CARGO_TOML
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter(|l| l.contains("cfdb-classify"))
        .collect();
    assert!(
        offending.is_empty(),
        "cfdb-query/Cargo.toml names cfdb-classify (dependencies or dev-dependencies): {offending:?}"
    );
}
