# Spec: cfdb-core

The schema vocabulary, fact types, query AST, result types, the `StoreBackend` port, and the sibling `EnrichBackend` port — the innermost layer that every other cfdb crate depends on and that depends on nothing in the workspace.

`cfdb-core` is intentionally in the **Zone of Pain** (D ≈ 0.95 — high stability, near-zero abstractness) and `cfdb-concepts` is at D = 1.00. This is accepted architectural debt: both are maximally stable zero-dep foundation crates whose vocabulary is the wire contract for every downstream keyspace. The mitigation is procedural, not structural — changes here evolve only via RFC-gated additive bumps of `SchemaVersion` with a lockstep graph-specs-rust PR per CLAUDE.md §3. Unilateral modification is the drift mode we prevent by making every schema-surface change pass through the council + dogfood gates.

## Aggregation

Aggregation functions supported in `RETURN` and `WITH` clauses.

## AttributeDescriptor

Metadata for a single node or edge attribute — name, value kind, provenance, documentation.

## CfgGate

Feature-only `#[cfg(...)]` expression tree captured on `:Item.cfg_gate`: `Feature("x")` for `feature = "x"`, `All(children)`, `Any(children)`, `Not(child)`. Added in SchemaVersion v0.1.2 per Issue #36. `Display`/`FromStr` round-trip canonical wire strings; `evaluate(&[&str]) -> bool` so consumers can ask "is this item active under feature set X". All-or-nothing capture: the extractor omits the attribute when any non-feature cfg predicate appears on the item.

## CompareOp

Comparison operators available in predicates (`Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`).

## ContextSource

Provenance discriminator for `:Context` nodes (RFC-038). `Declared` is author-asserted in `.cfdb/concepts/<name>.toml`; `Heuristic` is auto-derived by `cfdb_concepts::compute_bounded_context` via crate-name prefix stripping. Wire form via `Display`/`FromStr` round-trips through `:Context.source` prop. Closed-set wire enum (no variant carries owned data), so `as_wire_str` returns `&'static str` per RFC-038 §3.1.

## Direction

Traversal direction for a path pattern — outgoing, incoming, or either.

## Edge

A directed, labelled graph edge from a source node id to a target node id.

## EdgeLabel

Open newtype wrapping an edge-label string. The label vocabulary is defined by schema descriptors.

## EdgeLabelDescriptor

Metadata for an edge label — source filter, target filter, attributes, documentation.

## EdgePattern

The edge component of a `PathPattern` — label filter, direction, variable-length bounds.

## EnrichBackend

The enrichment port — sibling of `StoreBackend`. Split out of the fat trait per RFC-031 §2 (ISP). The trait ships **seven** default stubs returning `EnrichReport::not_implemented` (`enrich_git_history`, `enrich_rfc_docs`, `enrich_deprecation`, `enrich_bounded_context`, `enrich_concepts`, `enrich_reachability`, `enrich_metrics`); concrete enrichment passes override methods as RFC-032 §4 / Group D issues land (#43–#48). `PetgraphStore` impls the trait with an empty body — inherited stubs only.

The verb surface is **closed at seven** under the 11-verb API (RFC-036 §3.3 + RFC-031 §2). New enrichment functionality extends existing verbs via internal module decomposition (see `cfdb-petgraph` → `enrich_metrics` 3-module split under `quality-metrics` feature) rather than adding trait methods. Growing the verb count requires council approval.

## EnrichReport

The result of an enrichment pass — verb, completed flag, optional message, facts-added count.

## Expr

A query expression used in `WITH` and `RETURN` — literal, property access, function call, aggregation, or arithmetic combination.

## ItemKind

Vocabulary for the `list-items-matching` verb. Kept in `cfdb-core` for v0.1; may move to `cfdb-query` in v0.2 per RFC-031 §3 if determined to be verb-level rather than schema-level.

## Keyspace

Open newtype wrapping a keyspace identifier string. Keyspace names are workspace-scoped and stable across runs.

## Label

Open newtype wrapping a node-label string. The label vocabulary is defined by schema descriptors — no exhaustive enum (RFC-029 §7.1). The open-newtype shape is the **OCP extension point** for schema: adding a new node label (e.g. `:EntryPoint` in v0.2.0 per Issue #86, `:Param` producer in v0.2.x per Issue #209) is a registration against the descriptor table, not a source-level modification of the `Label` type. Additions land under minor `SchemaVersion` bumps with a lockstep graph-specs-rust fixture bump per CLAUDE.md §3. Closed-enum labels would force every consumer to re-compile on every new fact type; the open newtype lets v0.3-aware graphs remain queryable by v0.2 consumers for the subset they recognise.

Current v0.2-era labels include: `Crate`, `Module`, `File`, `Item`, `Field`, `Variant`, `Param` (declared; producer landed in #209), `CallSite`, `EntryPoint` (populated since #86/#124/#125), `Concept`, `Context`. Edge labels include `IN_CRATE`, `IN_MODULE`, `IN_FILE`, `HAS_FIELD`, `HAS_VARIANT`, `HAS_PARAM`, `TYPE_OF`, `IMPLEMENTS`, `IMPLEMENTS_FOR`, `RETURNS`, `EXPOSES`, `REGISTERS_PARAM`, `CALLS`, `INVOKES_AT`. The authoritative registry lives in `schema/describe/nodes.rs` + `schema/describe/edges.rs`; this paragraph is documentation, not vocabulary definition.

## Node

A labelled, property-carrying graph node. Carries a stable id (qname), one or more labels, and a property map.

## NodeLabelDescriptor

Metadata for a node label — name, provenance, attributes, documentation.

## NodePattern

A single-node pattern with optional variable binding, optional label filter, optional property predicates.

## OrderBy

An expression paired with a sort direction, used in the `ORDER BY` clause.

## Param

A **query parameter binding** — named (`$name`) or positional. Lives at `query::ast::Param`. Homonym note (flagged by RFC-036 council DDD lens): the graph-node label string `"Param"` (`Label::PARAM`, producing `:Param` nodes carrying a fn/method parameter's `index`, `name`, `is_self`, `parent_qname`, `type_path`, `type_normalized`) is an **unrelated concept** — same word, different domain. The query-AST `Param` is a value supplied by the caller at query time; the graph-node `:Param` is a structural fact emitted by the extractor. A future boy-scout PR renames this query-AST type to `ParamBinding` to eliminate the homonym at source (per RFC-036 §3.1 council decision, DDD condition 2); until then, the two are disambiguated by context (`cfdb_core::query::ast::Param` vs `cfdb_core::schema::Label::PARAM`).

## PathPattern

A two-endpoint path pattern with a direction, an optional edge-label filter, and optional variable-length bounds.

## Pattern

A query pattern — either a `NodePattern` or a `PathPattern`. Top-level inside a `MATCH` clause.

## Predicate

A filter condition — equality, comparison, regex match, `IS NULL`, or boolean `AND`/`OR`/`NOT` combinations.

## Projection

A single column in a `RETURN` or `WITH` clause — expression plus optional alias.

## ProjectionValue

Discriminates between a regular expression projection and a wildcard (`*`) projection.

## PropValue

The value type for a node or edge property. Open newtype per RFC-029 §7.1 — string-keyed, not enum-discriminated.

## Props

Type alias for the property map shared by `Node` and `Edge`.

## Provenance

Where a schema element (node attribute, edge attribute) originated. Six variants:

- `Extractor` — written by the syn-based `cfdb-extractor` during `extract`.
- `EnrichRfcDocs` — written by `enrich_rfc_docs` (RFC-reference facts).
- `EnrichMetrics` — written by `enrich_metrics` (quality signals: `unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id`). `dup_cluster_id` is assigned as `sha256(lex_sorted(member_qnames).join("\n"))` — deterministic across re-extractions, insensitive to iteration order (RFC-036 CP5). `test_coverage` is excluded from G1 canonical-dump byte-stability per G6 (see `SchemaVersion`).
- `EnrichGitHistory` — written by `enrich_git_history` (commit age, author, churn).
- `EnrichConcepts` — written by `enrich_concepts` (`:Concept` materialisation + `LABELED_AS` / `CANONICAL_FOR` edges).
- `EnrichReachability` — written by `enrich_reachability` (entry-point BFS over `CALLS*`).

## Query

The root AST node for a parsed or builder-constructed Cypher-subset query.

## QueryResult

The output of `StoreBackend::execute` — list of `Row` values and list of `Warning` values.

## ReturnClause

The `RETURN` clause — projections, optional `ORDER BY`, optional `SKIP`, optional `LIMIT`.

## Row

Type alias for a result row — a `BTreeMap` from column name to `RowValue`.

## RowValue

Value type for a result row cell — mirrors `PropValue` plus `Null` and `List` variants for aggregation output.

## SchemaDescribe

The full schema introspection payload returned by `schema_describe` — version, node-label descriptors, edge-label descriptors.

## SchemaVersion

Versioned schema identifier (major + minor). Backends assert compatibility on `execute` and `ingest_*`.

Five determinism guarantees govern the wire contract (RFC-029 §6, formalised and inlined here for RFC-036 council consumption):

- **G1** — same `(workspace SHA, schema major.minor)` → byte-identical canonical JSONL dump.
- **G2** — `query()` and `query_with_input()` are read-only; no query mutates the graph.
- **G3** — `enrich_*()` is additive; no enrichment deletes structural facts.
- **G4** — monotonic within a major: a v0.x graph is readable by any v0.y reader where `y ≥ x` (same major). A lower-(minor, patch) reader **correctly refuses** a higher-(minor, patch) graph per `SchemaVersion::can_read` (`schema/labels.rs:312-315`: `self.major == graph.major && graph <= self`) — this is the intended forward-incompatibility signal when a minor bump introduces new node/edge types older readers cannot handle.
- **G5** — snapshots are immutable; keyspaces are never rewritten in place, only dropped or replaced wholesale.

**G6 — toolchain-scoped attributes (RFC-036 §4, additive, no existing guarantee broken).** The `:Item.test_coverage` attribute (written by `enrich_metrics` via `cargo-llvm-cov` JSON) is byte-stable only **within the same Rust toolchain version**. It is **excluded from the G1 canonical-dump sha256** and declared as toolchain-version-scoped in `SchemaDescribe` output. Callers that need cross-toolchain comparability record the toolchain version alongside the keyspace themselves; cfdb does not record it automatically. Any future attribute with similar toolchain-dependent provenance must be declared under G6 at introduction and excluded from G1.

## StoreBackend

The graph-store port. Implementations ingest facts, execute queries, emit canonical dumps, and manage keyspace lifecycle (7 methods). Enrichment now lives on the sibling `EnrichBackend` trait (RFC-031 §2). v0.1+ has one implementor — `cfdb-petgraph::PetgraphStore` — which implements both traits.

The verb surface is **closed at seven** under the 11-verb API (RFC-036 §3 + RFC-029 §A1). New capability extends existing verbs via schema + Cypher composition, not via new trait methods. The orthogonal API's 11-verb ceiling (`StoreBackend`'s 7 + `EnrichBackend`'s 7 overlap plus the external `extract`/`schema_version`/`schema_describe` shapes) is enforced by council review on every RFC.

## StoreError

Error type produced by backend operations — `UnknownKeyspace`, `SchemaMismatch`, `Eval`, `Ingest`, `Io`, `Other`.

## UnknownItemKind

Error type for unrecognised `ItemKind` string values during deserialisation.

## Visibility

Rust item visibility captured on `:Item` facts: `Public` (`pub`), `CrateLocal` (`pub(crate)`), `Module` (`pub(super)` or `pub(self)`), `Private` (inherited), `Restricted(path)` (`pub(in path)`). Added in SchemaVersion v0.1.1 per Issue #35. Wire form via `Display`/`FromStr`.

## Warning

Non-fatal diagnostic produced during query evaluation — a `WarningKind` plus a human-readable message.

## WarningKind

Categories of warning — undocumented label, undocumented edge, undocumented attribute, unresolved parameter.

## WithClause

The `WITH` clause — projections that filter and rebind variables between `MATCH` and `RETURN`.
