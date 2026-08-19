use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_CRATE_FRAGMENTS: &[&str] = &[
    "ra-ap-",
    "ra_ap_",
    "cfdb-hir-extractor",
    "cfdb-hir-petgraph-adapter",
];

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("crate manifest resolves to <workspace>/crates/<crate>")
        .to_path_buf()
}

fn direct_dependencies(crate_manifest: &Path) -> HashSet<String> {
    let contents = fs::read_to_string(crate_manifest)
        .unwrap_or_else(|e| panic!("read {} failed: {e}", crate_manifest.display()));

    let mut deps = HashSet::new();
    let mut in_deps_section = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps_section = trimmed == "[dependencies]";
            continue;
        }
        if !in_deps_section {
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.contains("optional = true") {
            continue;
        }
        if let Some(name_end) = trimmed.find(['=', '.']) {
            let name = trimmed[..name_end].trim();
            if !name.is_empty() {
                deps.insert(name.to_string());
            }
        }
    }

    deps
}

fn workspace_crates(root: &Path) -> Vec<String> {
    let manifest_path = root.join("Cargo.toml");
    let contents = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("read {} failed: {e}", manifest_path.display()));

    let mut members: Vec<String> = Vec::new();
    let mut in_members = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members") {
            in_members = true;
        }
        if in_members {
            if let Some(start) = trimmed.find('"') {
                let rest = &trimmed[start + 1..];
                if let Some(end) = rest.find('"') {
                    let path = &rest[..end];
                    if let Some(name) = path.strip_prefix("crates/") {
                        members.push(name.to_string());
                    }
                }
            }
            if trimmed.ends_with(']') {
                break;
            }
        }
    }
    members
}

fn cli_transitive_workspace_deps(root: &Path) -> HashSet<String> {
    let members = workspace_crates(root);
    let member_set: HashSet<String> = members.iter().cloned().collect();

    let mut visited = HashSet::new();
    let mut frontier: Vec<String> = vec!["cfdb-cli".to_string()];

    while let Some(crate_name) = frontier.pop() {
        if !visited.insert(crate_name.clone()) {
            continue;
        }
        let manifest = root.join("crates").join(&crate_name).join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        for dep in direct_dependencies(&manifest) {
            if member_set.contains(&dep) {
                frontier.push(dep);
            }
        }
    }

    visited
}

#[test]
fn cli_workspace_closure_contains_no_hir_crate() {
    let root = workspace_root();
    let closure = cli_transitive_workspace_deps(&root);

    assert!(
        closure.contains("cfdb-cli"),
        "non-vacuity guard: cli closure must at least contain cfdb-cli; got {closure:?}",
    );
    assert!(
        closure.contains("cfdb-core"),
        "non-vacuity guard: cli closure must contain cfdb-core; got {closure:?}",
    );

    for forbidden in ["cfdb-hir-extractor", "cfdb-hir-petgraph-adapter"] {
        assert!(
            !closure.contains(forbidden),
            "cfdb-cli transitively depends on `{forbidden}` — RFC-032 §3 lines 221–227 \
             violation. The 90-150s `ra-ap-*` cold-compile cost must NOT land on every \
             CLI build. Route HIR access through a feature flag (Issue #86 / slice 4) \
             instead of a direct Cargo.toml entry. Full closure: {closure:?}",
        );
    }
}

#[test]
fn adapter_direct_dependencies_include_trait_source_and_target_type() {
    let manifest = workspace_root()
        .join("crates")
        .join("cfdb-hir-petgraph-adapter")
        .join("Cargo.toml");
    let deps = direct_dependencies(&manifest);

    for required in ["cfdb-core", "cfdb-hir-extractor", "cfdb-petgraph"] {
        assert!(
            deps.contains(required),
            "adapter must declare `{required}` in [dependencies] for orphan-rule \
             compliance; actual deps: {deps:?}",
        );
    }
}

#[test]
fn adapter_crate_does_not_reference_ra_ap_directly() {
    let src = workspace_root()
        .join("crates")
        .join("cfdb-hir-petgraph-adapter")
        .join("src");
    for entry in
        fs::read_dir(&src).unwrap_or_else(|e| panic!("read_dir {} failed: {e}", src.display()))
    {
        let path = entry.expect("readable dir entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
            for (lineno, line) in contents.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for fragment in FORBIDDEN_CRATE_FRAGMENTS {
                    if *fragment == "cfdb-hir-extractor" || *fragment == "cfdb-hir-petgraph-adapter"
                    {
                        continue;
                    }
                    assert!(
                        !line.contains(fragment),
                        "{}:{}: forbidden fragment `{fragment}` in adapter source — \
                         HIR-type handling belongs in cfdb-hir-extractor, not the \
                         adapter. Line: {}",
                        path.display(),
                        lineno + 1,
                        line.trim(),
                    );
                }
            }
        }
    }
}
