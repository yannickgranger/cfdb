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
fn schema_version_v0_5_0_is_current_and_g4_monotonic() {
    // RFC-043 §3.4: :Argument is a new fact type ⇒ minor bump within
    // major 0. CURRENT advances from V0_4_0 to V0_5_0.
    assert_eq!(SchemaVersion::CURRENT, SchemaVersion::V0_5_0);
    assert!(SchemaVersion::CURRENT > SchemaVersion::V0_4_0);
    // Same major — additive within 0.x (G4).
    assert_eq!(SchemaVersion::CURRENT.major, SchemaVersion::V0_4_0.major);
    // A V0_5_0 reader can read a V0_4_0 graph (older minor, same major).
    assert!(SchemaVersion::CURRENT.can_read(&SchemaVersion::V0_4_0));
    // A V0_4_0 reader refuses a V0_5_0 graph (newer minor — G4 reject).
    assert!(!SchemaVersion::V0_4_0.can_read(&SchemaVersion::CURRENT));
}

// ---- RFC-044 §3.1 sub-band 2: SchemaVersion::CURRENT lockstep ----------

/// RFC-044 §3.1 — SchemaVersion::CURRENT lockstep completeness constant.
///
/// This test declares an exhaustive `ALL_VERSIONS` array containing every
/// `pub const V*` on `SchemaVersion` (in declaration order). It asserts:
///
/// (a) The array is strictly ascending (sorted, no duplicates) — versions
///     are ordered by (major, minor, patch), consistent with `Ord`.
/// (b) `SchemaVersion::CURRENT` equals `*ALL_VERSIONS.last().unwrap()` —
///     CURRENT is the most recent declared version.
///
/// **What this catches:** "Added a new `V0_N_M` const but forgot to advance
/// `CURRENT`." After adding a new version constant, the developer must also
/// update both `ALL_VERSIONS` here AND `SchemaVersion::CURRENT` in labels.rs.
/// The test fails at compile time if `ALL_VERSIONS` is out of date (because
/// the comparison to CURRENT will fail); the doc comment is the explicit review
/// surface.
#[test]
fn schema_version_current_is_exhaustive_maximum() {
    // ---- Completeness constant ----
    // EVERY `pub const V*` declared on SchemaVersion MUST appear in this
    // array. When you add a new version constant, add it here too.
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
    ];

    // (a) Strictly ascending — no duplicates, each entry < next.
    for window in ALL_VERSIONS.windows(2) {
        assert!(
            window[0] < window[1],
            "ALL_VERSIONS is not strictly ascending at {:?} → {:?}; \
             ensure versions are listed in declaration order and no duplicates exist",
            window[0],
            window[1],
        );
    }

    // (b) CURRENT == last entry — exhaustiveness guard.
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

    // Non-vacuity guard — the array must cover all known versions.
    assert!(
        ALL_VERSIONS.len() >= 12,
        "ALL_VERSIONS has fewer entries than expected — likely incomplete",
    );
}
