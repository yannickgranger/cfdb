//! Structural node-label descriptors — the Rust AST skeleton: crate,
//! module, file, item (+ its provenance-grouped attribute sets), field,
//! variant, and param.

use crate::schema::descriptors::{attr, AttributeDescriptor, NodeLabelDescriptor, Provenance};
use crate::schema::labels::Label;

pub(in crate::schema::describe) fn crate_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::CRATE),
        description: "A Cargo package in the workspace.".into(),
        attributes: vec![
            attr(
                "crate_tier",
                "int",
                "Topological longest-path depth of this crate in the intra-workspace normal-`[dependencies]` DAG: a crate with no in-workspace normal dependencies is tier 0, otherwise `1 + max(crate_tier of its in-workspace normal deps)`. Computed at extract time from each package's declared `[dependencies]` (`kind == Normal`, workspace-filtered); dev/build deps are excluded. Deterministic, recall-gated, and inside the G1 canonical dump. SchemaVersion V0_6_0+ (RFC-050 50-A).",
                Extractor,
            ),
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

pub(in crate::schema::describe) fn module_node_descriptor() -> NodeLabelDescriptor {
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

pub(in crate::schema::describe) fn file_node_descriptor() -> NodeLabelDescriptor {
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

pub(in crate::schema::describe) fn item_node_descriptor() -> NodeLabelDescriptor {
    let mut attributes = item_attrs_extractor();
    attributes.extend(item_attrs_enrich_metrics());
    attributes.extend(item_attrs_enrich_git_history());
    attributes.extend(item_attrs_enrich_reachability());
    attributes.sort_by(|a, b| a.name.cmp(&b.name));
    NodeLabelDescriptor {
        label: Label::new(Label::ITEM),
        description: "A top-level item of any visibility (`pub`, `pub(crate)`, `pub(super)`, private, or `pub(in <path>)`, per the `visibility` attribute) — struct, enum, trait, impl, fn, const, static, or type alias.".into(),
        attributes,
    }
}

/// Extractor-provenance attributes on `:Item` — syntactic facts the
/// AST walker populates directly.
fn item_attrs_extractor() -> Vec<AttributeDescriptor> {
    // Composed from two builders purely to keep each short and readable; the
    // caller sorts by name, so the partition is cosmetic (#488 boy-scout — the
    // flat 20-attr builder tripped the line-based complexity heuristic on the
    // `?` / `for` tokens inside its doc strings, not on real branching).
    let mut attrs = item_attrs_extractor_metadata();
    attrs.extend(item_attrs_extractor_structural());
    attrs
}

/// Extractor-provenance `:Item` attributes (part 1) — context, cfg, rustdoc,
/// deprecation, impl-block, and compile-scope facts.
fn item_attrs_extractor_metadata() -> Vec<AttributeDescriptor> {
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
    ]
}

/// Extractor-provenance `:Item` attributes (part 2) — kind, naming, location,
/// signature, visibility, and cross-producer (PHP/TS) disambiguation facts.
fn item_attrs_extractor_structural() -> Vec<AttributeDescriptor> {
    use Provenance::Extractor;
    // #479/#481 — the top-level kind list is GENERATED from
    // `ItemKind::variants()` so the descriptor can never again drift from
    // the vocabulary the CLI parses (`method` is not an `ItemKind`: it is
    // the impl-member kind, appended textually below).
    let top_level_kinds = crate::query::ItemKind::variants()
        .iter()
        .map(|k| format!("`{}`", k.to_extractor_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let kind_description = format!(
        "Item kind, as the lowercase wire string emitted by the extractor. \
         Top-level items: {top_level_kinds}. Impl members additionally appear \
         with kind `method`."
    );
    vec![
        attr("kind", "enum", &kind_description, Extractor),
        attr("line", "int", "1-based line number of the item's first token.", Extractor),
        attr("module_qpath", "string", "Fully-qualified path of the enclosing module.", Extractor),
        attr("name", "string", "Unqualified item name.", Extractor),
        attr("php_construct", "string?", "Tree-sitter AST node kind for `:Item`s emitted by the PHP producer (`cfdb-extractor-php`, RFC-041 / RFC-045): `class_declaration`, `interface_declaration`, or `trait_declaration` (all three squash to `kind:\"trait\"`), `method_declaration`, or `function_definition`. The disambiguation seam for cypher queries that must distinguish a PHP class (an `IMPLEMENTS` source) from an interface (its target) — both carry `kind:\"trait\"`. Absent on `:Item`s from the Rust producer (and from the TS producer, which uses `ts_construct`). Emitted since the PHP MVP (#264) but documented from RFC-045 45-A onward.", Extractor),
        attr("ts_construct", "string?", "Tree-sitter AST node kind for `:Item`s emitted by the TypeScript producer (`cfdb-extractor-ts`, RFC-045 45-B): `class_declaration`, `abstract_class_declaration` (both → `kind:\"struct\"`), or `interface_declaration` (→ `kind:\"trait\"`). The disambiguation seam for cypher queries that must distinguish a TS class (an `IMPLEMENTS` source) from an interface (its target). Absent on `:Item`s from the Rust producer (and from the PHP producer, which uses `php_construct`). Emitted only on class/interface declarations.", Extractor),
        attr("qname", "string", "Fully-qualified name (`crate::module::Item`). Homonym note (RFC-054): the DISPLAY name, deliberately NOT unique across cargo targets — N bin targets each carrying `fn main` yield N distinct `:Item` nodes sharing one qname, disambiguated by the `target` attribute and by their target-scoped node ids. Queries filtering on qname must expect multiple rows in multi-target packages.", Extractor),
        attr("signature", "string?", "Canonical fn / method signature string of shape `[const ][async ][unsafe ]fn(<param-types>) -> <return-type>` — parameter NAMES omitted, only types contribute. Emitted on fn / method kinds only (absent on struct / enum / trait / const / impl_block / type_alias / static). Produced by `cfdb-extractor::type_render::render_fn_signature`. Load-bearing input for the `signature_divergent(a, b)` UDF (issue #47, RFC-029 §A1.5 gate v0.2-8) that discriminates Shared Kernel (same signature across bounded contexts) from Context Homonym (divergent signatures). Additive and non-breaking — V0_2_3 readers loading a keyspace that emits the prop ignore the extra attribute.", Extractor),
        attr("signature_hash", "string", "Stable hash of the item's normalized signature.", Extractor),
        attr("target", "string?", "Which cargo build target the item was walked from (RFC-054 §3.2): `lib` or `bin:<target-name>`. Cargo's own term — unrelated to the edge-endpoint sense of \"target\" and to `impl_target`. Absent ⇒ pre-RFC-054 extract OR a non-Rust producer (PHP/TS items never carry it).", Extractor),
        attr("visibility", "enum", "Rust visibility: `pub`, `pub(crate)`, `pub(super)`, `private`, or `pub(in <path>)`. SchemaVersion v0.1.1+ only — legacy V0_1_0 graphs do not carry this attribute.", Extractor),
    ]
}

/// `enrich_metrics`-provenance attributes on `:Item` — populated by
/// `PetgraphStore::enrich_metrics` (RFC-036 §3.3 / issue #203) when the
/// `quality-metrics` feature is active. Descriptors were reserved in
/// V0_3_0 and became load-bearing in V0_3_1 (producer landing). G6
/// invariant: `test_coverage` is toolchain-version-scoped (depends on
/// `cargo-llvm-cov` output) and excluded from the G1 canonical-dump
/// sha256; the other three attrs participate in G1 as normal.
fn item_attrs_enrich_metrics() -> Vec<AttributeDescriptor> {
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
fn item_attrs_enrich_git_history() -> Vec<AttributeDescriptor> {
    use Provenance::EnrichGitHistory;
    vec![
        attr("git_commit_count", "int?", "Number of git commits touching the defining file. Written by `enrich_git_history()` (RFC addendum §A2.2 row 1). Populated by slice 43-B (issue #105) behind the `git-enrich` feature flag; reserved in slice 43-A.", EnrichGitHistory),
        attr("git_last_author", "string?", "Committer email of the most recent commit touching the defining file. Written by `enrich_git_history()`. Populated by slice 43-B.", EnrichGitHistory),
        attr("git_last_commit_unix_ts", "int?", "Unix epoch seconds (i64) of the most recent commit touching the defining file. Stored as an absolute timestamp rather than a calendar-relative age — clean-arch B2: `git_age_days` computed at enrichment time would violate G1 byte-stability across calendar days. The Stage-2 classifier Cypher computes `age_delta` from this timestamp at query time.", EnrichGitHistory),
    ]
}

/// `enrich_reachability`-provenance attributes on `:Item` — populated by
/// slice 43-G (issue #110), extended by RFC-042 042-B (issue #392) with
/// the production-only twin attrs that exclude `:EntryPoint{kind ∈ {test, bench}}`.
fn item_attrs_enrich_reachability() -> Vec<AttributeDescriptor> {
    use Provenance::EnrichReachability;
    vec![
        attr("reachable_entry_count", "int?", "Number of distinct `:EntryPoint` nodes reaching this item via `CALLS*` edges. Written by `enrich_reachability()` (RFC addendum §A2.2 row 5). `0` for items not reached from any entry point. Populated by slice 43-G (issue #110) — consumes `:EntryPoint` nodes from `cfdb-hir-extractor`. Reserved in slice 43-A.", EnrichReachability),
        attr("reachable_from_entry", "bool?", "True when at least one `:EntryPoint` reaches this item via `CALLS*`. Written by `enrich_reachability()`. When the keyspace has zero `:EntryPoint` nodes the pass returns `ran: false` rather than silently marking all items unreachable (clean-arch B3 degraded path). Populated by slice 43-G.", EnrichReachability),
        attr("reachable_production_entry_count", "int?", "Number of distinct production `:EntryPoint` nodes (i.e. `kind ∉ {test, bench}`) reaching this item via `CALLS*` edges. Written by `enrich_reachability()`'s ProductionOnly pass (RFC-042 042-B / issue #392). `0` for items not reached from any production entry point. Used by `classifier-unwired-production.cypher` to surface code that is technically reached by tests/benches but has no production caller.", EnrichReachability),
        attr("reachable_from_production_entry", "bool?", "True when at least one production `:EntryPoint` (i.e. `kind ∉ {test, bench}`) reaches this item via `CALLS*`. Written by `enrich_reachability()`'s ProductionOnly pass (RFC-042 042-B / issue #392). ORTHOGONAL to `:Item.is_test` — `is_test` is a compile-scope flag on the item itself, while `reachable_from_production_entry` is a graph property derived from `:EntryPoint.kind` filtering. An item with `is_test = false` may still have `reachable_from_production_entry = false` if all its reaching entries are test-kind (i.e. only test code exercises it).", EnrichReachability),
    ]
}

pub(in crate::schema::describe) fn field_node_descriptor() -> NodeLabelDescriptor {
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

pub(in crate::schema::describe) fn variant_node_descriptor() -> NodeLabelDescriptor {
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

pub(in crate::schema::describe) fn param_node_descriptor() -> NodeLabelDescriptor {
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
