use super::*;
use crate::schema::SchemaVersion;

#[test]
fn schema_version_compat() {
    let reader = SchemaVersion::new(0, 1, 0);
    assert!(reader.can_read(&SchemaVersion::new(0, 1, 0)));
    assert!(!reader.can_read(&SchemaVersion::new(0, 1, 1)));
    assert!(!reader.can_read(&SchemaVersion::new(1, 0, 0)));
}

#[test]
fn label_serde_round_trip() {
    let l = Label::new(Label::ITEM);
    let json = serde_json::to_string(&l).expect("Label is a transparent String newtype");
    assert_eq!(json, "\"Item\"");
    let back: Label = serde_json::from_str(&json).expect("round-trip of just-serialized Label");
    assert_eq!(l, back);
}

#[test]
fn edge_label_serde_round_trip() {
    let e = EdgeLabel::new(EdgeLabel::CALLS);
    let json = serde_json::to_string(&e).expect("EdgeLabel is a transparent String newtype");
    assert_eq!(json, "\"CALLS\"");
    let back: EdgeLabel =
        serde_json::from_str(&json).expect("round-trip of just-serialized EdgeLabel");
    assert_eq!(e, back);
}

#[test]
fn keyspace_serde_round_trip() {
    let k = Keyspace::new("qbot-core");
    let json = serde_json::to_string(&k).expect("Keyspace is a transparent String newtype");
    assert_eq!(json, "\"qbot-core\"");
    let back: Keyspace =
        serde_json::from_str(&json).expect("round-trip of just-serialized Keyspace");
    assert_eq!(k, back);
}

#[test]
fn schema_version_serde_round_trip() {
    let v = SchemaVersion::V0_1_0;
    let json = serde_json::to_string(&v).expect("SchemaVersion has a plain derived Serialize");
    let back: SchemaVersion =
        serde_json::from_str(&json).expect("round-trip of just-serialized SchemaVersion");
    assert_eq!(v, back);
}

#[test]
fn literal_label_serde_round_trip() {
    let l = Label::new(Label::LITERAL);
    let json = serde_json::to_string(&l).expect("Label is a transparent String newtype");
    assert_eq!(json, "\"Literal\"");
    let back: Label = serde_json::from_str(&json).expect("round-trip of just-serialized Label");
    assert_eq!(l, back);
}

#[test]
fn schema_version_v0_8_0_is_current_and_g4_monotonic() {
    assert_eq!(SchemaVersion::CURRENT, SchemaVersion::V0_8_0);
    assert!(SchemaVersion::CURRENT > SchemaVersion::V0_7_0);
    assert_eq!(SchemaVersion::CURRENT.major, SchemaVersion::V0_7_0.major);
    assert!(SchemaVersion::CURRENT.can_read(&SchemaVersion::V0_7_0));
    assert!(!SchemaVersion::V0_7_0.can_read(&SchemaVersion::CURRENT));
}

#[test]
fn schema_version_current_is_exhaustive_maximum() {
    const ALL_VERSIONS: &[SchemaVersion] = &[
        SchemaVersion::V0_1_0,
        SchemaVersion::V0_1_1,
        SchemaVersion::V0_1_2,
        SchemaVersion::V0_1_3,
        SchemaVersion::V0_1_4,
        SchemaVersion::V0_2_0,
        SchemaVersion::V0_2_1,
        SchemaVersion::V0_2_2,
        SchemaVersion::V0_2_3,
        SchemaVersion::V0_3_0,
        SchemaVersion::V0_3_1,
        SchemaVersion::V0_3_2,
        SchemaVersion::V0_4_0,
        SchemaVersion::V0_5_0,
        SchemaVersion::V0_6_0,
        SchemaVersion::V0_7_0,
        SchemaVersion::V0_8_0,
    ];

    for window in ALL_VERSIONS.windows(2) {
        assert!(
            window[0] < window[1],
            "ALL_VERSIONS is not strictly ascending at {:?} → {:?}; \
             ensure versions are listed in declaration order and no duplicates exist",
            window[0],
            window[1],
        );
    }

    let last = ALL_VERSIONS.last().expect("ALL_VERSIONS must be non-empty");
    assert_eq!(
        SchemaVersion::CURRENT,
        *last,
        "SchemaVersion::CURRENT ({}) does not equal the last entry in \
         ALL_VERSIONS ({}). Either add the new version const to ALL_VERSIONS \
         here, or advance SchemaVersion::CURRENT in labels.rs to match.",
        SchemaVersion::CURRENT,
        last,
    );

    assert!(
        ALL_VERSIONS.len() >= 12,
        "ALL_VERSIONS has fewer entries than expected — likely incomplete",
    );
}
