use std::fs;

use dogfood_enrich::runner;

const TEMPLATE_REL_PATH: &str = "../../.cfdb/queries/self-enrich-bounded-context.cypher";

fn nulls_threshold_for(total_items: usize, threshold_pct: u32) -> usize {
    total_items.saturating_mul(100usize.saturating_sub(threshold_pct as usize)) / 100
}

#[test]
fn small_fixture_floors_nulls_threshold_to_zero() {
    let nulls_threshold = nulls_threshold_for(10, 95);
    assert_eq!(
        nulls_threshold, 0,
        "10-item / 95% fixture must floor nulls_threshold to 0; got {nulls_threshold}"
    );
}

#[test]
fn cfdb_self_scale_nulls_threshold_is_5_percent_of_total() {
    let nulls_threshold = nulls_threshold_for(1869, 95);
    assert_eq!(nulls_threshold, 93);
}

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

    assert!(
        materialized.contains("empty_count > 0"),
        "materialized template must contain the AC-3-firing comparison \
         `empty_count > 0`; got:\n{materialized}"
    );

    assert!(
        materialized.contains("RETURN empty_count"),
        "materialized template must RETURN empty_count column for the violation row"
    );
}

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

    assert!(
        materialized.contains("empty_count > 93"),
        "materialized template must contain the cfdb-self-scale comparison \
         `empty_count > 93`; got:\n{materialized}"
    );
}
