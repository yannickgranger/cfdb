use sha2::{Digest, Sha256};

use super::super::descriptors::Provenance;
use super::super::labels::{EdgeLabel, Label};
use super::*;

#[test]
fn schema_describe_covers_all_node_labels() {
    let d = schema_describe();
    let labels: Vec<&str> = d.nodes.iter().map(|n| n.label.as_str()).collect();
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
            "Literal",
            "Argument",
            "MatchSite",
        ]
    );
}

#[test]
fn schema_describe_covers_all_edge_labels() {
    let d = schema_describe();
    let edges: Vec<&str> = d.edges.iter().map(|e| e.label.as_str()).collect();
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
        "HAS_ARG",
        "EXPOSES",
        "REGISTERS_PARAM",
        "LABELED_AS",
        "CANONICAL_FOR",
        "EQUIVALENT_TO",
        "REFERENCED_BY",
        "MATCHES_AT",
        "MATCHES_ON",
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
fn schema_describe_overlay_edges_are_provenanced_by_their_enrich_pass() {
    let d = schema_describe();
    for (label, expected) in [
        (EdgeLabel::LABELED_AS, Provenance::EnrichConcepts),
        (EdgeLabel::CANONICAL_FOR, Provenance::EnrichConcepts),
        (EdgeLabel::REFERENCED_BY, Provenance::EnrichRfcDocs),
    ] {
        let edge = d
            .edges
            .iter()
            .find(|e| e.label.as_str() == label)
            .unwrap_or_else(|| panic!("{label} edge descriptor missing"));
        assert_eq!(
            edge.provenance, expected,
            "{label} is emitted by an enrichment pass, not by extract; its provenance must say so",
        );
    }
}

#[test]
fn schema_describe_item_kind_documents_static_and_union() {
    let d = schema_describe();
    let item = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::ITEM)
        .expect("Item node descriptor");
    let kind = item
        .attributes
        .iter()
        .find(|a| a.name == "kind")
        .expect("Item.kind attribute descriptor");
    for wire in ["`static`", "`union`"] {
        assert!(
            kind.description.contains(wire),
            ":Item.kind descriptor must document the {wire} wire value; got: {}",
            kind.description
        );
    }
}

#[test]
fn schema_describe_item_kind_documents_lowercase_wire_values() {
    use crate::query::ItemKind;

    let d = schema_describe();
    let item = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::ITEM)
        .expect("Item node descriptor");
    let kind = item
        .attributes
        .iter()
        .find(|a| a.name == "kind")
        .expect("Item.kind attribute descriptor");
    let desc = &kind.description;

    for variant in ItemKind::variants() {
        let wire = variant.to_extractor_str();
        assert!(
            desc.contains(wire),
            "Item.kind descriptor must document wire value `{wire}` \
             (ItemKind::to_extractor_str); description was: {desc:?}",
        );
        let council = variant.as_str();
        assert!(
            !desc.contains(council),
            "Item.kind descriptor must not carry the capitalized council / CLI \
             spelling `{council}` — `:Item.kind` wire strings are lowercase; \
             description was: {desc:?}",
        );
    }

    assert!(
        desc.contains("static"),
        "Item.kind descriptor must document the `static` wire value \
         (visit_item_static); description was: {desc:?}",
    );
}

#[test]
fn schema_describe_item_description_documents_any_visibility() {
    let d = schema_describe();
    let item = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::ITEM)
        .expect("Item node descriptor");
    let desc = &item.description;

    assert!(
        !desc.contains("pub`/`pub(crate)`"),
        "Item description must not claim the node is restricted to \
         `pub`/`pub(crate)` visibility — private and pub(super)/pub(in ...) \
         items are emitted too; description was: {desc:?}",
    );

    let visibility_attr = item
        .attributes
        .iter()
        .find(|a| a.name == "visibility")
        .expect("Item.visibility attribute descriptor");
    for wire in ["pub", "pub(crate)", "pub(super)", "private", "pub(in"] {
        assert!(
            visibility_attr.description.contains(wire),
            "sanity: Item.visibility attribute must document `{wire}` \
             (test fixture assumption broke); description was: {:?}",
            visibility_attr.description,
        );
        assert!(
            desc.contains(wire),
            "Item description must document every visibility form the \
             `visibility` attribute enumerates, including `{wire}`; \
             description was: {desc:?}",
        );
    }
}

#[test]
fn schema_describe_literal_attrs_match_rfc_041() {
    let d = schema_describe();
    let lit = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::LITERAL)
        .expect("Literal node descriptor");
    let mut names: Vec<&str> = lit.attributes.iter().map(|a| a.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["col", "crate", "file", "is_test", "line", "value"],
        "RFC-041 §3.1 prescribes exactly these six attributes"
    );
    assert!(
        !lit.attributes.iter().any(|a| a.name == "kind"),
        "RFC-041 §3.1: :Literal MUST NOT carry a `kind` attr (three-way \
         homonym vs :Item.kind / :ConstTable.element_type)"
    );
    for a in &lit.attributes {
        assert_eq!(
            a.provenance,
            Provenance::Extractor,
            "Literal attr {} is an extractor-time syntactic fact",
            a.name,
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

#[test]
fn schema_describe_narrative_digest() {
    const FROZEN_NARRATIVE_DIGEST: &str =
        "5a459149b167b95891e265fcac69fcbae0c0c41762c4a7c527857d352568c653";

    let d = schema_describe();

    let mut lines: Vec<String> = Vec::new();

    for node in &d.nodes {
        let label = node.label.as_str();
        for attr in &node.attributes {
            lines.push(format!(
                "NODE {}.{} = {}",
                label, attr.name, attr.description
            ));
        }
    }

    for edge in &d.edges {
        let label = edge.label.as_str();
        for attr in &edge.attributes {
            lines.push(format!(
                "EDGE {}.{} = {}",
                label, attr.name, attr.description
            ));
        }
    }

    lines.sort();

    let snapshot = lines.join("\n");

    let mut hasher = Sha256::new();
    hasher.update(snapshot.as_bytes());
    let digest_bytes = hasher.finalize();
    let actual_digest = format!("{:x}", digest_bytes);

    if actual_digest != FROZEN_NARRATIVE_DIGEST {
        eprintln!(
            "\n=== NARRATIVE SNAPSHOT (current) ===\n{}\n=== END SNAPSHOT ===\n",
            snapshot
        );
        eprintln!("actual digest:  {}", actual_digest);
        eprintln!("frozen digest:  {}", FROZEN_NARRATIVE_DIGEST);
        eprintln!(
            "\nTo update: copy the 'actual digest:' value above into \
             FROZEN_NARRATIVE_DIGEST in schema/describe/tests.rs. \
             The update MUST be in the same PR as the narrative change."
        );
    }
    assert_eq!(
        actual_digest, FROZEN_NARRATIVE_DIGEST,
        "descriptor narrative digest mismatch — a description string in \
         schema/describe/nodes.rs or edges.rs was changed without updating \
         FROZEN_NARRATIVE_DIGEST. See stderr for the full snapshot and the \
         new digest value to copy."
    );
}

#[test]
fn schema_describe_call_site_narrative_pins() {
    let d = schema_describe();
    let call_site = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::CALL_SITE)
        .expect(":CallSite node descriptor must be present in schema_describe()");

    let callee_resolved = call_site
        .attributes
        .iter()
        .find(|a| a.name == "callee_resolved")
        .expect(":CallSite must have a callee_resolved attribute");
    assert!(
        callee_resolved.description.contains("RFC-043"),
        ":CallSite.callee_resolved description must contain 'RFC-043' \
         (epistemic precision caveat — RFC-044 §3.1 I6 narrative pin). \
         Current description: {:?}",
        callee_resolved.description,
    );
    assert!(
        callee_resolved
            .description
            .contains("no per-keyspace status flag"),
        ":CallSite.callee_resolved description must contain \
         'no per-keyspace status flag' (RFC-043 design decision caveat — \
         RFC-044 §3.1 I6 narrative pin). \
         Current description: {:?}",
        callee_resolved.description,
    );
    assert!(
        callee_resolved
            .description
            .contains("rust-analyzer-proc-macro-srv"),
        ":CallSite.callee_resolved description must contain \
         'rust-analyzer-proc-macro-srv' (RFC-043 sysroot dependency caveat — \
         RFC-044 §3.1 I6 narrative pin). \
         Current description: {:?}",
        callee_resolved.description,
    );

    let resolver = call_site
        .attributes
        .iter()
        .find(|a| a.name == "resolver")
        .expect(":CallSite must have a resolver attribute");
    assert!(
        resolver.description.contains("`syn`"),
        ":CallSite.resolver description must enumerate valid value `syn` \
         (cfdb-extractor, unresolved name-based) — RFC-044 §3.1 I6 narrative pin. \
         Current description: {:?}",
        resolver.description,
    );
    assert!(
        resolver.description.contains("`hir`"),
        ":CallSite.resolver description must enumerate valid value `hir` \
         (cfdb-hir-extractor, HIR-resolved) — RFC-044 §3.1 I6 narrative pin. \
         Current description: {:?}",
        resolver.description,
    );
    assert!(
        resolver.description.contains("v0.1.3"),
        ":CallSite.resolver description must reference SchemaVersion v0.1.3+ \
         constraint — RFC-044 §3.1 I6 narrative pin. \
         Current description: {:?}",
        resolver.description,
    );
    assert!(
        resolver.description.contains("tree-sitter-php"),
        ":CallSite.resolver description must enumerate valid value \
         `tree-sitter-php` (cfdb-extractor-php) — RFC-045 45-C enum-pin extension. \
         Current description: {:?}",
        resolver.description,
    );
}

#[test]
fn spec_sections_cover_all_schema_labels() {
    let spec_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../specs/concepts/cfdb-core.md");
    let spec_content = std::fs::read_to_string(&spec_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read specs/concepts/cfdb-core.md at {}: {}",
            spec_path.display(),
            e
        )
    });

    let node_section = extract_section_content(&spec_content, "Label");
    let edge_section = extract_section_content(&spec_content, "EdgeLabel");

    assert!(
        node_section.len() > 200 && edge_section.len() > 200,
        "## Label / ## EdgeLabel sections of cfdb-core.md are missing or too short \
         (node={} bytes, edge={} bytes) — spec layout changed unexpectedly",
        node_section.len(),
        edge_section.len(),
    );

    assert!(
        node_section.contains("`crates/cfdb-core/src/schema/describe/nodes.rs` is authoritative"),
        "## Label section must declare describe/nodes.rs authoritative (ddd R1 / RFC-044 §3.1)"
    );
    assert!(
        edge_section.contains("`crates/cfdb-core/src/schema/describe/edges.rs` is authoritative"),
        "## EdgeLabel section must declare describe/edges.rs authoritative (ddd R1 / RFC-044 §3.1)"
    );

    let d = schema_describe();

    for node in &d.nodes {
        let label = node.label.as_str();
        assert!(
            node_section.contains(label),
            "## Label section does not document node label `:{label}`. Add it to the \
             flat vocabulary list (RFC-044 §3.1 completeness constant)."
        );
        for a in &node.attributes {
            assert!(
                node_section.contains(a.name.as_str()),
                "## Label section omits `:{label}` attribute `{}`. Each label's \
                 NodeLabelDescriptor attribute field names must appear in the list.",
                a.name,
            );
        }
    }

    for edge in &d.edges {
        let label = edge.label.as_str();
        assert!(
            edge_section.contains(label),
            "## EdgeLabel section does not document edge label `[:{label}]`. Add it to \
             the flat vocabulary list (RFC-044 §3.1 completeness constant)."
        );
        for a in &edge.attributes {
            assert!(
                edge_section.contains(a.name.as_str()),
                "## EdgeLabel section omits `[:{label}]` attribute `{}`.",
                a.name,
            );
        }
    }

    assert!(
        d.nodes.len() >= 15 && d.edges.len() >= 20,
        "schema_describe() returned a degraded schema (nodes={}, edges={})",
        d.nodes.len(),
        d.edges.len(),
    );
}

fn extract_section_content(markdown: &str, section_name: &str) -> String {
    let heading_marker = format!("## {}", section_name);
    let mut in_section = false;
    let mut content = String::new();

    for line in markdown.lines() {
        if line.trim() == heading_marker {
            in_section = true;
            continue;
        }
        if in_section {
            if line.trim().starts_with("## ") {
                break;
            }
            content.push_str(line);
            content.push('\n');
        }
    }
    content
}
