use super::workspace_nodes::{accumulate_heuristic_context, seed_declared_contexts};
use cfdb_concepts::{compute_bounded_context, ConceptOverrides, ContextMeta};
use cfdb_core::ContextSource;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tempfile::TempDir;

fn overrides_with_one_context(context_name: &str, crates: &[&str]) -> (TempDir, ConceptOverrides) {
    let tmp = TempDir::new().expect("tempdir");
    let dir: PathBuf = tmp.path().join(".cfdb").join("concepts");
    std::fs::create_dir_all(&dir).expect("mkdir concepts");
    let mut body = format!("name = \"{context_name}\"\ncrates = [");
    for c in crates {
        body.push_str(&format!("\"{c}\","));
    }
    body.push_str("]\n");
    std::fs::write(dir.join(format!("{context_name}.toml")), body).expect("write toml");
    let loaded = cfdb_concepts::load_concept_overrides(tmp.path()).expect("load overrides");
    (tmp, loaded)
}

fn run_accumulator(
    overrides: &ConceptOverrides,
    crate_visit_order: &[&str],
) -> BTreeMap<String, (ContextMeta, ContextSource)> {
    let mut acc = seed_declared_contexts(overrides);
    for crate_name in crate_visit_order {
        let bc = compute_bounded_context(crate_name, overrides);
        accumulate_heuristic_context(&mut acc, &bc.name);
    }
    acc
}

#[test]
fn declared_plus_heuristic_resolves_to_declared() {
    let (_tmp, overrides) = overrides_with_one_context("trading", &["messenger"]);
    let acc = run_accumulator(&overrides, &["messenger", "domain-trading"]);
    let (_, source) = acc.get("trading").expect("trading context present");
    assert_eq!(
        *source,
        ContextSource::Declared,
        "declared+heuristic mixed → context source must be Declared"
    );
}

#[test]
fn heuristic_plus_heuristic_resolves_to_heuristic() {
    let overrides = ConceptOverrides::default();
    let acc = run_accumulator(&overrides, &["domain-trading", "ports-trading"]);
    let (_, source) = acc.get("trading").expect("trading context present");
    assert_eq!(
        *source,
        ContextSource::Heuristic,
        "heuristic+heuristic → context source must be Heuristic"
    );
}

#[test]
fn declared_plus_declared_resolves_to_declared() {
    let (_tmp, overrides) = overrides_with_one_context("trading", &["messenger", "ledger"]);
    let acc = run_accumulator(&overrides, &["messenger", "ledger"]);
    let (_, source) = acc.get("trading").expect("trading context present");
    assert_eq!(
        *source,
        ContextSource::Declared,
        "declared+declared → context source must be Declared"
    );
}

#[test]
fn visitation_order_independence() {
    let (_tmp, overrides) = overrides_with_one_context("trading", &["messenger"]);

    let acc_declared_first = run_accumulator(
        &overrides,
        &["messenger", "domain-trading", "ports-trading"],
    );
    let acc_heuristic_first = run_accumulator(
        &overrides,
        &["ports-trading", "domain-trading", "messenger"],
    );

    let project = |acc: &BTreeMap<String, (ContextMeta, ContextSource)>| {
        acc.iter()
            .map(|(name, (_meta, source))| (name.clone(), *source))
            .collect::<BTreeMap<String, ContextSource>>()
    };
    assert_eq!(
        project(&acc_declared_first),
        project(&acc_heuristic_first),
        "(name, source) tuple set must be invariant under visitation order"
    );
    assert_eq!(
        acc_declared_first.get("trading").map(|(_, s)| *s),
        Some(ContextSource::Declared),
    );
}
