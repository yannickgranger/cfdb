# Spec: cfdb-query

Cypher-subset parser (chumsky 0.10) plus a Rust builder API. Both produce the same `cfdb_core::Query` AST. Contains the scanner primitives that the DSL (#49) will share — RFC-031 §6 gates that unification.

## ParseError

The parser's error type — carries source span, expected token set, and the raw Cypher input for user-facing diagnostics.

## QueryBuilder

A fluent Rust API that constructs a `cfdb_core::Query` programmatically, as an alternative to parsing a Cypher string. Primary consumers are the verb composers (e.g. `list_items_matching`) and integration tests that need to build a query without round-tripping through source text.

## ShapeLint

A shape-lint finding emitted during parse — flags queries whose shape is likely a mistake (e.g. cartesian function-equality — the main v0.1 example). Non-fatal; surfaced to the caller as warnings rather than errors.
