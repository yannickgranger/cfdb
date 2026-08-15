//! Aggregation rule for bounded contexts — when multiple crates resolve
//! to the same bounded-context name, the emitted `:Context` node's `source`
//! is `Declared` if ANY contributing crate declared it via override, else
//! `Heuristic`. Pre-seeding declared contexts first + `or_insert_with` for
//! heuristic crates implements this implicitly.

use super::workspace_nodes::{accumulate_heuristic_context, seed_declared_contexts};
use cfdb_concepts::{compute_bounded_context, ConceptOverrides, ContextMeta};
use cfdb_core::ContextSource;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tempfile::TempDir;

/// Synthesise a `ConceptOverrides` by writing a single
/// `.cfdb/concepts/<context>.toml` file inside a tempdir and feeding it
/// through the real `cfdb_concepts::load_concept_overrides` loader.
/// Real-infra preferred over hand-built struct.
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

/// Drive the accumulator using the same call sequence as the production
/// extract loop: seed → per-crate heuristic insert (when not pre-seeded).
/// Returns the final accumulator state; visitation order is whatever the
/// caller passes in.
fn run_accumulator(
    overrides: &ConceptOverrides,
    crate_visit_order: &[&str],
) -> BTreeMap<String, (ContextMeta, ContextSource)> {
    let mut acc = seed_declared_contexts(overrides);
    for crate_name in crate_visit_order {
        let bc = compute_bounded_context(crate_name, overrides);
        // Mirror `emit_crate_and_walk_targets`: heuristic-only insert.
        // Pre-seeded declared entries are NEVER overwritten here.
        accumulate_heuristic_context(&mut acc, &bc.name);
    }
    acc
}

/// Case 1: declared + heuristic mixed for the same context name.
/// One TOML-overridden crate maps to context `"trading"`; one
/// prefix-heuristic crate also maps to `"trading"`. The pre-seeded
/// declared entry must NOT be demoted by the heuristic crate.
#[test]
fn declared_plus_heuristic_resolves_to_declared() {
    // Override: `messenger` -> `trading` (no prefix, declared via TOML).
    let (_tmp, overrides) = overrides_with_one_context("trading", &["messenger"]);
    // `domain-trading` is NOT in the override → prefix heuristic strips
    // `domain-` and returns `trading` as the context name.
    let acc = run_accumulator(&overrides, &["messenger", "domain-trading"]);
    let (_, source) = acc.get("trading").expect("trading context present");
    assert_eq!(
        *source,
        ContextSource::Declared,
        "declared+heuristic mixed → context source must be Declared"
    );
}

/// Case 2: heuristic + heuristic only — both crates resolve to the same
/// context name via prefix stripping; neither is in the override file.
/// The aggregated source is `Heuristic`.
#[test]
fn heuristic_plus_heuristic_resolves_to_heuristic() {
    // Empty overrides — every crate resolves heuristically.
    let overrides = ConceptOverrides::default();
    // `domain-trading` and `ports-trading` both strip to `trading`.
    let acc = run_accumulator(&overrides, &["domain-trading", "ports-trading"]);
    let (_, source) = acc.get("trading").expect("trading context present");
    assert_eq!(
        *source,
        ContextSource::Heuristic,
        "heuristic+heuristic → context source must be Heuristic"
    );
}

/// Case 3: declared + declared — two TOML-overridden crates declare the
/// same context. The aggregated source is `Declared`.
#[test]
fn declared_plus_declared_resolves_to_declared() {
    // Both crates declared in the same `trading.toml`.
    let (_tmp, overrides) = overrides_with_one_context("trading", &["messenger", "ledger"]);
    let acc = run_accumulator(&overrides, &["messenger", "ledger"]);
    let (_, source) = acc.get("trading").expect("trading context present");
    assert_eq!(
        *source,
        ContextSource::Declared,
        "declared+declared → context source must be Declared"
    );
}

/// Case 4: visitation-order independence. Shuffling the crate visitation
/// order across runs must produce the identical `(name, source)` tuple
/// set — this is the determinism invariant (RFC-029 §12.1 G1) projected
/// onto the new `source` discriminator.
#[test]
fn visitation_order_independence() {
    let (_tmp, overrides) = overrides_with_one_context("trading", &["messenger"]);

    // Two interleavings of the same crate set: declared-first vs
    // heuristic-first. Both must yield the same final accumulator.
    let acc_declared_first = run_accumulator(
        &overrides,
        &["messenger", "domain-trading", "ports-trading"],
    );
    let acc_heuristic_first = run_accumulator(
        &overrides,
        &["ports-trading", "domain-trading", "messenger"],
    );

    // Project both accumulators to (name, source) sets — drop ContextMeta
    // because its canonical_crate/owning_rfc fields are NOT subject to
    // the aggregation rule (they come straight from the TOML).
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
    // And the trading context must be Declared in both — the override
    // pre-seed wins regardless of when `messenger` is visited.
    assert_eq!(
        acc_declared_first.get("trading").map(|(_, s)| *s),
        Some(ContextSource::Declared),
    );
}
