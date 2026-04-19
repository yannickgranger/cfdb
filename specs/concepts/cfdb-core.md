# Spec: cfdb-core

The schema vocabulary, fact types, query AST, result types, the `StoreBackend` port, and the sibling `EnrichBackend` port — the innermost layer that every other cfdb crate depends on and that depends on nothing in the workspace.

## Aggregation

Aggregation functions supported in `RETURN` and `WITH` clauses.

## AttributeDescriptor

Metadata for a single node or edge attribute — name, value kind, provenance, documentation.

## CanonicalCandidate

A candidate for the canonical form of a duplicated concept — qname, usage count, owning crate.

## CompareOp

Comparison operators available in predicates (`Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`).

## DebtClass

The six debt-cause classes from RFC-029 §A2.1 used by the classifier.

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

The enrichment port — sibling of `StoreBackend`. Split out of the fat trait per RFC-031 §2 (ISP). v0.1 ships four default stubs returning `EnrichReport::not_implemented`; concrete enrichment passes override methods as RFC-032 §4 / Group D issues land (#43–#48). `PetgraphStore` impls the trait with an empty body — inherited stubs only.

## EnrichReport

The result of an enrichment pass — verb, completed flag, optional message, facts-added count.

## Expr

A query expression used in `WITH` and `RETURN` — literal, property access, function call, aggregation, or arithmetic combination.

## Finding

A structured debt finding emitted by the classifier. Carries qnames, evidence, `DebtClass`, confidence, RFC references, bounded contexts.

## ItemKind

Vocabulary for the `list-items-matching` verb. Kept in `cfdb-core` for v0.1; may move to `cfdb-query` in v0.2 per RFC-031 §3 if determined to be verb-level rather than schema-level.

## Keyspace

Open newtype wrapping a keyspace identifier string. Keyspace names are workspace-scoped and stable across runs.

## Label

Open newtype wrapping a node-label string. The label vocabulary is defined by schema descriptors — no exhaustive enum (RFC-029 §7.1).

## Node

A labelled, property-carrying graph node. Carries a stable id (qname), one or more labels, and a property map.

## NodeLabelDescriptor

Metadata for a node label — name, provenance, attributes, documentation.

## NodePattern

A single-node pattern with optional variable binding, optional label filter, optional property predicates.

## OrderBy

An expression paired with a sort direction, used in the `ORDER BY` clause.

## Param

A query parameter binding — named (`$name`) or positional.

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

Where a schema element originated — `Core` (shipped with cfdb) or `UserDefined` (registered at runtime).

## Query

The root AST node for a parsed or builder-constructed Cypher-subset query.

## QueryResult

The output of `StoreBackend::execute` — list of `Row` values and list of `Warning` values.

## ReachabilityEntry

An entry in the reachability map — item qname, reachable-from-entry boolean, entry-point count.

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

## ScopeInventory

Structured output of `cfdb scope` — findings grouped by `DebtClass`, canonical candidates, reachability map, LoC per crate.

## StoreBackend

The graph-store port. Implementations ingest facts, execute queries, emit canonical dumps, and manage keyspace lifecycle (7 methods). Enrichment now lives on the sibling `EnrichBackend` trait (RFC-031 §2). v0.1 has one implementor — `cfdb-petgraph::PetgraphStore` — which implements both traits.

## StoreError

Error type produced by backend operations — `UnknownKeyspace`, `SchemaMismatch`, `Eval`, `Ingest`, `Io`, `Other`.

## UnknownDebtClass

Error type for unrecognised `DebtClass` string values during deserialisation.

## UnknownItemKind

Error type for unrecognised `ItemKind` string values during deserialisation.

## Warning

Non-fatal diagnostic produced during query evaluation — a `WarningKind` plus a human-readable message.

## WarningKind

Categories of warning — undocumented label, undocumented edge, undocumented attribute, unresolved parameter.

## WithClause

The `WITH` clause — projections that filter and rebind variables between `MATCH` and `RETURN`.
