//! Architecture test — cfdb-cli must NOT depend (directly OR
//! transitively) on this adapter crate, cfdb-hir-extractor, or any
//! `ra-ap-*` crate.
//!
//! This is the load-bearing drift tripwire from #85 AC-12 /
//! rust-systems CRITICAL finding in the architect decomposition of
//! #40: because `cfdb-cli → cfdb-petgraph` already exists, any
//! `cfdb-petgraph` dep on this adapter would transitively contaminate
//! `cfdb-cli` with the 90–150s `ra-ap-*` cold-compile cost (RFC-032
//! §3 lines 221–227).
//!
//! The fix is structural — this adapter lives in its OWN crate that
//! `cfdb-cli` does not import by default. Slice 4 (Issue #86) adds a
//! `hir` feature flag that conditionally pulls it. This test catches
//! any accidental reintroduction of a direct-or-transitive arrow
//! from `cfdb-cli` to anything HIR-tainted.
//!
//! **Why this test lives in the adapter crate, not cfdb-cli.** The
//! adapter is the component that must STAY OUT of cfdb-cli's tree.
//! Its existence proves a contract cfdb-cli must honor; testing it
//! here co-locates the invariant with the source of the risk.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Names / token fragments that MUST NOT appear anywhere on
/// cfdb-cli's dependency path. Matches `cargo tree` output lines
/// whether they use the `ra-ap-` (hyphen) or `ra_ap_` (underscore)
/// form on package names.
const FORBIDDEN_CRATE_FRAGMENTS: &[&str] = &[
    "ra-ap-",
    "ra_ap_",
    "cfdb-hir-extractor",
    "cfdb-hir-petgraph-adapter",
];

/// Workspace root two `..` up from `CARGO_MANIFEST_DIR`.
fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("crate manifest resolves to <workspace>/crates/<crate>")
        .to_path_buf()
}

/// Read the direct `[dependencies]` table from a crate's Cargo.toml
/// as a set of package-name strings. The parsing is intentionally
/// tolerant: for each line inside the `[dependencies]` section that
/// looks like `name = ...` or `name.workspace = true`, record the
/// name before the first `=` or `.`.
fn direct_dependencies(crate_manifest: &Path) -> HashSet<String> {
    let contents = fs::read_to_string(crate_manifest)
        .unwrap_or_else(|e| panic!("read {} failed: {e}", crate_manifest.display()));

    let mut deps = HashSet::new();
    let mut in_deps_section = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Track only the top-level `[dependencies]` section. The
            // `[dev-dependencies]` table is excluded because tests
            // can legitimately reuse adapter fixtures without landing
            // the adapter into the CLI's release build.
            in_deps_section = trimmed == "[dependencies]";
            continue;
        }
        if !in_deps_section {
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Parse the package name: everything before the first `=` or
        // `.`, trimmed of whitespace.
        if let Some(name_end) = trimmed.find(['=', '.']) {
            let name = trimmed[..name_end].trim();
            if !name.is_empty() {
                deps.insert(name.to_string());
            }
        }
    }

    deps
}

/// All in-workspace crate names (reading `Cargo.toml` `members`).
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
            // `"crates/cfdb-foo",`  →  `cfdb-foo`
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

/// Walk cfdb-cli's transitive direct-dependency closure within the
/// workspace, following only `path = "../X"` / `X.workspace = true`
/// entries that resolve to workspace members. External deps
/// (`petgraph`, `serde`, etc.) are ignored because they are the
/// designed dependency surface; the test is about WORKSPACE-internal
/// contamination.
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
            // Only recurse into workspace members — external crates
            // are not the subject of this test.
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

    // Sanity: cfdb-cli's closure must contain cfdb-cli itself plus
    // its known direct workspace deps (cfdb-core, cfdb-extractor,
    // cfdb-petgraph, cfdb-query, cfdb-recall). A non-vacuous result
    // guards against the walk resolving incorrectly and silently
    // passing on an empty set.
    assert!(
        closure.contains("cfdb-cli"),
        "non-vacuity guard: cli closure must at least contain cfdb-cli; got {closure:?}",
    );
    assert!(
        closure.contains("cfdb-core"),
        "non-vacuity guard: cli closure must contain cfdb-core; got {closure:?}",
    );

    // The forbidden set: if ANY of these appears in the closure, the
    // boundary is breached.
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

    // Orphan-rule contract: this crate must depend on BOTH the trait
    // source (cfdb-hir-extractor) and the target type crate
    // (cfdb-petgraph). Either missing would make the impl ill-formed.
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
    // The adapter works on `(Vec<Node>, Vec<Edge>)` post-extraction
    // facts and MUST NOT import any `ra-ap-*` crate. All HIR-type
    // handling stays in `cfdb-hir-extractor` (#85c onward).
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
                // Skip comment-only lines — the module header
                // legitimately cites `ra-ap-*` for architectural
                // rationale.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for fragment in FORBIDDEN_CRATE_FRAGMENTS {
                    if *fragment == "cfdb-hir-extractor" || *fragment == "cfdb-hir-petgraph-adapter"
                    {
                        // The adapter legitimately references its
                        // own trait source; skip self-refs here. The
                        // other test catches cli contamination.
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
