use super::*;

#[test]
fn schema_version_compat() {
    let reader = SchemaVersion::new(0, 1, 0);
    assert!(reader.can_read(&SchemaVersion::new(0, 1, 0)));
    assert!(!reader.can_read(&SchemaVersion::new(0, 1, 1))); // newer minor: no
    assert!(!reader.can_read(&SchemaVersion::new(1, 0, 0))); // different major: no
}

// ---- Serde round-trip tests (#3625 AC) ---------------------------------

#[test]
fn label_serde_round_trip() {
    let l = Label::new(Label::ITEM);
    let json = serde_json::to_string(&l).expect("Label is a transparent String newtype");
    // #[serde(transparent)] flattens to a bare string.
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

// ---- RFC-041 slice 041-A (#369): :Literal vocabulary ------------------

#[test]
fn literal_label_serde_round_trip() {
    let l = Label::new(Label::LITERAL);
    let json = serde_json::to_string(&l).expect("Label is a transparent String newtype");
    // #[serde(transparent)] flattens to a bare string.
    assert_eq!(json, "\"Literal\"");
    let back: Label = serde_json::from_str(&json).expect("round-trip of just-serialized Label");
    assert_eq!(l, back);
}

#[test]
fn schema_version_v0_4_0_is_current_and_g4_monotonic() {
    // RFC-041 §3.3: :Literal is purely additive ⇒ minor bump within
    // major 0. CURRENT advances from V0_3_2 to V0_4_0.
    assert_eq!(SchemaVersion::CURRENT, SchemaVersion::V0_4_0);
    assert!(SchemaVersion::CURRENT > SchemaVersion::V0_3_2);
    // Same major — additive within 0.x (G4).
    assert_eq!(SchemaVersion::CURRENT.major, SchemaVersion::V0_3_2.major);
    // A V0_4_0 reader can read a V0_3_2 graph (older minor, same major).
    assert!(SchemaVersion::CURRENT.can_read(&SchemaVersion::V0_3_2));
    // A V0_3_2 reader refuses a V0_4_0 graph (newer minor — G4 reject).
    assert!(!SchemaVersion::V0_3_2.can_read(&SchemaVersion::CURRENT));
}
