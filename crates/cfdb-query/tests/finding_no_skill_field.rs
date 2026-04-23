//! Architecture test — `Finding` MUST NOT carry any skill-related field
//! (issue #48, council BLOCK-1 from RFC-cfdb.md §A2.2).
//!
//! Embedding a `fix_skill` / `skill` / `skill_name` field in `Finding`
//! couples the classifier (data layer) to the orchestration policy
//! (skill layer). A skill rename or a `/port-epic`-vs-`/sweep-epic
//! --mode=port` decision would force a graph schema migration. Skill
//! routing lives in `SkillRoutingTable` (loaded from
//! `.cfdb/skill-routing.toml`), a deliberately external concern.
//!
//! This test freezes the JSON shape of `Finding` against the set of
//! forbidden field names. It runs via `serde_json::to_value(...)`
//! round-trip, so schema drift (field rename, field add) is caught
//! regardless of whether serde rename attributes are used.

use cfdb_query::Finding;

/// Every field name that would be a DIP violation if present on
/// `Finding`. If a new skill-related concept is invented, extend this
/// list — the arch test is the canary for schema drift.
const FORBIDDEN_FIELDS: &[&str] = &[
    "fix_skill",
    "skill",
    "skill_name",
    "routing",
    "council_required",
    "mode",
    "concrete_skill",
];

fn sample_finding() -> Finding {
    Finding {
        qname: "some::qname::Foo".to_string(),
        name: "Foo".to_string(),
        kind: "struct".to_string(),
        crate_name: "some-crate".to_string(),
        file: "src/foo.rs".to_string(),
        line: 42,
        bounded_context: "trading".to_string(),
    }
}

#[test]
fn finding_has_no_forbidden_skill_fields() {
    let finding = sample_finding();
    let json = serde_json::to_value(&finding).expect("serialize Finding");
    let obj = json
        .as_object()
        .expect("Finding serializes to a JSON object");
    for forbidden in FORBIDDEN_FIELDS {
        assert!(
            !obj.contains_key(*forbidden),
            "Finding must NOT carry `{forbidden}` — skill routing is external \
             (SkillRoutingTable, §A2.3). Keys present: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn finding_carries_exactly_the_canonical_column_set() {
    // Positive companion to the forbidden-field check: pin the exact
    // field set so renames (not just additions) are caught. If this
    // test fails because a legitimate column was added, update the
    // expected set AND audit the per-class classifier rules' RETURN
    // clauses in examples/queries/classifier-*.cypher — their
    // projections MUST match Finding's fields or `finding_from_row`
    // in cfdb-cli silently drops rows.
    let finding = sample_finding();
    let json = serde_json::to_value(&finding).expect("serialize Finding");
    let obj = json.as_object().expect("object");
    let mut actual: Vec<&String> = obj.keys().collect();
    actual.sort();
    let expected = vec![
        "bounded_context",
        "crate",
        "file",
        "kind",
        "line",
        "name",
        "qname",
    ];
    let actual_strs: Vec<&str> = actual.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        actual_strs, expected,
        "Finding field set drift — classifier rules' RETURN columns and \
         cfdb-cli `finding_from_row` must be updated together"
    );
}

#[test]
fn debtclass_is_not_a_field_on_finding() {
    // `Finding` carries the structural coordinates; the class label
    // lives on the OUTER `ScopeInventory::findings_by_class` map key.
    // A `class` field on `Finding` would be redundant AND would pave
    // the way for a `fix_skill` follow-up field by normalisation of
    // "add another enum-valued column".
    let finding = sample_finding();
    let json = serde_json::to_value(&finding).expect("serialize Finding");
    let obj = json.as_object().expect("object");
    assert!(
        !obj.contains_key("class"),
        "Finding must NOT carry `class` — the class label keys the outer \
         ScopeInventory::findings_by_class map"
    );
    assert!(
        !obj.contains_key("debt_class"),
        "Finding must NOT carry `debt_class` — same reason as `class`"
    );
}
