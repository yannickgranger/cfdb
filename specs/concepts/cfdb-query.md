# Spec: cfdb-query

Cypher-subset parser (chumsky 0.10) plus a Rust builder API. Both produce the same `cfdb_core::Query` AST. Also hosts the verb-level query composers and the debt-class taxonomy moved from `cfdb-core` per RFC-031 §3 (CRP — composer and taxonomy change with the CLI verb surface, not with the schema).

## CanonicalCandidate

A candidate for the canonical form of a duplicated concept — qname, usage count, owning crate. Populated by `cfdb scope` from Pattern A (horizontal split-brain) findings.

## DebtClass

The six-variant canonical debt taxonomy used by the `cfdb scope` verb (`DuplicatedFeature`, `ContextHomonym`, `UnfinishedRefactor`, `RandomScattering`, `CanonicalBypass`, `Unwired`). Serde key naming is snake_case to match the RFC-029 addendum §A2.1 JSON schema.

## Finding

A structured debt finding — qname, pattern, class (`DebtClass`), confidence, canonical side, other sides, evidence, age delta, RFC references, bounded contexts. Emitted by the classifier (Phase B / RFC-032 Group D #48).

## ParseError

The parser's error type — carries source span, expected token set, and the raw Cypher input for user-facing diagnostics.

## QueryBuilder

A fluent Rust API that constructs a `cfdb_core::Query` programmatically, as an alternative to parsing a Cypher string. Primary consumers are the verb composers (e.g. `list_items_matching`) and integration tests that need to build a query without round-tripping through source text.

## ReachabilityEntry

An entry in the reachability map — item qname, `reachable_from_entry` boolean, entry-point count.

## ScopeInventory

The JSON envelope returned by `cfdb scope` — findings grouped by `DebtClass`, canonical candidates, reachability map, LoC per crate, plus warnings. Consumed by `/operate-module` and similar skills.

## ShapeLint

A shape-lint finding emitted during parse — flags queries whose shape is likely a mistake (e.g. cartesian function-equality — the main v0.1 example). Non-fatal; surfaced to the caller as warnings rather than errors.

## UnknownDebtClass

Error type for unrecognised `DebtClass` string values during deserialisation.
