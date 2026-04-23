//! Node-label descriptors for `schema_describe()`.

use super::super::descriptors::{attr, AttributeDescriptor, NodeLabelDescriptor, Provenance};
use super::super::labels::Label;

pub(super) fn crate_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::CRATE),
        description: "A Cargo package in the workspace.".into(),
        attributes: vec![
            attr("name", "string", "Cargo package name.", Extractor),
            attr(
                "path",
                "string",
                "Manifest directory relative to workspace root.",
                Extractor,
            ),
            attr("version", "string", "SemVer from Cargo.toml.", Extractor),
        ],
    }
}

pub(super) fn module_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::MODULE),
        description: "A Rust module — either a `mod` block or a file-level module.".into(),
        attributes: vec![
            attr("crate", "string", "Containing crate name.", Extractor),
            attr(
                "file",
                "string",
                "Source file declaring the module.",
                Extractor,
            ),
            attr(
                "is_inline",
                "bool",
                "True when declared as `mod foo { ... }` inside another file.",
                Extractor,
            ),
            attr(
                "qpath",
                "string",
                "Fully-qualified module path (e.g. `foo::bar`).",
                Extractor,
            ),
        ],
    }
}

pub(super) fn file_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::FILE),
        description: "A `.rs` source file on disk.".into(),
        attributes: vec![
            attr("crate", "string", "Containing crate name.", Extractor),
            attr(
                "loc",
                "int",
                "Line-of-code count (non-blank, non-comment).",
                Extractor,
            ),
            attr(
                "module_qpath",
                "string",
                "Fully-qualified path of the module defined by this file.",
                Extractor,
            ),
            attr(
                "path",
                "string",
                "Source path relative to workspace root.",
                Extractor,
            ),
        ],
    }
}

pub(super) fn item_node_descriptor() -> NodeLabelDescriptor {
    let mut attributes = item_attrs_extractor();
    attributes.extend(item_attrs_enrich_metrics());
    attributes.extend(item_attrs_enrich_git_history());
    attributes.extend(item_attrs_enrich_reachability());
    attributes.sort_by(|a, b| a.name.cmp(&b.name));
    NodeLabelDescriptor {
        label: Label::new(Label::ITEM),
        description: "A top-level `pub`/`pub(crate)` item — struct, enum, trait, impl, fn, const, static, or type alias.".into(),
        attributes,
    }
}

/// Extractor-provenance attributes on `:Item` — syntactic facts the
/// AST walker populates directly.
pub(super) fn item_attrs_extractor() -> Vec<AttributeDescriptor> {
    use Provenance::Extractor;
    vec![
        attr("bounded_context", "string", "Bounded context the containing crate belongs to — derived at extraction time from the crate-prefix heuristic with optional `.cfdb/concepts/<name>.toml` overrides (council-cfdb-wiring §B.1.2).", Extractor),
        attr("cfg_gate", "string?", "Feature-only `#[cfg(...)]` expression captured on the item: `feature = \"x\"`, `all(...)`, `any(...)`, `not(...)`. Absent when the item has no `cfg(feature = ...)` or carries a non-feature cfg predicate. SchemaVersion v0.1.2+ only.", Extractor),
        attr("crate", "string", "Containing crate name.", Extractor),
        attr("deprecation_since", "string?", "Version string from `#[deprecated(since = \"X.Y.Z\")]`; `None` when the attribute is bare or absent. Extractor-time per RFC addendum §A2.2 row 3 (`#[deprecated]` is a syntactic fact and the AST walker already visits attributes). Populated by slice 43-C (issue #106). Reserved in slice 43-A; descriptor lands before any data writes.", Extractor),
        attr("doc_text", "string?", "Raw rustdoc comment text attached to the item.", Extractor),
        attr("file", "string", "Source file path relative to workspace root.", Extractor),
        attr("impl_target", "string?", "Normalised target type of an impl block (e.g. `Vec` for `impl<T> Foo for Vec<T>`). Only emitted on `:Item { kind: \"impl_block\" }` nodes — absent on all other item kinds. SchemaVersion V0_2_2+ (#42).", Extractor),
        attr("impl_trait", "string?", "Rendered trait path for a trait-impl block (e.g. `Display`, `cfdb_core::StoreBackend`). Only emitted on `:Item { kind: \"impl_block\" }` nodes AND only when the impl has a trait (inherent `impl Foo {}` emits no `impl_trait` prop). The `IMPLEMENTS` edge encodes the same information structurally when the trait :Item is resolvable within the walked workspace; cross-crate re-exports that syn cannot follow emit the prop but drop the edge (HIR-based resolution is a follow-up slice). SchemaVersion V0_2_2+ (#42).", Extractor),
        attr("is_deprecated", "bool", "True when the item carries a `#[deprecated]` attribute (any form — bare, `note =`, or `since =`). Extractor-time per RFC addendum §A2.2 row 3. Populated by slice 43-C (issue #106); reserved in slice 43-A.", Extractor),
        attr("is_test", "bool", "True when the item is under a `#[cfg(test)]` module or directly annotated `#[test]` (fn items only) (council-cfdb-wiring §B.1.1).", Extractor),
        attr("kind", "enum", "Item kind: `Struct`, `Enum`, `Trait`, `Impl`, `Fn`, `Const`, `TypeAlias`.", Extractor),
        attr("line", "int", "1-based line number of the item's first token.", Extractor),
        attr("module_qpath", "string", "Fully-qualified path of the enclosing module.", Extractor),
        attr("name", "string", "Unqualified item name.", Extractor),
        attr("qname", "string", "Fully-qualified name (`crate::module::Item`).", Extractor),
        attr("signature", "string?", "Canonical fn / method signature string of shape `[const ][async ][unsafe ]fn(<param-types>) -> <return-type>` — parameter NAMES omitted, only types contribute. Emitted on fn / method kinds only (absent on struct / enum / trait / const / impl_block / type_alias / static). Produced by `cfdb-extractor::type_render::render_fn_signature`. Load-bearing input for the `signature_divergent(a, b)` UDF (issue #47, RFC-029 §A1.5 gate v0.2-8) that discriminates Shared Kernel (same signature across bounded contexts) from Context Homonym (divergent signatures). Additive and non-breaking — V0_2_3 readers loading a keyspace that emits the prop ignore the extra attribute.", Extractor),
        attr("signature_hash", "string", "Stable hash of the item's normalized signature.", Extractor),
        attr("visibility", "enum", "Rust visibility: `pub`, `pub(crate)`, `pub(super)`, `private`, or `pub(in <path>)`. SchemaVersion v0.1.1+ only — legacy V0_1_0 graphs do not carry this attribute.", Extractor),
    ]
}

/// `enrich_metrics`-provenance attributes on `:Item` — deferred pass per
/// RFC addendum §A2.2; descriptors remain reserved.
pub(super) fn item_attrs_enrich_metrics() -> Vec<AttributeDescriptor> {
    use Provenance::EnrichMetrics;
    vec![
        attr("cyclomatic", "int", "Cyclomatic complexity (fn items only).", EnrichMetrics),
        attr("dup_cluster_id", "string?", "Structural-duplicate cluster id (only set when enrich_metrics has clustered this item).", EnrichMetrics),
        attr("test_coverage", "float", "Covered-line ratio in [0.0, 1.0] (fn items only).", EnrichMetrics),
        attr("unwrap_count", "int", "Count of panic-on-None / panic-on-Err method calls (unwrap / expect) inside the item body.", EnrichMetrics),
    ]
}

/// `enrich_git_history`-provenance attributes on `:Item` — populated by
/// slice 43-B (issue #105) behind the `git-enrich` feature flag.
pub(super) fn item_attrs_enrich_git_history() -> Vec<AttributeDescriptor> {
    use Provenance::EnrichGitHistory;
    vec![
        attr("git_commit_count", "int?", "Number of git commits touching the defining file. Written by `enrich_git_history()` (RFC addendum §A2.2 row 1). Populated by slice 43-B (issue #105) behind the `git-enrich` feature flag; reserved in slice 43-A.", EnrichGitHistory),
        attr("git_last_author", "string?", "Committer email of the most recent commit touching the defining file. Written by `enrich_git_history()`. Populated by slice 43-B.", EnrichGitHistory),
        attr("git_last_commit_unix_ts", "int?", "Unix epoch seconds (i64) of the most recent commit touching the defining file. Stored as an absolute timestamp rather than a calendar-relative age — clean-arch B2: `git_age_days` computed at enrichment time would violate G1 byte-stability across calendar days. The Stage-2 classifier Cypher computes `age_delta` from this timestamp at query time.", EnrichGitHistory),
    ]
}

/// `enrich_reachability`-provenance attributes on `:Item` — populated by
/// slice 43-G (issue #110).
pub(super) fn item_attrs_enrich_reachability() -> Vec<AttributeDescriptor> {
    use Provenance::EnrichReachability;
    vec![
        attr("reachable_entry_count", "int?", "Number of distinct `:EntryPoint` nodes reaching this item via `CALLS*` edges. Written by `enrich_reachability()` (RFC addendum §A2.2 row 5). `0` for items not reached from any entry point. Populated by slice 43-G (issue #110) — consumes `:EntryPoint` nodes from `cfdb-hir-extractor`. Reserved in slice 43-A.", EnrichReachability),
        attr("reachable_from_entry", "bool?", "True when at least one `:EntryPoint` reaches this item via `CALLS*`. Written by `enrich_reachability()`. When the keyspace has zero `:EntryPoint` nodes the pass returns `ran: false` rather than silently marking all items unreachable (clean-arch B3 degraded path). Populated by slice 43-G.", EnrichReachability),
    ]
}

pub(super) fn field_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::FIELD),
        description: "A struct field, tuple-struct element, or enum variant field.".into(),
        attributes: vec![
            attr(
                "index",
                "int",
                "Declaration index inside the parent (0-based).",
                Extractor,
            ),
            attr(
                "name",
                "string",
                "Field identifier (`_0`, `_1`, ... for tuple structs and tuple variants).",
                Extractor,
            ),
            attr(
                "parent_qname",
                "string",
                "Qualified name of the owning struct or enum variant.",
                Extractor,
            ),
            attr(
                "type_normalized",
                "string",
                "Type after RFC §6.4 normalization rules.",
                Extractor,
            ),
            attr(
                "type_path",
                "string",
                "Raw type path as written in source.",
                Extractor,
            ),
        ],
    }
}

pub(super) fn variant_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::VARIANT),
        description: "An enum variant.".into(),
        attributes: vec![
            attr(
                "index",
                "int",
                "Declaration index inside the parent enum (0-based).",
                Extractor,
            ),
            attr("name", "string", "Variant identifier.", Extractor),
            attr(
                "parent_qname",
                "string",
                "Qualified name of the enum that owns this variant.",
                Extractor,
            ),
            attr(
                "payload_kind",
                "enum",
                "Payload shape: `unit`, `tuple`, `struct`.",
                Extractor,
            ),
        ],
    }
}

pub(super) fn param_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::PARAM),
        description: "A function or method parameter.".into(),
        attributes: vec![
            attr("index", "int", "Parameter position (0-based).", Extractor),
            attr(
                "is_self",
                "bool",
                "True when this parameter is `self` / `&self` / `&mut self`.",
                Extractor,
            ),
            attr(
                "name",
                "string",
                "Parameter identifier; empty for wildcard patterns.",
                Extractor,
            ),
            attr(
                "parent_qname",
                "string",
                "Qualified name of the enclosing fn.",
                Extractor,
            ),
            attr(
                "type_normalized",
                "string",
                "Type after RFC §6.4 normalization.",
                Extractor,
            ),
            attr(
                "type_path",
                "string",
                "Raw type path as written in source.",
                Extractor,
            ),
        ],
    }
}

pub(super) fn call_site_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::CALL_SITE),
        description: "One concrete call expression in the source tree (caller → callee, file:line).".into(),
        attributes: vec![
            attr("arg_count", "int", "Number of arguments at the call site.", Extractor),
            attr("callee_path", "string", "Best-effort path of the callee (may be unresolved).", Extractor),
            attr("callee_resolved", "bool", "`true` when method dispatch / re-export / trait impl was resolved via HIR; `false` for textual-only syn-based extraction. SchemaVersion v0.1.3+ only. See Label::CALL_SITE discriminator contract.", Extractor),
            attr("caller_qname", "string", "Qualified name of the fn that contains this call.", Extractor),
            attr("file", "string", "Source file relative to workspace root.", Extractor),
            attr("is_test", "bool", "True when the enclosing item is under `#[cfg(test)]` or in `tests/`.", Extractor),
            attr("kind", "enum", "CallSite shape: `call` (ExprCall/MethodCall), `fn_ptr` (path passed as fn-pointer arg), `serde_default` (`#[serde(default = \"...\")]`).", Extractor),
            attr("line", "int", "1-based line number.", Extractor),
            attr("resolver", "enum", "Which extractor produced this node: `syn` (cfdb-extractor, unresolved name-based) or `hir` (cfdb-hir-extractor, HIR-resolved). SchemaVersion v0.1.3+ only. See Label::CALL_SITE discriminator contract.", Extractor),
        ],
    }
}

pub(super) fn entry_point_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::ENTRY_POINT),
        description: "A top-level entry into the system — MCP tool, CLI command, HTTP route, or cron registration. First populated in SchemaVersion v0.2.0 (Issue #86) by `cfdb-hir-extractor::extract_entry_points`. v0.1.x graphs have no :EntryPoint nodes.".into(),
        attributes: vec![
            attr("file", "string", "Source file path where the entry-point declaration lives (relative to workspace root, or absolute).", Extractor),
            attr("handler_qname", "string", "Qualified name of the handler item (fn / struct / enum) this entry point dispatches to.", Extractor),
            attr("kind", "enum", "Entry-point kind: `mcp_tool`, `cli_command`, `http_route`, or `cron_job`. v0.2.0 MVP detects `cli_command` (clap `#[derive(Parser/Subcommand)]`) and `mcp_tool` (`#[tool]`) via attribute heuristics; HTTP + cron kinds reserved for follow-up.", Extractor),
            attr("name", "string", "Public-facing name (tool name, CLI subcommand, route path, cron job id).", Extractor),
            attr("params", "json", "Registered parameters as a JSON array of `{name, type}` objects. v0.2.0 MVP emits `[]`; clap arg + MCP tool input-schema enrichment deferred to follow-up.", Extractor),
        ],
    }
}

pub(super) fn concept_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::EnrichConcepts;
    NodeLabelDescriptor {
        label: Label::new(Label::CONCEPT),
        description:
            "An overlay label assigned to items by concept rules (RFC §6.1 — Layer 2 enrichment)."
                .into(),
        attributes: vec![
            attr(
                "assigned_by",
                "enum",
                "Source of the assignment: `doc`, `rule`, `llm`, `manual`.",
                EnrichConcepts,
            ),
            attr(
                "name",
                "string",
                "Concept identifier (e.g. `CanonicalTimeframeResolver`).",
                EnrichConcepts,
            ),
        ],
    }
}

pub(super) fn context_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::CONTEXT),
        description: "A bounded context owning one or more crates (council-cfdb-wiring §B.1.3).".into(),
        attributes: vec![
            attr("canonical_crate", "string?", "Crate nominated as the authoritative owner of this context (if declared in `.cfdb/concepts/<name>.toml`; else empty).", Extractor),
            attr("name", "string", "Context identifier (e.g. `trading`, `strategy`, `cfdb`).", Extractor),
            attr("owning_rfc", "string?", "RFC identifier attached to this context (if declared in override TOML).", Extractor),
        ],
    }
}

pub(super) fn rfc_doc_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::EnrichRfcDocs;
    NodeLabelDescriptor {
        label: Label::new(Label::RFC_DOC),
        description: "An RFC document file (`docs/rfc/*.md`, `.concept-graph/*.md`, etc.) scanned by `enrich_rfc_docs()` for concept-name matches (RFC addendum §A2.2 row 2). Reserved in #43-A; first emissions land in slice 43-D (issue #107) with a SchemaVersion patch bump.".into(),
        attributes: vec![
            attr("path", "string", "Workspace-relative path of the RFC file.", EnrichRfcDocs),
            attr("title", "string?", "First `# ` heading of the file; `None` when the file has no level-1 heading.", EnrichRfcDocs),
        ],
    }
}
