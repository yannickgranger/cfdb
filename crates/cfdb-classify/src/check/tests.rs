use super::*;

#[test]
fn trigger_id_display_is_uppercase_tag() {
    assert_eq!(TriggerId::T1.to_string(), "T1");
}

#[test]
fn trigger_id_from_str_round_trips_every_variant() {
    for v in TriggerId::variants() {
        let spelled = v.to_string();
        let parsed: TriggerId = spelled.parse().expect("round-trip");
        assert_eq!(&parsed, v);
    }
}

#[test]
fn trigger_id_from_str_rejects_unknown_with_derived_valid_values() {
    let err = TriggerId::from_str("T999").expect_err("unknown should fail");
    assert_eq!(err.0, "T999");
    let msg = err.to_string();
    for v in TriggerId::variants() {
        assert!(
            msg.contains(v.as_str()),
            "error message missing {}: {msg}",
            v.as_str()
        );
    }
    assert!(
        msg.contains("valid values:"),
        "error message missing preamble: {msg}"
    );
}

#[test]
fn trigger_id_from_str_is_case_sensitive() {
    assert!(TriggerId::from_str("t1").is_err());
    assert!(TriggerId::from_str("T1").is_ok());
    assert!(TriggerId::from_str("t3").is_err());
    assert!(TriggerId::from_str("T3").is_ok());
}

#[test]
fn trigger_id_variants_enumerates_t1_and_t3_in_stable_order() {
    assert_eq!(
        TriggerId::variants(),
        &[TriggerId::T1, TriggerId::T3],
        "TriggerId::variants() order must be T1, T3 — \
         consumer skills rely on stable iteration order"
    );
}
