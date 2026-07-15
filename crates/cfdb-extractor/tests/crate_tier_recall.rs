//! RFC-050 50-A recall gate — the extractor's emitted `:Crate.crate_tier`
//! equals the topological longest-path depth of the intra-workspace
//! normal-`[dependencies]` manifest DAG, for EVERY crate in cfdb's own tree.
//!
//! The expected depths are recomputed here by an **independent** reference —
//! iterative relaxation to a fixpoint (Bellman-Ford style) — deliberately a
//! different algorithm from the production memoised DFS in
//! `crate_tier::longest_path_tiers`, so a bug in one does not hide in the
//! other.
//!
//! **Documented deviation (CLAUDE.md §5 / RFC-050 §4 "recall extension").**
//! `crate_tier` has no rustdoc ground truth, so this gate does NOT live in
//! `cfdb-recall`: that crate measures public-item-qname recall against
//! `cargo public-api` / `rustdoc --output-format json`, which is structurally
//! incompatible with a crate-level Cargo-manifest fact. The manifest DAG
//! itself is `crate_tier`'s source of truth; this test is its equivalence
//! gate, the layering analog of the rustdoc recall gate.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cargo_metadata::{DependencyKind, MetadataCommand};
use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;
use cfdb_extractor::extract_workspace;

fn cfdb_workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-extractor crate dir has parent (crates/)")
        .parent()
        .expect("crates/ has parent (cfdb sub-workspace root)")
}

/// Reference longest-path depth via iterative relaxation: start every node at
/// 0, then repeatedly set `tier[c] = max(tier[c], 1 + max(tier[dep]))` until a
/// pass changes nothing. A DAG converges in `< |V|` passes; the input is a DAG
/// because the production `extract_workspace` above would have returned
/// `CrateTierCycle` (and this test would never reach here) otherwise.
fn reference_tiers(adjacency: &BTreeMap<String, BTreeSet<String>>) -> BTreeMap<String, i64> {
    let mut tier: BTreeMap<String, i64> = adjacency.keys().map(|k| (k.clone(), 0)).collect();
    for _ in 0..adjacency.len() {
        let mut changed = false;
        for (crate_name, deps) in adjacency {
            let want = deps.iter().map(|d| tier[d] + 1).max().unwrap_or(0);
            if want > tier[crate_name] {
                tier.insert(crate_name.clone(), want);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    tier
}

#[test]
fn crate_tier_matches_independent_manifest_dag_for_every_crate() {
    let root = cfdb_workspace_root();

    // Emitted tiers — the production path (cargo metadata + memoised DFS).
    let (nodes, _edges) = extract_workspace(root).expect("extract cfdb sub-workspace");
    let emitted: BTreeMap<String, i64> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CRATE)
        .filter_map(|n| {
            let name = n.id.strip_prefix("crate:")?.to_string();
            match n.props.get("crate_tier") {
                Some(PropValue::Int(i)) => Some((name, *i)),
                _ => None,
            }
        })
        .collect();

    // Independent ground truth: rebuild the normal-deps adjacency from the
    // manifests and relax (a different algorithm).
    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata on cfdb workspace");
    let members: BTreeSet<String> = metadata
        .workspace_packages()
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    let adjacency: BTreeMap<String, BTreeSet<String>> = metadata
        .workspace_packages()
        .iter()
        .map(|p| {
            let name = p.name.to_string();
            let deps = p
                .dependencies
                .iter()
                .filter(|d| d.kind == DependencyKind::Normal)
                .map(|d| d.name.to_string())
                .filter(|d| members.contains(d) && *d != name)
                .collect();
            (name, deps)
        })
        .collect();
    let expected = reference_tiers(&adjacency);

    assert!(
        !emitted.is_empty(),
        "extractor emitted no :Crate crate_tier values"
    );
    assert_eq!(
        emitted, expected,
        "emitted :Crate.crate_tier diverges from the independent \
         manifest-DAG longest-path depth"
    );
}
