//! Ingest-time identity-contention detection.
//!
//! `ingest_one_node` replaces an existing node in place when an incoming id
//! is already in the graph (documented additive-load behavior). When the two
//! nodes are *different* — distinct source constructs colliding on one
//! identity — the replace silently drops a real node; this module
//! decides when that deserves a warning and how the warning reads.

use cfdb_core::fact::Node;
use cfdb_core::result::{Warning, WarningKind};

/// Cap on individually-recorded contention warnings per keyspace. Collisions
/// are systemic by construction; an uncapped record would scale heap,
/// keyspace bytes, and per-query clone cost with the tree. Past the cap a
/// single summary row carries the count (`KeyspaceState::materialized_ingest_warnings`).
pub(crate) const CONTENTION_WARNING_CAP: usize = 50;

/// Remedy line attached to the FIRST recorded contention only; the rest
/// would repeat it verbatim.
pub(crate) const CONTENTION_SUGGESTION: &str = "distinct source constructs are contending for one \
     node identity; the earlier one is absent from this graph (RFC-054)";

#[derive(Debug, PartialEq, Eq)]
enum ReingestClass {
    /// The same logical node re-ingested — stays silent (documented
    /// additive-load behavior, e.g. enrich passes re-saving a keyspace).
    Silent,
    /// A different node is claiming an existing identity; the replace
    /// would silently drop the earlier node.
    Contention,
}

/// Pure classifier (values in → values out, zero I/O).
///
/// Same `file` prop ⇒ the same logical node being updated (silent, whatever
/// the other props do — enrich passes legitimately re-save with added attrs);
/// both have `file` and it differs ⇒ contention; either side lacks `file` ⇒
/// full node equality decides (the caller guarantees id equality, so this is
/// `existing == incoming`).
fn classify_reingest(existing: &Node, incoming: &Node) -> ReingestClass {
    match (existing.props.get("file"), incoming.props.get("file")) {
        (Some(a), Some(b)) if a == b => ReingestClass::Silent,
        (Some(_), Some(_)) => ReingestClass::Contention,
        _ if existing == incoming => ReingestClass::Silent,
        _ => ReingestClass::Contention,
    }
}

/// Combined detect step — `Some(warning)` iff the re-ingest is a contention.
/// Keeps the branchy part out of `ingest_one_node` (complexity gate). The
/// suggestion line is attached at the recording site (first warning only).
pub(crate) fn detect_contention(existing: &Node, incoming: &Node) -> Option<Warning> {
    match classify_reingest(existing, incoming) {
        ReingestClass::Contention => Some(contention_warning(existing, incoming)),
        ReingestClass::Silent => None,
    }
}

/// Render the `file` prop for the warning message. Non-string values (e.g.
/// an enrich-written `Null`) render via `Debug` so the message can always
/// explain the classifier's decision; absence is spelled distinctly.
fn file_of(n: &Node) -> String {
    match n.props.get("file") {
        Some(v) => v
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("{v:?}")),
        None => "(no file prop)".to_string(),
    }
}

/// The message names the id and both `file` renderings so the loss is
/// traceable without re-running the extract.
fn contention_warning(existing: &Node, incoming: &Node) -> Warning {
    Warning {
        kind: WarningKind::IdentityContention,
        message: format!(
            "identity contention on `{}`: `{}` replaced by `{}` — the earlier node was dropped",
            incoming.id,
            file_of(existing),
            file_of(incoming)
        ),
        suggestion: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::PropValue;
    use cfdb_core::result::WarningKind;
    use cfdb_core::schema::{Keyspace, Label};

    use crate::PetgraphStore;
    use cfdb_core::store::StoreBackend;

    fn item_in_file(id: &str, qname: &str, file: &str) -> Node {
        Node::new(id, Label::new(Label::ITEM))
            .with_prop("qname", qname)
            .with_prop("file", file)
    }

    fn item_no_file(id: &str, qname: &str) -> Node {
        Node::new(id, Label::new(Label::ITEM)).with_prop("qname", qname)
    }

    // --- classifier (pure fn) ---

    #[test]
    fn identical_node_reingest_is_silent() {
        let a = item_in_file("item:x::main", "x::main", "src/bin/a.rs");
        assert_eq!(classify_reingest(&a, &a.clone()), ReingestClass::Silent);
    }

    #[test]
    fn same_file_prop_update_is_silent() {
        let a = item_in_file("item:x::main", "x::main", "src/bin/a.rs");
        let updated = a.clone().with_prop("reachable", "true");
        assert_eq!(classify_reingest(&a, &updated), ReingestClass::Silent);
    }

    #[test]
    fn different_file_is_contention() {
        let a = item_in_file("item:x::main", "x::main", "src/bin/a.rs");
        let b = item_in_file("item:x::main", "x::main", "src/bin/b.rs");
        assert_eq!(classify_reingest(&a, &b), ReingestClass::Contention);
    }

    #[test]
    fn missing_file_falls_back_to_prop_equality_silent() {
        let a = item_no_file("item:x::T", "x::T");
        assert_eq!(classify_reingest(&a, &a.clone()), ReingestClass::Silent);
    }

    #[test]
    fn missing_file_falls_back_to_prop_inequality_contention() {
        let a = item_no_file("item:x::T", "x::T");
        let b = item_no_file("item:x::T", "x::T").with_prop("kind", "struct");
        assert_eq!(classify_reingest(&a, &b), ReingestClass::Contention);
    }

    #[test]
    fn one_sided_file_with_differing_props_is_contention() {
        let a = item_no_file("item:x::main", "x::main");
        let b = item_in_file("item:x::main", "x::main", "src/bin/b.rs");
        assert_eq!(classify_reingest(&a, &b), ReingestClass::Contention);
    }

    #[test]
    fn one_sided_file_reverse_direction_is_contention() {
        // The fallback arm is direction-agnostic — pin the reverse
        // orientation too so a future asymmetric rewrite cannot pass on
        // the single-direction case alone.
        let a = item_in_file("item:x::main", "x::main", "src/bin/a.rs");
        let b = item_no_file("item:x::main", "x::main");
        assert_eq!(classify_reingest(&a, &b), ReingestClass::Contention);
    }

    // --- warning content: pinned by VARIANT, message names id + both files ---

    #[test]
    fn contention_warning_is_identity_contention_variant_naming_both_files() {
        let a = item_in_file("item:x::main", "x::main", "src/bin/a.rs");
        let b = item_in_file("item:x::main", "x::main", "src/bin/b.rs");
        let w = detect_contention(&a, &b).expect("differing files are a contention");
        assert_eq!(w.kind, WarningKind::IdentityContention);
        assert!(
            w.message.contains("identity contention")
                && w.message.contains("item:x::main")
                && w.message.contains("src/bin/a.rs")
                && w.message.contains("src/bin/b.rs"),
            "message must name the id and both files, got: {}",
            w.message
        );
    }

    #[test]
    fn non_string_file_prop_renders_distinctly_from_absence() {
        // An enrich-written Null file prop must not read as "(no file
        // prop)" — the message has to explain the decision that was made.
        let a = item_no_file("item:x::T", "x::T").with_prop("file", PropValue::Null);
        let b = item_in_file("item:x::T", "x::T", "src/a.rs");
        let w = detect_contention(&a, &b).expect("Null vs Str file props differ");
        assert!(
            !w.message.contains("(no file prop)"),
            "Null must render as a value, not as absence: {}",
            w.message
        );
    }

    #[test]
    fn classifier_keys_on_the_schema_declared_file_attribute() {
        // The classifier reaches the `file` prop by literal; pin that the
        // describe registry actually declares that attribute on :Item so a
        // schema-side rename cannot silently degrade the rule to the equality
        // fallback.
        let describe = cfdb_core::schema_describe();
        let item = describe
            .nodes
            .iter()
            .find(|n| n.label.as_str() == Label::ITEM)
            .expect(":Item is described");
        assert!(
            item.attributes.iter().any(|a| a.name == "file"),
            ":Item must declare the `file` attribute the classifier keys on"
        );
    }

    // --- ingest wiring: the store records the contention, capped ---

    #[test]
    fn ingest_of_contending_nodes_records_identity_contention_warning() {
        let ks = Keyspace::new("t");
        let mut store = PetgraphStore::new();
        store
            .ingest_nodes(
                &ks,
                vec![
                    item_in_file("item:x::main", "x::main", "src/bin/a.rs"),
                    item_in_file("item:x::main", "x::main", "src/bin/b.rs"),
                ],
            )
            .expect("ingest");
        let warnings = store.ingest_warnings(&ks);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::IdentityContention),
            "expected an IdentityContention warning, got: {warnings:?}"
        );
    }

    #[test]
    fn ingest_of_identical_reemit_stays_silent() {
        let ks = Keyspace::new("t");
        let mut store = PetgraphStore::new();
        let n = item_in_file("item:x::main", "x::main", "src/bin/a.rs");
        store.ingest_nodes(&ks, vec![n.clone(), n]).expect("ingest");
        assert!(
            store.ingest_warnings(&ks).is_empty(),
            "identical re-emit must not warn"
        );
    }

    #[test]
    fn suggestion_rides_only_the_first_recorded_contention() {
        let ks = Keyspace::new("t");
        let mut store = PetgraphStore::new();
        let mut nodes = Vec::new();
        for i in 0..3 {
            nodes.push(item_in_file(
                "item:x::main",
                "x::main",
                &format!("src/bin/{i}.rs"),
            ));
        }
        store.ingest_nodes(&ks, nodes).expect("ingest");
        let warnings = store.ingest_warnings(&ks);
        let with_suggestion = warnings.iter().filter(|w| w.suggestion.is_some()).count();
        assert_eq!(with_suggestion, 1, "exactly the first warning carries it");
        assert_eq!(
            warnings[0].suggestion.as_deref(),
            Some(CONTENTION_SUGGESTION)
        );
    }

    #[test]
    fn contentions_past_the_cap_collapse_into_one_summary_row() {
        let ks = Keyspace::new("t");
        let mut store = PetgraphStore::new();
        let over = 7usize;
        let mut nodes = Vec::new();
        for i in 0..(CONTENTION_WARNING_CAP + over + 1) {
            nodes.push(item_in_file("item:x::main", "x::main", &format!("f{i}.rs")));
        }
        // N nodes produce N-1 same-id re-ingests, all contending.
        store.ingest_nodes(&ks, nodes).expect("ingest");
        let warnings = store.ingest_warnings(&ks);
        assert_eq!(
            warnings.len(),
            CONTENTION_WARNING_CAP + 1,
            "cap + one summary row"
        );
        let summary = warnings.last().expect("summary row present");
        assert_eq!(summary.kind, WarningKind::IdentityContention);
        assert!(
            summary.message.contains(&over.to_string()) && summary.message.contains("suppressed"),
            "summary names the suppressed count, got: {}",
            summary.message
        );
    }

    // --- persistence: contention warnings survive save/load so a later
    // `cfdb query` process sees them; the transient per-edge ingest log does
    // NOT ride along ---

    #[test]
    fn contention_warnings_survive_persist_round_trip() {
        let ks = Keyspace::new("t");
        let mut store = PetgraphStore::new();
        store
            .ingest_nodes(
                &ks,
                vec![
                    item_in_file("item:x::main", "x::main", "src/bin/a.rs"),
                    item_in_file("item:x::main", "x::main", "src/bin/b.rs"),
                ],
            )
            .expect("ingest");

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.json");
        crate::persist::save(&store, &ks, &path).expect("save");

        let mut reloaded = PetgraphStore::new();
        crate::persist::load(&mut reloaded, &ks, &path).expect("load");
        let warnings = reloaded.ingest_warnings(&ks);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::IdentityContention),
            "persisted contention warning lost on round-trip: {warnings:?}"
        );
    }

    #[test]
    fn edge_drop_warnings_stay_transient_across_persist() {
        // Durability is a property of the record, not of the shared buffer.
        // The per-edge unknown-endpoint log is process-local; only contention
        // diagnostics are contracted to survive into the keyspace file.
        use cfdb_core::fact::Edge;
        use cfdb_core::schema::EdgeLabel;
        let ks = Keyspace::new("t");
        let mut store = PetgraphStore::new();
        store
            .ingest_nodes(&ks, vec![item_in_file("item:a", "a", "src/a.rs")])
            .expect("ingest");
        store
            .ingest_edges(
                &ks,
                vec![Edge::new(
                    "item:a",
                    "item:missing",
                    EdgeLabel::new(EdgeLabel::CALLS),
                )],
            )
            .expect("edge ingest");
        assert!(
            !store.ingest_warnings(&ks).is_empty(),
            "edge drop warns in-process"
        );

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.json");
        crate::persist::save(&store, &ks, &path).expect("save");
        let mut reloaded = PetgraphStore::new();
        crate::persist::load(&mut reloaded, &ks, &path).expect("load");
        assert!(
            reloaded
                .ingest_warnings(&ks)
                .iter()
                .all(|w| w.kind == WarningKind::IdentityContention),
            "only contention diagnostics persist"
        );
    }

    #[test]
    fn pre_rfc054_keyspace_file_without_warnings_field_loads() {
        // Legacy compat pin: a file missing `contention_warnings` must load
        // with zero warnings, not error.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("legacy.json");
        let legacy = serde_json::json!({
            "schema_version": cfdb_core::schema::SchemaVersion::CURRENT,
            "nodes": [],
            "edges": []
        });
        std::fs::write(&path, serde_json::to_vec(&legacy).expect("json")).expect("write");
        let ks = Keyspace::new("legacy");
        let mut store = PetgraphStore::new();
        crate::persist::load(&mut store, &ks, &path).expect("legacy file must load");
        assert!(store.ingest_warnings(&ks).is_empty());
    }
}
