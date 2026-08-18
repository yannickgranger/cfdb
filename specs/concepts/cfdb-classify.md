# Spec: cfdb-classify

The judgment layer over cfdb's code facts: the six-class `DebtClass` taxonomy, `Finding` rows, the `ScopeInventory` / `ClassifyEnvelope` wire envelopes, and the `ClassifyEngine` that runs the classifier rules for `cfdb scope` / `cfdb classify`. Generic over `cfdb_core::graph::GraphBackend` (through `cfdb_eval::QueryEngine`) — never names a storage engine, never does I/O; the composition root (`cfdb-cli`) loads the store, prints, writes and exits. Skill routing is external to cfdb (RFC-cfdb §A2.3): no routing table or loader lives here (`tests/finding_no_skill_field.rs`, `cfdb-query/tests/skill_routing_deleted.rs`). One section per `pub` type (graph-specs gate).

## CanonicalCandidate

A candidate for the canonical form of a duplicated concept — qname, usage count, owning crate. Populated by `cfdb scope` from Pattern A (horizontal split-brain) findings.

## ClassifyEngine

The judgment engine over one store — `ClassifyEngine<'s, S: GraphBackend>` holds a `cfdb_eval::QueryEngine<'s, S>` by value (the one way it reaches a keyspace) and exposes `scope(keyspace, context, &ScopeOptions, Option<&ExplainSink>) -> ScopeInventory` and `classify(keyspace, context, &DiffEnvelope) -> ClassifyEnvelope`. Dispatch and orchestration only: it validates the context, runs the classifier rules through the `scope` primitives and assembles the payloads; rule execution and Cypher construction stay in submodules. Third engine of the `EnrichEngine` / `QueryEngine` family; constructed by `cfdb-cli`'s `compose::classify_engine`. Warnings travel inside the payload (`ScopeInventory::warnings`), exactly as the CLI has always emitted them.

## ClassifyEnvelope

The JSON wire envelope emitted by `cfdb classify` (#213) — `{schema_version, inventory: ScopeInventory, diff_source: DiffSourceMeta}`. Composes a classifier-populated `ScopeInventory` (findings restricted to qnames in the upstream diff) with a `DiffSourceMeta` that identifies the source diff. `schema_version` is `CLASSIFY_ENVELOPE_SCHEMA_VERSION` (`"v1"`) — bumped independently of `DiffEnvelope::schema_version` and `cfdb_core::SchemaVersion`. Consumed by qbot-core #3736's per-PR drift gate. Routing from `DebtClass` → skill is external to cfdb (the consumer's concern) per RFC-cfdb.md §A2.3.

## ClassifyError

Everything a `scope` / `classify` run can fail with — `Store(StoreError)` (a query the engine cannot degrade), `Parse { rule, source }` (an embedded rule failed to parse: a build defect), `UnknownContext { context, known }` (the requested bounded context is not a `:Context` node; renders `unknown context `x`; known contexts: [a, b]`). `#[non_exhaustive]`; the CLI maps `Store` to its store error and everything else to a usage error, so exit codes and messages are unchanged by the crate move.

## DebtClass

The six-variant canonical debt taxonomy used by the `cfdb scope` verb (`DuplicatedFeature`, `ContextHomonym`, `UnfinishedRefactor`, `RandomScattering`, `CanonicalBypass`, `Unwired`). Serde key naming is snake_case to match the RFC-029 addendum §A2.1 JSON schema.

## DiffSourceMeta

Projection of the upstream `DiffEnvelope`'s identity — `{a, b, restrict_count}`. Carried on every `ClassifyEnvelope` (#213) so consumers can correlate classify output with the specific diff that drove the restriction. `restrict_count` is the cardinality of the qname set derived from the diff's `added` ∪ `changed` facts.

## ExplainSink

The `--explain` accumulator handed to `ClassifyEngine::scope`: interior-mutable (`&self` everywhere) so every scope helper can share one `&ExplainSink`. `enabled()` collects one `ExplainRow` batch per query run through it (via `QueryEngine::execute_explained`); `disabled()` takes the plain `execute` path with zero overhead; `drain()` hands the rows to the caller once. The rows themselves are `cfdb_eval::explain::ExplainRow`.

## Finding

A structured debt finding — qname, pattern, class (`DebtClass`), confidence, canonical side, other sides, evidence, age delta, RFC references, bounded contexts. Emitted by the classifier (Phase B / RFC-032 Group D #48).

## ReachabilityEntry

An entry in the reachability map — item qname, `reachable_from_entry` boolean, entry-point count.

## ScopeInventory

The JSON envelope returned by `cfdb scope` — findings grouped by `DebtClass`, canonical candidates, reachability map, LoC per crate, plus warnings. Consumed by `/operate-module` and similar skills.

## ScopeOptions

Knobs for a `scope` run — today only `production_only: bool`, which swaps the `Unwired` classifier rule to its production-only variant (`reachable_from_production_entry`); `cfdb classify` never sets it. Every knob defaults to off (`Default`).

## UnknownDebtClass

Error type for unrecognised `DebtClass` string values during deserialisation.
