//! RFC-054 §3.4 (54-A, #556) — classify a same-id re-ingest as a silent
//! legitimate update or an identity contention worth warning about.
//!
//! `ingest_one_node` replaces an existing node in place when an incoming
//! node carries an id already in the graph. That is documented additive-load
//! behavior — but when the two nodes are *different* (distinct cargo targets
//! colliding on one qname, #542), the replace silently drops a real node.
//! The ratified rule: compare on the `file` prop; when either side lacks
//! `file`, fall back to full prop inequality.

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::result::{Warning, WarningKind};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReingestClass {
    /// The same logical node re-ingested — stays silent (documented
    /// additive-load behavior, e.g. enrich passes re-saving a keyspace).
    Silent,
    /// A different node is claiming an existing identity; the replace
    /// would silently drop the earlier node (RFC-054 §3.4 / #542).
    Contention,
}

/// Pure classifier (values in → values out, zero I/O).
///
/// Ratified rule (RFC-054 §3.4): same `file` prop ⇒ the same logical node
/// being updated (silent, whatever the other props do — enrich passes
/// legitimately re-save with added attrs); both have `file` and it differs
/// ⇒ contention; either side lacks `file` ⇒ full node equality decides.
pub(crate) fn classify_reingest(existing: &Node, incoming: &Node) -> ReingestClass {
    match (existing.props.get("file"), incoming.props.get("file")) {
        (Some(a), Some(b)) if a == b => ReingestClass::Silent,
        (Some(_), Some(_)) => ReingestClass::Contention,
        _ => {
            if existing.label == incoming.label && existing.props == incoming.props {
                ReingestClass::Silent
            } else {
                ReingestClass::Contention
            }
        }
    }
}

/// Build the contention warning for a classified [`ReingestClass::Contention`].
/// The message names the id and both `file` props so the loss is traceable
/// without re-running the extract.
pub(crate) fn contention_warning(existing: &Node, incoming: &Node) -> Warning {
    fn file_of(n: &Node) -> &str {
        match n.props.get("file") {
            Some(PropValue::Str(s)) => s.as_str(),
            _ => "(no file prop)",
        }
    }
    Warning {
        kind: WarningKind::IdentityContention,
        message: format!(
            "identity contention on `{}`: `{}` replaced by `{}` — the earlier node was dropped",
            incoming.id,
            file_of(existing),
            file_of(incoming)
        ),
        suggestion: Some(
            "distinct cargo targets sharing one qname collide until RFC-054 54-B (#557) lands"
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    // --- classifier (pure fn, ratified rule RFC-054 §3.4) ---

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

    // --- warning content: pinned by VARIANT, message names id + both files ---

    #[test]
    fn contention_warning_is_identity_contention_variant_naming_both_files() {
        let a = item_in_file("item:x::main", "x::main", "src/bin/a.rs");
        let b = item_in_file("item:x::main", "x::main", "src/bin/b.rs");
        let w = contention_warning(&a, &b);
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

    // --- ingest wiring: the store records the contention ---

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

    // --- persistence: extract-time warnings survive save/load so a later
    // `cfdb query` process sees them (RFC-054 54-A test row) ---

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
    fn pre_rfc054_keyspace_file_without_warnings_field_loads() {
        // Legacy compat pin: a file missing `ingest_warnings` (pre-54-A)
        // must load with zero warnings, not error.
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
