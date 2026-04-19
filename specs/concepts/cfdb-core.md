---
crate: cfdb-core
rfc: RFC-029, RFC-030
status: approved
---

# Spec: cfdb-core

The schema vocabulary, fact types, query AST, result types, and the `StoreBackend` port — the innermost layer that all other crates depend on and that depends on nothing in the workspace.

## Facts

### PropValue

The value type for a node or edge property. An open newtype per the ratified RFC-029 §7.1 encoding — string-keyed, not enum-discriminated, so consumers depend only on the variants they query.

### Node

A labelled, property-carrying graph node. Carries a stable `id` (a qname derived from the extraction path), one or more `labels`, and a `Props` map.

### Edge

A directed, labelled graph edge from a source node id to a target node id.

### Props

Type alias for the property map shared by `Node` and `Edge`.

## Query AST

### Query

The root AST node for a parsed or builder-constructed Cypher-subset query. Carries the full pattern list, predicates, with-clause, return-clause, and parameter bindings.

### Pattern

A query pattern — either a `NodePattern` or a `PathPattern`. The top-level list of patterns inside a `MATCH` clause.

### NodePattern

A single-node pattern with an optional variable binding, optional label filter, and optional property predicates.

### PathPattern

A two-endpoint path pattern with a direction, an optional edge label filter, and optional variable-length bounds.

### EdgePattern

The edge component of a `PathPattern`: label filter, direction, and variable-length bounds.

### Direction

Traversal direction for a path pattern: `Outgoing`, `Incoming`, or `Either`.

### Predicate

A filter condition: property equality, comparison, regex match, `IS NULL`, or boolean `AND`/`OR`/`NOT` combinations.

### CompareOp

The set of comparison operators supported in predicates: `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`.

### Expr

A query expression used in `WITH` and `RETURN` clauses: a literal, a property access, a function call, an aggregation, or an arithmetic combination.

### Aggregation

The aggregation functions supported in `RETURN` and `WITH`: `Count`, `CountDistinct`, `Collect`, `Min`, `Max`, `Sum`, `Avg`.

### WithClause

The `WITH` clause of a query: projections that filter and rebind variables between `MATCH` and `RETURN`.

### ReturnClause

The `RETURN` clause of a query: projections, an optional `ORDER BY`, an optional `SKIP`, and an optional `LIMIT`.

### Projection

A single column in a `RETURN` or `WITH` clause: an expression and an optional alias.

### ProjectionValue

Discriminates between a regular expression projection and a wildcard (`*`) projection.

### OrderBy

An expression and a sort direction (`Asc` or `Desc`) for the `ORDER BY` clause.

### Param

A query parameter binding: named (`$name`) or positional.

## Query verbs

### ItemKind

The vocabulary of item kinds used in `list_items_matching` queries: `Struct`, `Enum`, `Fn`, `Trait`, `TypeAlias`, `Const`, `Static`, `ImplBlock`, `Union`, `Macro`.

Note (RFC-031 §3): `ItemKind` may move to `cfdb-query` in v0.2 if it is determined to be verb-level vocabulary rather than schema vocabulary. This spec reflects current state.

### Finding

A structured debt finding produced by the classifier: carries `id`, `pattern`, `class` (`DebtClass`), `confidence`, `canonical_side`, `other_sides`, `evidence`, `age_delta_days`, `rfc_references`, `bounded_contexts`, and `is_cross_context`.

### DebtClass

The six debt-cause classes from RFC-029 §A2.1: `DuplicatedFeature`, `ContextHomonym`, `UnfinishedRefactor`, `RandomScattering`, `CanonicalBypass`, `Unwired`.

### CanonicalCandidate

A candidate for the canonical form of a duplicated concept: carries the qname, usage count, and the crate it lives in.

### ReachabilityEntry

An entry in the reachability map: maps an item qname to a boolean `reachable_from_entry` and a count of entry points that reach it.

### ScopeInventory

The structured output of `cfdb scope`: findings grouped by `DebtClass`, canonical candidates, reachability map, and LoC per crate. Consumed by `/operate-module`.

## Results

### QueryResult

The output of `StoreBackend::execute`: a list of `Row` values and a list of `Warning` values. Warnings are non-fatal — they describe undocumented schema references encountered during evaluation (RFC-029 §6A.1).

### Row

Type alias for a result row: a `BTreeMap` from column name to `RowValue`.

### RowValue

The value type for a result row cell: mirrors `PropValue` plus a `Null` variant and a `List` variant for aggregation outputs.

### Warning

A non-fatal diagnostic produced during query evaluation: carries a `WarningKind` and a human-readable message.

### WarningKind

Discriminates warning categories: `UndocumentedLabel`, `UndocumentedEdge`, `UndocumentedAttribute`, `UnresolvedParameter`.

## Schema vocabulary

### Label

An open newtype wrapping a node label string. The label vocabulary is defined by the schema descriptors; no exhaustive enum is used (RFC-029 §7.1).

### EdgeLabel

An open newtype wrapping an edge label string.

### Keyspace

An open newtype wrapping a keyspace identifier string. Keyspace names are workspace-scoped and stable across runs.

### SchemaVersion

A versioned schema identifier: major and minor. Backends assert schema compatibility on `execute` and `ingest_*`.

### Provenance

Describes where a schema element came from: `Core` (shipped with cfdb), `UserDefined` (registered at runtime by the consumer).

### AttributeDescriptor

Metadata for a single node or edge attribute: name, value kind, provenance, and documentation string.

### NodeLabelDescriptor

Metadata for a node label: name, provenance, list of `AttributeDescriptor`s, and documentation string.

### EdgeLabelDescriptor

Metadata for an edge label: source label filter, target label filter, list of `AttributeDescriptor`s, and documentation string.

### SchemaDescribe

The full schema introspection payload returned by the `schema_describe` verb: version, list of `NodeLabelDescriptor`s, list of `EdgeLabelDescriptor`s.

## Enrichment

### EnrichReport

The result of an enrichment pass: a verb name, a `completed` flag, an optional `message`, and a count of facts added.

## Port

### StoreBackend

```rust
pub trait StoreBackend: Send + Sync {
    fn ingest_nodes(&mut self, keyspace: &Keyspace, nodes: Vec<Node>) -> Result<(), StoreError>;
    fn ingest_edges(&mut self, keyspace: &Keyspace, edges: Vec<Edge>) -> Result<(), StoreError>;
    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError>;
    fn schema_version(&self, keyspace: &Keyspace) -> Result<SchemaVersion, StoreError>;
    fn list_keyspaces(&self) -> Vec<Keyspace>;
    fn drop_keyspace(&mut self, keyspace: &Keyspace) -> Result<(), StoreError>;
    fn canonical_dump(&self, keyspace: &Keyspace) -> Result<String, StoreError>;
    fn enrich_docs(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>;
    fn enrich_metrics(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>;
    fn enrich_history(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>;
    fn enrich_concepts(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>;
}
```

Note (RFC-031 §2): `StoreBackend` will be split into `StoreBackend` (ingest + query + lifecycle) and a new `EnrichBackend` trait (the four `enrich_*` methods). This spec reflects current state; it will be updated in the same PR as that split.

### StoreError

The error type produced by backend operations: `UnknownKeyspace`, `SchemaMismatch`, `Eval`, `Ingest`, `Io`, `Other`.

### UnknownItemKind

Error type for unrecognised `ItemKind` string values during deserialisation.

### UnknownDebtClass

Error type for unrecognised `DebtClass` string values during deserialisation.
