//! The `schema_describe()` verb — runtime contract for the cfdb vocabulary.
//!
//! RFC §6A.1 / PLAN-v1 §6.1. Deterministic and byte-stable per build (G1).

use super::descriptors::{
    attr, EdgeLabelDescriptor, NodeLabelDescriptor, Provenance, SchemaDescribe,
};
use super::labels::{EdgeLabel, Label, SchemaVersion};

/// Return the canonical schema description for the current cfdb-core build.
///
/// This is the runtime contract cfdb exposes to consumers — the complete
/// vocabulary of node labels, edge labels, attributes, and per-attribute
/// provenance (RFC §7 fact schema, PLAN-v1 §6.1). Deterministic and
/// byte-stable for a given build.
pub fn schema_describe() -> SchemaDescribe {
    SchemaDescribe {
        schema_version: SchemaVersion::V0_1_0,
        nodes: node_descriptors(),
        edges: edge_descriptors(),
    }
}

fn node_descriptors() -> Vec<NodeLabelDescriptor> {
    use Provenance::{EnrichConcepts, EnrichMetrics, Extractor};
    vec![
        NodeLabelDescriptor {
            label: Label::new(Label::CRATE),
            description: "A Cargo package in the workspace.".into(),
            attributes: vec![
                attr("name", "string", "Cargo package name.", Extractor),
                attr("path", "string", "Manifest directory relative to workspace root.", Extractor),
                attr("version", "string", "SemVer from Cargo.toml.", Extractor),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::MODULE),
            description: "A Rust module — either a `mod` block or a file-level module.".into(),
            attributes: vec![
                attr("crate", "string", "Containing crate name.", Extractor),
                attr("file", "string", "Source file declaring the module.", Extractor),
                attr("is_inline", "bool", "True when declared as `mod foo { ... }` inside another file.", Extractor),
                attr("qpath", "string", "Fully-qualified module path (e.g. `foo::bar`).", Extractor),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::FILE),
            description: "A `.rs` source file on disk.".into(),
            attributes: vec![
                attr("crate", "string", "Containing crate name.", Extractor),
                attr("loc", "int", "Line-of-code count (non-blank, non-comment).", Extractor),
                attr("module_qpath", "string", "Fully-qualified path of the module defined by this file.", Extractor),
                attr("path", "string", "Source path relative to workspace root.", Extractor),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::ITEM),
            description: "A top-level `pub`/`pub(crate)` item — struct, enum, trait, impl, fn, const, static, or type alias.".into(),
            attributes: vec![
                attr("bounded_context", "string", "Bounded context the containing crate belongs to — derived at extraction time from the crate-prefix heuristic with optional `.cfdb/concepts/<name>.toml` overrides (council-cfdb-wiring §B.1.2).", Extractor),
                attr("crate", "string", "Containing crate name.", Extractor),
                attr("cyclomatic", "int", "Cyclomatic complexity (fn items only).", EnrichMetrics),
                attr("doc_text", "string?", "Raw rustdoc comment text attached to the item.", Extractor),
                attr("dup_cluster_id", "string?", "Structural-duplicate cluster id (only set when enrich_metrics has clustered this item).", EnrichMetrics),
                attr("file", "string", "Source file path relative to workspace root.", Extractor),
                attr("is_test", "bool", "True when the item is under a `#[cfg(test)]` module or directly annotated `#[test]` (fn items only) (council-cfdb-wiring §B.1.1).", Extractor),
                attr("kind", "enum", "Item kind: `Struct`, `Enum`, `Trait`, `Impl`, `Fn`, `Const`, `TypeAlias`.", Extractor),
                attr("line", "int", "1-based line number of the item's first token.", Extractor),
                attr("module_qpath", "string", "Fully-qualified path of the enclosing module.", Extractor),
                attr("name", "string", "Unqualified item name.", Extractor),
                attr("qname", "string", "Fully-qualified name (`crate::module::Item`).", Extractor),
                attr("signature_hash", "string", "Stable hash of the item's normalized signature.", Extractor),
                attr("test_coverage", "float", "Covered-line ratio in [0.0, 1.0] (fn items only).", EnrichMetrics),
                attr("unwrap_count", "int", "Number of `.unwrap()` / `.expect()` sites inside the item body.", EnrichMetrics),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::FIELD),
            description: "A struct field or tuple-struct element.".into(),
            attributes: vec![
                attr("index", "int", "Declaration index inside the parent (0-based).", Extractor),
                attr("name", "string", "Field identifier (`_0`, `_1`, ... for tuple structs).", Extractor),
                attr("parent_qname", "string", "Qualified name of the struct that owns this field.", Extractor),
                attr("type_normalized", "string", "Type after RFC §6.4 normalization rules.", Extractor),
                attr("type_path", "string", "Raw type path as written in source.", Extractor),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::VARIANT),
            description: "An enum variant.".into(),
            attributes: vec![
                attr("index", "int", "Declaration index inside the parent enum (0-based).", Extractor),
                attr("name", "string", "Variant identifier.", Extractor),
                attr("parent_qname", "string", "Qualified name of the enum that owns this variant.", Extractor),
                attr("payload_kind", "enum", "Payload shape: `unit`, `tuple`, `struct`.", Extractor),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::PARAM),
            description: "A function or method parameter.".into(),
            attributes: vec![
                attr("index", "int", "Parameter position (0-based).", Extractor),
                attr("is_self", "bool", "True when this parameter is `self` / `&self` / `&mut self`.", Extractor),
                attr("name", "string", "Parameter identifier; empty for wildcard patterns.", Extractor),
                attr("parent_qname", "string", "Qualified name of the enclosing fn.", Extractor),
                attr("type_normalized", "string", "Type after RFC §6.4 normalization.", Extractor),
                attr("type_path", "string", "Raw type path as written in source.", Extractor),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::CALL_SITE),
            description: "One concrete call expression in the source tree (caller → callee, file:line).".into(),
            attributes: vec![
                attr("arg_count", "int", "Number of arguments at the call site.", Extractor),
                attr("caller_qname", "string", "Qualified name of the fn that contains this call.", Extractor),
                attr("callee_path", "string", "Best-effort path of the callee (may be unresolved).", Extractor),
                attr("file", "string", "Source file relative to workspace root.", Extractor),
                attr("is_test", "bool", "True when the enclosing item is under `#[cfg(test)]` or in `tests/`.", Extractor),
                attr("kind", "enum", "CallSite shape: `call` (ExprCall/MethodCall), `fn_ptr` (path passed as fn-pointer arg), `serde_default` (`#[serde(default = \"...\")]`).", Extractor),
                attr("line", "int", "1-based line number.", Extractor),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::ENTRY_POINT),
            description: "A top-level entry into the system — MCP tool, CLI command, HTTP route, or cron registration.".into(),
            attributes: vec![
                attr("handler_qname", "string", "Qualified name of the handler fn this entry point dispatches to.", Extractor),
                attr("kind", "enum", "Entry-point kind: `mcp`, `cli`, `http`, `cron`.", Extractor),
                attr("name", "string", "Public-facing name (tool name, CLI subcommand, route path, cron job id).", Extractor),
                attr("params", "json", "Registered parameters as a JSON array of `{name, type}` objects.", Extractor),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::CONCEPT),
            description: "An overlay label assigned to items by concept rules (RFC §6.1 — Layer 2 enrichment).".into(),
            attributes: vec![
                attr("assigned_by", "enum", "Source of the assignment: `doc`, `rule`, `llm`, `manual`.", EnrichConcepts),
                attr("name", "string", "Concept identifier (e.g. `CanonicalTimeframeResolver`).", EnrichConcepts),
            ],
        },
        NodeLabelDescriptor {
            label: Label::new(Label::CONTEXT),
            description: "A bounded context owning one or more crates (council-cfdb-wiring §B.1.3).".into(),
            attributes: vec![
                attr("canonical_crate", "string?", "Crate nominated as the authoritative owner of this context (if declared in `.cfdb/concepts/<name>.toml`; else empty).", Extractor),
                attr("name", "string", "Context identifier (e.g. `trading`, `strategy`, `cfdb`).", Extractor),
                attr("owning_rfc", "string?", "RFC identifier attached to this context (if declared in override TOML).", Extractor),
            ],
        },
    ]
}

fn edge_descriptors() -> Vec<EdgeLabelDescriptor> {
    use Provenance::Extractor;
    vec![
        // ---- Structural ------------------------------------------------------
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            description: "Any node with a crate belongs to that Crate.".into(),
            attributes: vec![],
            from: vec![],
            to: vec![Label::new(Label::CRATE)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::IN_MODULE),
            description: "An Item or File is contained in a Module.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM), Label::new(Label::FILE)],
            to: vec![Label::new(Label::MODULE)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::HAS_FIELD),
            description: "A struct Item owns a Field.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::FIELD)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::HAS_VARIANT),
            description: "An enum Item owns a Variant.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::VARIANT)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::HAS_PARAM),
            description: "An fn Item owns a Param.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::PARAM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::TYPE_OF),
            description: "A Field, Param, or Variant payload references an Item used as its type."
                .into(),
            attributes: vec![],
            from: vec![
                Label::new(Label::FIELD),
                Label::new(Label::PARAM),
                Label::new(Label::VARIANT),
            ],
            to: vec![Label::new(Label::ITEM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::IMPLEMENTS),
            description: "An impl Item implements a trait Item.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::IMPLEMENTS_FOR),
            description: "An impl Item targets a type Item (the receiver of the impl).".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::RETURNS),
            description: "An fn Item returns a type Item.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::SUPERTRAIT),
            description: "A trait Item extends another trait Item as a supertrait bound.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::BELONGS_TO),
            description: "A Crate belongs to its bounded Context (council-cfdb-wiring §B.1.3)."
                .into(),
            attributes: vec![],
            from: vec![Label::new(Label::CRATE)],
            to: vec![Label::new(Label::CONTEXT)],
        },
        // ---- Call graph ------------------------------------------------------
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::CALLS),
            description: "Static call edge between two fn Items (best-effort cross-crate).".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
            description: "A CallSite invokes a concrete fn Item.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::CALL_SITE)],
            to: vec![Label::new(Label::ITEM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::RECEIVES_ARG),
            description: "A CallSite binds one of its arguments to a callee Param.".into(),
            attributes: vec![attr(
                "arg_index",
                "int",
                "Position of the argument at the call site (0-based).",
                Extractor,
            )],
            from: vec![Label::new(Label::CALL_SITE)],
            to: vec![Label::new(Label::PARAM)],
        },
        // ---- Entry points ----------------------------------------------------
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::EXPOSES),
            description: "An EntryPoint dispatches to a handler fn Item.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ENTRY_POINT)],
            to: vec![Label::new(Label::ITEM)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
            description: "An EntryPoint declares a registered parameter.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ENTRY_POINT)],
            to: vec![Label::new(Label::PARAM)],
        },
        // ---- Concept overlay -------------------------------------------------
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::LABELED_AS),
            description: "An Item carries a semantic Concept label.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::CONCEPT)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::CANONICAL_FOR),
            description: "An Item is the designated authoritative implementation of a Concept."
                .into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::CONCEPT)],
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::EQUIVALENT_TO),
            description: "Two Concepts are synonyms (e.g. `TradeSide ≡ Direction`).".into(),
            attributes: vec![],
            from: vec![Label::new(Label::CONCEPT)],
            to: vec![Label::new(Label::CONCEPT)],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::super::descriptors::Provenance;
    use super::*;

    #[test]
    fn schema_describe_covers_all_node_labels() {
        let d = schema_describe();
        let labels: Vec<&str> = d.nodes.iter().map(|n| n.label.as_str()).collect();
        // Order follows RFC §6.1 / PLAN-v1 §6.1 table order; `Context` appended
        // per council-cfdb-wiring §B.1.3 (v0.1 minor schema bump, #3727).
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
            ]
        );
    }

    #[test]
    fn schema_describe_covers_all_edge_labels() {
        let d = schema_describe();
        let edges: Vec<&str> = d.edges.iter().map(|e| e.label.as_str()).collect();
        // Every const on EdgeLabel must appear in schema_describe exactly once.
        let expected = [
            "IN_CRATE",
            "IN_MODULE",
            "HAS_FIELD",
            "HAS_VARIANT",
            "HAS_PARAM",
            "TYPE_OF",
            "IMPLEMENTS",
            "IMPLEMENTS_FOR",
            "RETURNS",
            "SUPERTRAIT",
            "BELONGS_TO",
            "CALLS",
            "INVOKES_AT",
            "RECEIVES_ARG",
            "EXPOSES",
            "REGISTERS_PARAM",
            "LABELED_AS",
            "CANONICAL_FOR",
            "EQUIVALENT_TO",
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
}
