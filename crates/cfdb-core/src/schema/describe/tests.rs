use sha2::{Digest, Sha256};

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
    // in slice 3/5, issue #325); `Literal` appended per RFC-041 slice
    // 041-A (issue #369 reservation; first emissions land in slice 041-B,
    // issue #370).
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
        "HAS_ARG",
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

/// RFC-041 §3.1 (ddd lens, council 2026-05-15) — `:Literal` carries exactly
/// `value`/`file`/`line`/`col`/`crate`/`is_test`, all `Extractor`-provenanced
/// (the producer lands in slice 041-B, issue #370; the descriptor is reserved
/// here with the `:ConstTable` precedent of an Extractor-tagged reservation).
/// It MUST NOT carry a `kind` attr: that would be a three-way homonym against
/// `:Item.kind` (declaration kind) and `:ConstTable.element_type`. Future
/// non-string literals use `lit_syntax`, not `kind`.
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

// ---- RFC-044 §3.1 sub-band 3: descriptor narrative freeze --------------

/// RFC-044 §3.1 sub-band 3(i) — frozen sha256 digest of every attribute
/// narrative string in `schema_describe()`.
///
/// The canonical snapshot is built by sorting all (node + edge) attribute
/// descriptors by `(kind, label, attr_name)` and joining them as:
///   `"<NODE|EDGE> <Label>.<attr> = <description>"`
/// lines separated by `\n`. The sha256 hex of the resulting UTF-8 bytes is
/// frozen in `FROZEN_NARRATIVE_DIGEST`.
///
/// **On mismatch:** the test prints the full current snapshot so you can
/// diff it against the previous one. To update a narrative legitimately
/// (e.g., an RFC changes the description of an attribute), recompute the
/// digest by temporarily setting `FROZEN_NARRATIVE_DIGEST = "RECOMPUTE"`,
/// running `cargo test -p cfdb-core schema_describe_narrative_digest 2>&1`,
/// and copying the `actual digest:` hex from the failure output into the
/// const. The update MUST be in the same PR as the narrative change —
/// explicit review surface for RFC §4 I6-shaped concerns.
#[test]
fn schema_describe_narrative_digest() {
    /// The frozen sha256 (lowercase hex) of the canonical narrative snapshot.
    /// See test body for the snapshot construction algorithm.
    ///
    /// To update after a legitimate narrative change: set this to `"RECOMPUTE"`,
    /// run the test, copy the `actual digest:` value from the failure output.
    const FROZEN_NARRATIVE_DIGEST: &str =
        "ab22b9ed42d291fc954ab8972ffe82e0afcf39a9ee8b62c8a34e31aea653f5ea";

    let d = schema_describe();

    // Build a canonically sorted list of lines.
    let mut lines: Vec<String> = Vec::new();

    // Node attributes
    for node in &d.nodes {
        let label = node.label.as_str();
        for attr in &node.attributes {
            lines.push(format!(
                "NODE {}.{} = {}",
                label, attr.name, attr.description
            ));
        }
    }

    // Edge attributes
    for edge in &d.edges {
        let label = edge.label.as_str();
        for attr in &edge.attributes {
            lines.push(format!(
                "EDGE {}.{} = {}",
                label, attr.name, attr.description
            ));
        }
    }

    // Deterministic sort by the line itself (already prefixed with kind+label+attr).
    lines.sort();

    let snapshot = lines.join("\n");

    // Compute sha256 hex.
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

/// RFC-044 §3.1 sub-band 3(ii) — explicit narrative pins for load-bearing
/// `:CallSite` attribute descriptions (RFC-043 §4 I6-shaped concerns).
///
/// These two pins lock the specific narrative sentences that RFC-043 §4 cites:
///   - `callee_resolved`: the epistemic precision caveat about proc-macro support.
///   - `resolver`: the closed-set enum-as-string description.
///
/// If either description is gutted or changed, this test fails with a clear
/// message identifying which pin broke and what the new text is.
#[test]
fn schema_describe_call_site_narrative_pins() {
    let d = schema_describe();
    let call_site = d
        .nodes
        .iter()
        .find(|n| n.label.as_str() == Label::CALL_SITE)
        .expect(":CallSite node descriptor must be present in schema_describe()");

    // Pin 1 — callee_resolved: RFC-043 epistemic precision caveat.
    // The key sentence: the RFC-043 caveat about proc-macro server enabling
    // improved resolution, and the statement that there is no per-keyspace
    // status flag. A deliberate truncation of the description removes this
    // caveat and this test fails.
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

    // Pin 2 — resolver: closed-set enum description.
    // The description must name both valid values ("syn", "hir") and the
    // SchemaVersion constraint. Removing either value string makes the enum
    // undiscoverable to consumers reading the schema at runtime.
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
}

/// RFC-044 §3.1 sub-band 1 (spec-coverage test) — asserts the `## Label` and
/// `## EdgeLabel` sections of `specs/concepts/cfdb-core.md` document EVERY node
/// and edge label returned by `schema_describe()` (by name and by attribute
/// field name), and that each section declares the `describe/{nodes,edges}.rs`
/// descriptor authoritative.
///
/// **Deviation from RFC-044 §3.1 (documented):** the RFC prescribed per-variant
/// `### :Crate`-style headings. `graph-specs check` validates EVERY markdown
/// heading bidirectionally against a `pub` type, so per-label headings would
/// require minting a marker type per label — the exact "second vocabulary
/// source" the ddd lens forbade in the same §3.1. The load-bearing ratified
/// constraint (no second vocabulary source) wins: the per-label vocabulary is
/// documented as a flat list inside the existing `## Label` / `## EdgeLabel`
/// concept sections (no new headings, no new types). This test enforces the
/// list stays complete — adding a label or attribute to the descriptor without
/// documenting it here fails the test (the "completeness constant" of §3.1).
///
/// **Non-vacuity guard:** asserts the spec file was read, both sections are
/// non-empty, and the descriptor returns the full schema.
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

    // Non-vacuity: both concept sections must be present and non-trivial.
    assert!(
        node_section.len() > 200 && edge_section.len() > 200,
        "## Label / ## EdgeLabel sections of cfdb-core.md are missing or too short \
         (node={} bytes, edge={} bytes) — spec layout changed unexpectedly",
        node_section.len(),
        edge_section.len(),
    );

    // ddd-specialist R1: each section declares the descriptor authoritative.
    assert!(
        node_section.contains("`crates/cfdb-core/src/schema/describe/nodes.rs` is authoritative"),
        "## Label section must declare describe/nodes.rs authoritative (ddd R1 / RFC-044 §3.1)"
    );
    assert!(
        edge_section.contains("`crates/cfdb-core/src/schema/describe/edges.rs` is authoritative"),
        "## EdgeLabel section must declare describe/edges.rs authoritative (ddd R1 / RFC-044 §3.1)"
    );

    let d = schema_describe();

    // Every node label + every attribute name is documented in ## Label.
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

    // Every edge label + every edge attribute name is documented in ## EdgeLabel.
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

    // Non-vacuity: the descriptor returned the full schema, not a degraded subset.
    assert!(
        d.nodes.len() >= 15 && d.edges.len() >= 20,
        "schema_describe() returned a degraded schema (nodes={}, edges={})",
        d.nodes.len(),
        d.edges.len(),
    );
}

/// Extract the content of a `## heading` section from a markdown string.
/// Returns everything from (but not including) the heading line up to the
/// next `##`-level heading (or end of file).
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
            // Stop at the next ## heading (any section).
            if line.trim().starts_with("## ") {
                break;
            }
            content.push_str(line);
            content.push('\n');
        }
    }
    content
}
