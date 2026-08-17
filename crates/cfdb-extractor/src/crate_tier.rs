//! `crate_tier` — topological longest-path depth of each workspace crate in
//! the intra-workspace **normal-`[dependencies]`** DAG.
//!
//! A crate with no in-workspace normal dependencies is tier 0; otherwise its
//! tier is `1 + max(crate_tier of its in-workspace normal deps)`. Longest-path
//! (not shortest) is the correct rank — a crate sits one above its *deepest*
//! dependency.
//!
//! **Normal deps only.** dev- and build-dependencies are excluded so the common
//! "test-only back-edge" shape does not cycle: cfdb's own tree has
//! `cfdb-cli --normal--> cfdb-hir-extractor` AND
//! `cfdb-hir-extractor --dev--> cfdb-cli`, which an all-kinds DAG would treat
//! as a cycle.
//!
//! Pure functions over `cargo_metadata` manifests — no I/O. A cycle in the
//! normal-deps DAG is a hard error ([`ExtractError::CrateTierCycle`]).

use std::collections::{BTreeMap, BTreeSet};

use cargo_metadata::{DependencyKind, Package};

use crate::ExtractError;

/// Crate name → its in-workspace normal-dependency crate names.
type Adjacency = BTreeMap<String, BTreeSet<String>>;

/// Compute `crate_tier` for every workspace package, keyed by package name.
///
/// `packages` is the workspace-member set (`metadata.workspace_packages()`);
/// dependencies are filtered to those members so external crates never enter
/// the DAG. Returns [`ExtractError::CrateTierCycle`] if the normal-deps DAG
/// contains a cycle.
pub(crate) fn compute_crate_tiers(
    packages: &[&Package],
) -> Result<BTreeMap<String, i64>, ExtractError> {
    longest_path_tiers(&normal_workspace_adjacency(packages))
}

/// Build the intra-workspace normal-`[dependencies]` adjacency from package
/// manifests: for each package, the subset of its `kind == Normal`
/// dependencies whose name is also a workspace member (self-edges dropped).
/// Deterministic via `BTreeMap`/`BTreeSet`.
fn normal_workspace_adjacency(packages: &[&Package]) -> Adjacency {
    let members: BTreeSet<String> = packages.iter().map(|p| p.name.to_string()).collect();
    packages
        .iter()
        .map(|p| {
            let name = p.name.to_string();
            let deps: BTreeSet<String> = p
                .dependencies
                .iter()
                .filter(|d| d.kind == DependencyKind::Normal)
                .map(|d| d.name.to_string())
                .filter(|dep| members.contains(dep) && *dep != name)
                .collect();
            (name, deps)
        })
        .collect()
}

/// Longest-path depth of every node in `adjacency` (edges point from a crate
/// to the crates it depends on): leaves are tier 0, others are
/// `1 + max(tier of deps)`. Memoised DFS with grey-node cycle detection.
fn longest_path_tiers(adjacency: &Adjacency) -> Result<BTreeMap<String, i64>, ExtractError> {
    let mut tiers: BTreeMap<String, i64> = BTreeMap::new();
    let mut on_stack: BTreeSet<String> = BTreeSet::new();
    for name in adjacency.keys() {
        tier_of(name, adjacency, &mut tiers, &mut on_stack)?;
    }
    Ok(tiers)
}

/// Memoised longest-path visit for one crate. A node already on the current
/// DFS stack (`on_stack`) is a back-edge → cycle. A node already in `tiers`
/// is resolved → return the memoised depth (so DAG cross-/diamond-edges do
/// not re-traverse and do not false-trip the cycle check).
fn tier_of(
    crate_name: &str,
    adjacency: &Adjacency,
    tiers: &mut BTreeMap<String, i64>,
    on_stack: &mut BTreeSet<String>,
) -> Result<i64, ExtractError> {
    if let Some(&t) = tiers.get(crate_name) {
        return Ok(t);
    }
    if !on_stack.insert(crate_name.to_string()) {
        return Err(ExtractError::CrateTierCycle(crate_name.to_string()));
    }
    let mut tier = 0i64;
    if let Some(deps) = adjacency.get(crate_name) {
        for dep in deps {
            tier = tier.max(1 + tier_of(dep, adjacency, tiers, on_stack)?);
        }
    }
    on_stack.remove(crate_name);
    tiers.insert(crate_name.to_string(), tier);
    Ok(tier)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adj(edges: &[(&str, &[&str])]) -> Adjacency {
        edges
            .iter()
            .map(|(c, deps)| {
                (
                    (*c).to_string(),
                    deps.iter().map(|d| (*d).to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn longest_path_matches_hand_computed_depths_on_4_crate_dag() {
        // core (0) <- petgraph (1) <- query (2) <- cli (3); cli also depends
        // directly on core, so the LONGEST path (via query), not the shortest
        // (direct), sets cli's tier.
        let tiers = longest_path_tiers(&adj(&[
            ("core", &[]),
            ("petgraph", &["core"]),
            ("query", &["petgraph", "core"]),
            ("cli", &["query", "core"]),
        ]))
        .expect("acyclic DAG computes tiers");

        assert_eq!(tiers["core"], 0);
        assert_eq!(tiers["petgraph"], 1);
        assert_eq!(tiers["query"], 2);
        assert_eq!(
            tiers["cli"], 3,
            "longest path (via query) wins over the direct core edge"
        );
    }

    #[test]
    fn normal_deps_cycle_is_a_hard_error() {
        let err = longest_path_tiers(&adj(&[("a", &["b"]), ("b", &["a"])]))
            .expect_err("a normal-deps cycle must error, not loop forever");
        assert!(
            matches!(err, ExtractError::CrateTierCycle(_)),
            "expected CrateTierCycle, got {err:?}"
        );
    }

    #[test]
    fn diamond_dag_does_not_false_trip_the_cycle_check() {
        // a -> {b, c} -> d : d is reached twice but is not a cycle.
        let tiers = longest_path_tiers(&adj(&[
            ("a", &["b", "c"]),
            ("b", &["d"]),
            ("c", &["d"]),
            ("d", &[]),
        ]))
        .expect("a diamond is acyclic");
        assert_eq!(tiers["d"], 0);
        assert_eq!(tiers["b"], 1);
        assert_eq!(tiers["c"], 1);
        assert_eq!(tiers["a"], 2);
    }
}
