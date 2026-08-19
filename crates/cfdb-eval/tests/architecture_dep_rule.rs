use std::collections::BTreeSet;

const CARGO_TOML: &str = include_str!("../Cargo.toml");

const DEP_RULES: &str = include_str!("../../../.cfdb/workspace-dep-rules.toml");

const CRATE_SECTION: &str = "cfdb-eval";

fn dep_rules_for(crate_name: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let header = format!("[{crate_name}]");
    let mut allowed = BTreeSet::new();
    let mut forbidden = BTreeSet::new();
    let mut in_section = false;
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
fn cfdb_eval_has_no_forbidden_dependencies() {
    let deps = parse_dependency_names();
    let (_, forbidden_rules) = dep_rules_for(CRATE_SECTION);
    let forbidden: Vec<&String> = forbidden_rules
        .iter()
        .filter(|name| deps.contains(*name))
        .collect();

    assert!(
        forbidden.is_empty(),
        "cfdb-eval/Cargo.toml [dependencies] contains forbidden crates: {forbidden:?}\n\
         cfdb-eval must depend only on cfdb-core — the GraphReader port \
         exists so this crate never needs the concrete storage engine \
         (RFC-057).\n\
         Found dependency set: {deps:?}"
    );
}

#[test]
fn cfdb_eval_dependencies_are_all_whitelisted() {
    let deps = parse_dependency_names();
    let (allowed, _) = dep_rules_for(CRATE_SECTION);
    let unknown: Vec<&String> = deps.iter().filter(|d| !allowed.contains(*d)).collect();

    assert!(
        unknown.is_empty(),
        "cfdb-eval/Cargo.toml [dependencies] contains crates not in the CLEAN-3 whitelist: {unknown:?}\n\
         Allowed: {allowed:?}\n\
         Update the [cfdb-eval] section of .cfdb/workspace-dep-rules.toml AND \
         justify why the crate is evaluator-layer in a comment there."
    );
}

#[test]
fn cfdb_eval_depends_on_cfdb_core() {
    let deps = parse_dependency_names();
    assert!(
        deps.contains("cfdb-core"),
        "cfdb-eval must depend on cfdb-core (GraphBackend/GraphReader/QueryBackend traits live there)"
    );
}
