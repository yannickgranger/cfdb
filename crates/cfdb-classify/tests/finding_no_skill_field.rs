use cfdb_classify::Finding;

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
             to cfdb (RFC-cfdb §A2.3). Keys present: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn finding_carries_exactly_the_canonical_column_set() {
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
