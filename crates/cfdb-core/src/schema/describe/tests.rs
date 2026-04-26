use super::super::descriptors::Provenance;
use super::super::labels::Label;
use super::*;

#[test]
fn schema_describe_covers_all_node_labels() {
    let d = schema_describe();
    let labels: Vec<&str> = d.nodes.iter().map(|n| n.label.as_str()).collect();
    // Order follows RFC §6.1 / PLAN-v1 §6.1 table order; `Context` appended
    // per council-cfdb-wiring §B.1.3 (v0.1 minor schema bump, #3727);
    // `RfcDoc` appended per #43-A council round 1 synthesis (reservation
    // only — first emissions land in slice 43-D); `ConstTable` appended
    // per RFC-040 slice 1/5 (issue #323 reservation; first emissions land
    // in slice 3/5, issue #325).
    assert_eq!(
        labels,
        vec![
            "Crate",
            "Module",
            "File",
            "Item",
            "Field",
            "Variant",
            "Param",
            "CallSite",
            "EntryPoint",
            "Concept",
            "Context",
            "RfcDoc",
            "ConstTable",
        ]
    );
}

#[test]
fn schema_describe_covers_all_edge_labels() {
    let d = schema_describe();
    let edges: Vec<&str> = d.edges.iter().map(|e| e.label.as_str()).collect();
    // Every const on EdgeLabel must appear in schema_describe exactly
    // once. `REFERENCED_BY` appended per #43-A (reservation only — first
    // emissions land in slice 43-D alongside `:RfcDoc`); `HAS_CONST_TABLE`
    // appended per RFC-040 slice 1/5 (issue #323 reservation; first
    // emissions land in slice 3/5, issue #325).
    let expected = [
        "IN_CRATE",
        "IN_MODULE",
        "HAS_FIELD",
        "HAS_VARIANT",
        "HAS_PARAM",
        "HAS_CONST_TABLE",
        "TYPE_OF",
        "IMPLEMENTS",
        "IMPLEMENTS_FOR",
        "RETURNS",
        "BELONGS_TO",
        "CALLS",
        "INVOKES_AT",
        "EXPOSES",
        "REGISTERS_PARAM",
        "LABELED_AS",
        "CANONICAL_FOR",
        "EQUIVALENT_TO",
        "REFERENCED_BY",
    ];
    assert_eq!(edges.len(), expected.len());
    for e in &expected {
        assert!(edges.contains(e), "edge {e} missing from schema_describe");
    }
}

#[test]
fn schema_describe_item_has_quality_signals_with_enrich_metrics_provenance() {
    let d = schema_describe();
    let item = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::ITEM)
        .expect("Item node descriptor");
    for name in [
        "unwrap_count",
        "test_coverage",
        "dup_cluster_id",
        "cyclomatic",
    ] {
        let attr = item
            .attributes
            .iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("Item attr {name} missing"));
        assert_eq!(
            attr.provenance,
            Provenance::EnrichMetrics,
            "{name} should be EnrichMetrics-provenanced",
        );
    }
}

/// #106 AC-4 — deprecation facts are extractor-time, not enrichment-time.
/// The `#[deprecated]` attribute is syntactic; cfdb-extractor's AST walker
/// captures it at extraction. Flipping either attr to an `Enrich*`
/// provenance would mis-route the classifier (#48) and contradict the
/// RFC amendment §A2.2 row 3.
#[test]
fn schema_describe_item_deprecation_attrs_are_extractor_provenanced() {
    let d = schema_describe();
    let item = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::ITEM)
        .expect("Item node descriptor");
    for name in ["is_deprecated", "deprecation_since"] {
        let attr = item
            .attributes
            .iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("Item attr {name} missing"));
        assert_eq!(
            attr.provenance,
            Provenance::Extractor,
            "{name} is an extractor-time syntactic fact; any other provenance would mis-route the #48 classifier",
        );
    }
}

#[test]
fn schema_describe_concept_attrs_are_enrich_concepts() {
    let d = schema_describe();
    let concept = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::CONCEPT)
        .expect("Concept node descriptor");
    for a in &concept.attributes {
        assert_eq!(
            a.provenance,
            Provenance::EnrichConcepts,
            "Concept attr {} should be EnrichConcepts",
            a.name,
        );
    }
}

#[test]
fn schema_describe_is_deterministic() {
    // G1: byte-stable. Two calls must produce identical JSON.
    let a = serde_json::to_string(&schema_describe())
        .expect("SchemaDescribe serializes deterministically");
    let b = serde_json::to_string(&schema_describe())
        .expect("SchemaDescribe serializes deterministically");
    assert_eq!(a, b);
}

#[test]
fn schema_describe_round_trips_through_serde() {
    let d = schema_describe();
    let json = serde_json::to_string(&d).expect("SchemaDescribe has derived Serialize");
    let back: super::super::descriptors::SchemaDescribe =
        serde_json::from_str(&json).expect("round-trip of just-serialized SchemaDescribe");
    assert_eq!(d, back);
}

/// Issue #307 — `EQUIVALENT_TO` is reserved-by-design (no producer in v0.x,
/// planned for Phase B). The descriptor's `provenance` MUST be
/// `Provenance::Reserved` and the human-readable description MUST advertise
/// the reservation so consumers can distinguish "no producer because
/// reserved" from "no producer because we forgot."
#[test]
fn schema_describe_equivalent_to_is_reserved() {
    let d = schema_describe();
    let eq = d
        .edges
        .iter()
        .find(|e| e.label.as_str() == "EQUIVALENT_TO")
        .expect("EQUIVALENT_TO descriptor is present in schema_describe");
    assert_eq!(
        eq.provenance,
        Provenance::Reserved,
        "EQUIVALENT_TO must be tagged Provenance::Reserved (issue #307)"
    );
    assert!(
        eq.description.contains("Reserved"),
        "description must advertise reservation: {:?}",
        eq.description
    );
    assert!(
        eq.description.contains("#307"),
        "description must reference issue #307: {:?}",
        eq.description
    );
}

/// Issue #307 — Forbidden move 5: only EQUIVALENT_TO carries the Reserved
/// tag. The other dormant labels on cfdb-self (CALLS, EXPOSES,
/// REGISTERS_PARAM, LABELED_AS, CANONICAL_FOR, REFERENCED_BY) have real
/// producers in `cfdb-hir-extractor` or enrichment passes — they must NOT
/// be silenced via the Reserved tag. This test locks the invariant: exactly
/// one edge label is Reserved, and that one is EQUIVALENT_TO.
#[test]
fn schema_describe_only_equivalent_to_is_reserved() {
    let d = schema_describe();
    let reserved: Vec<&str> = d
        .edges
        .iter()
        .filter(|e| e.provenance == Provenance::Reserved)
        .map(|e| e.label.as_str())
        .collect();
    assert_eq!(
        reserved,
        vec!["EQUIVALENT_TO"],
        "only EQUIVALENT_TO should carry Provenance::Reserved (issue #307); \
         expanding the tag to other dormant labels is a Forbidden move — \
         their proper fix is running the relevant enrich pass, not tagging \
         them reserved"
    );
}
