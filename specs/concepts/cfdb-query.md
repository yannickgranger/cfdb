---
crate: cfdb-query
rfc: RFC-029, RFC-030
status: approved
---

# Spec: cfdb-query

The Cypher-subset parser (chumsky-based) and Rust builder API — both surfaces produce the same `Query` AST defined in `cfdb-core`. Depends on `cfdb-core`; no other workspace dependency.

## Parser

### ParseError

The error type produced by the Cypher-subset parser. Carries position information and a human-readable description of what was expected vs. what was found.

## Builder

### QueryBuilder

The fluent Rust builder API for constructing `Query` AST values without writing Cypher text. Provides typed methods for `MATCH`, `WHERE`, `WITH`, and `RETURN` clauses. Produces the same `Query` AST that the parser produces, so both paths are interchangeable at the evaluation boundary.

## Lint

### ShapeLint

A structural lint finding produced by the query shape linter. Describes a query that is syntactically valid but structurally suspect (e.g. a `RETURN *` with no preceding `MATCH`, an empty `WHERE` clause, a `WITH` that projects nothing).

## Query composers

Note: `cfdb-core/src/query/list_items.rs` and `ItemKind` currently live in `cfdb-core`. RFC-031 §3 prescribes moving them to this crate. This spec anticipates the target state to lock the intended ownership boundary. Until the move lands, the graph-specs gate is informational-only for these two items; it becomes blocking once the move PR merges.

### list_items_matching (function — verb-level composer)

The `list_items_matching` query composer that builds the `list-items-matching` verb query. Takes a `name_pattern`, optional `kinds` filter, and optional `group_by_context` flag; returns a `Query` value ready for `StoreBackend::execute`.
