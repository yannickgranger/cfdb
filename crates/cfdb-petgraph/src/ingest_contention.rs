use cfdb_core::fact::Node;
use cfdb_core::result::{Warning, WarningKind};

pub(crate) const CONTENTION_WARNING_CAP: usize = 50;

pub(crate) const CONTENTION_SUGGESTION: &str = "distinct source constructs are contending for one \
     node identity; the earlier one is absent from this graph (RFC-054)";

#[derive(Debug, PartialEq, Eq)]
enum ReingestClass {
    Silent,
    Contention,
}

fn classify_reingest(existing: &Node, incoming: &Node) -> ReingestClass {
    match (existing.props.get("file"), incoming.props.get("file")) {
        (Some(a), Some(b)) if a == b => ReingestClass::Silent,
        (Some(_), Some(_)) => ReingestClass::Contention,
        _ if existing == incoming => ReingestClass::Silent,
        _ => ReingestClass::Contention,
    }
}

pub(crate) fn detect_contention(existing: &Node, incoming: &Node) -> Option<Warning> {
    match classify_reingest(existing, incoming) {
        ReingestClass::Contention => Some(contention_warning(existing, incoming)),
        ReingestClass::Silent => None,
    }
}

fn file_of(n: &Node) -> String {
    match n.props.get("file") {
        Some(v) => v
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("{v:?}")),
        None => "(no file prop)".to_string(),
    }
}

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
        let a = item_in_file("item:x::main", "x::main", "src/bin/a.rs");
        let b = item_no_file("item:x::main", "x::main");
        assert_eq!(classify_reingest(&a, &b), ReingestClass::Contention);
    }

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
