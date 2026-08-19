use std::collections::{BTreeMap, BTreeSet};

use cargo_metadata::{DependencyKind, Package};

use crate::ExtractError;

type Adjacency = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn compute_crate_tiers(
    packages: &[&Package],
) -> Result<BTreeMap<String, i64>, ExtractError> {
    longest_path_tiers(&normal_workspace_adjacency(packages))
}

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

fn longest_path_tiers(adjacency: &Adjacency) -> Result<BTreeMap<String, i64>, ExtractError> {
    let mut tiers: BTreeMap<String, i64> = BTreeMap::new();
    let mut on_stack: BTreeSet<String> = BTreeSet::new();
    for name in adjacency.keys() {
        tier_of(name, adjacency, &mut tiers, &mut on_stack)?;
    }
    Ok(tiers)
}

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
