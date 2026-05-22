//! AC-3 fixture for issue #345 (RFC-039 §7.4):
//!   "A fixture with 10% null `bounded_context` returns 1 row."
//!
//! Mirrors the structure of `self_enrich_deprecation_fixture.rs` —
//! exercises the pure helpers + the actual shipped template through
//! `runner::substitute_named` to assert the materialized Cypher's
//! WHERE predicate fires under a 10%-empty simulation. The end-to-end
//! `cfdb extract` → `cfdb violations` pipeline is exercised at CI step
//! `.gitea/workflows/ci.yml` against cfdb-self (AC-2).
//!
//! Path B from #355 splits the ratio computation across the harness
//! and the Cypher: the harness substitutes a derived
//! `{{ nulls_threshold }}` (an absolute count = `total * (100 -
//! threshold) / 100`); the Cypher then asserts the flat comparison
//! `empty_count > {{ nulls_threshold }}`. This test pins both halves.

use std::fs;

use dogfood_enrich::runner;

const TEMPLATE_REL_PATH: &str = "../../.cfdb/queries/self-enrich-bounded-context.cypher";

/// Compute the nulls_threshold the harness would produce for a given
/// `(total_items, threshold_pct)` pair. Mirrors the inline math in
/// `main::compute_extra_substitutions` for the
/// `enrich-bounded-context` arm; pulling the mirror into the test
/// keeps the assertion self-contained without exposing harness-private
/// implementation as a public API.
fn nulls_threshold_for(total_items: usize, threshold_pct: u32) -> usize {
    total_items.saturating_mul(100usize.saturating_sub(threshold_pct as usize)) / 100
}

/// At small fixture scale (10 items, 95% threshold), integer division
/// floors the nulls_threshold to 0 — any single empty `bounded_context`
/// fires the sentinel. This is the AC-3 contract.
#[test]
fn small_fixture_floors_nulls_threshold_to_zero() {
    let nulls_threshold = nulls_threshold_for(10, 95);
    assert_eq!(
        nulls_threshold, 0,
        "10-item / 95% fixture must floor nulls_threshold to 0; got {nulls_threshold}"
    );
}

/// At cfdb-self scale (~1869 items, 95% threshold), the nulls_threshold
/// is comfortably above the AC-3 trigger — verifies the same math
/// works at production keyspace size.
#[test]
fn cfdb_self_scale_nulls_threshold_is_5_percent_of_total() {
    let nulls_threshold = nulls_threshold_for(1869, 95);
    // floor(1869 * 5 / 100) = floor(93.45) = 93
    assert_eq!(nulls_threshold, 93);
}

/// Drives runner::substitute_named end-to-end against the actual
/// shipped template and asserts that a 10%-empty simulation
/// (empty_count = 1, nulls_threshold = 0) makes the materialized
/// Cypher's WHERE predicate true — i.e. the sentinel returns ≥1 row.
#[test]
fn ten_percent_null_fixture_satisfies_sentinel_predicate() {
    let template = fs::read_to_string(TEMPLATE_REL_PATH)
        .unwrap_or_else(|e| panic!("read shipped template at {TEMPLATE_REL_PATH}: {e}"));
    assert!(
        template.contains("{{ nulls_threshold }}"),
        "template must reference {{{{ nulls_threshold }}}} for the harness to substitute it; \
         drift detected"
    );
    assert!(
        template.contains("{{ total_items }}"),
        "template must reference {{{{ total_items }}}} so the row carries reviewer context; \
         drift detected"
    );

    // 10-item fixture, 1 empty (10%). Per nulls_threshold_for above,
    // nulls_threshold floors to 0 — so any empty fires the sentinel.
    let total_items = 10usize;
    let threshold_pct = 95u32;
    let nulls_threshold = nulls_threshold_for(total_items, threshold_pct);

    let materialized = runner::substitute_named(
        &template,
        &[
            ("total_items", &total_items.to_string()),
            ("nulls_threshold", &nulls_threshold.to_string()),
        ],
    );
    assert!(
        !materialized.contains("{{ nulls_threshold }}"),
        "post-substitution template must have no remaining {{{{ nulls_threshold }}}} placeholder"
    );
    assert!(
        !materialized.contains("{{ total_items }}"),
        "post-substitution template must have no remaining {{{{ total_items }}}} placeholder"
    );

    // The materialized WHERE must read `empty_count > 0` for the
    // 10-item / 95% fixture — that's the predicate that fires when
    // ≥1 :Item carries an empty bounded_context.
    assert!(
        materialized.contains("empty_count > 0"),
        "materialized template must contain the AC-3-firing comparison \
         `empty_count > 0`; got:\n{materialized}"
    );

    // Sanity-pin the fired-row return shape so a column rename is
    // caught at test time, not at first-CI-failure debug time.
    assert!(
        materialized.contains("RETURN empty_count"),
        "materialized template must RETURN empty_count column for the violation row"
    );
}

/// Pin the cfdb-self-scale predicate too — a regression that drops
/// `bounded_context` on 100 items at HEAD scale (~5%) must still fire.
/// Avoids the small-scale floor-to-zero special case.
#[test]
fn cfdb_self_scale_high_empty_count_satisfies_sentinel_predicate() {
    let template = fs::read_to_string(TEMPLATE_REL_PATH)
        .unwrap_or_else(|e| panic!("read shipped template at {TEMPLATE_REL_PATH}: {e}"));

    let total_items = 1869usize;
    let threshold_pct = 95u32;
    let nulls_threshold = nulls_threshold_for(total_items, threshold_pct);
    assert_eq!(nulls_threshold, 93, "guarded above; pin again here");

    let materialized = runner::substitute_named(
        &template,
        &[
            ("total_items", &total_items.to_string()),
            ("nulls_threshold", &nulls_threshold.to_string()),
        ],
    );

    // 100 empty items at this scale > 93 threshold → sentinel fires.
    assert!(
        materialized.contains("empty_count > 93"),
        "materialized template must contain the cfdb-self-scale comparison \
         `empty_count > 93`; got:\n{materialized}"
    );
}
