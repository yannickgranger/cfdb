# Spec: cfdb-classify

The judgment layer over cfdb's code facts: the six-class `DebtClass` taxonomy, `Finding` rows, the `ScopeInventory` / `ClassifyEnvelope` wire envelopes, the closed `TriggerId` registry with its `CheckReport`, and the `ClassifyEngine` that runs the classifier rules for `cfdb scope` / `cfdb classify` and the editorial-drift triggers for `cfdb check`. Two bounded contexts share the crate and never import from each other (debt classification; triggers) — `tests/module_wall.rs`. Generic over `cfdb_core::graph::GraphBackend` (through `cfdb_eval::QueryEngine`) — never names a storage engine, never does I/O; the composition root (`cfdb-cli`) loads the store, prints, writes and exits. Skill routing is external to cfdb (RFC-cfdb §A2.3): no routing table or loader lives here (`tests/finding_no_skill_field.rs`, `cfdb-query/tests/skill_routing_deleted.rs`). One section per `pub` type (graph-specs gate).

## CanonicalCandidate

<!-- parent:spec:ScopeInventory -->

A candidate for the canonical form of a duplicated concept — qname, usage count, owning crate. Populated by `cfdb scope` from Pattern A (horizontal split-brain) findings.

## CheckReport

<!-- parent:rfc:cfdb-059-classify-split#3.1.5 anchor:"What `cfdb check` prints today as one merged QueryResult" -->

One `cfdb check` run, typed — `{trigger: TriggerId, rows: Vec<Row>, warnings: Vec<Warning>}` plus `row_count()`. The rows are already projected to `cfdb_core::result::Row` / `RowValue` exactly as the T1 / T3 runners have always projected them (T1: `verdict, context_name, canonical_crate, owning_rfc, evidence`; T3: `name, kind, n, n_crates, n_contexts, crates[], bounded_contexts[], qnames[], files[], is_cross_context, canonical_candidate`; absent values are `PropValue::Null`); the two per-trigger row types share no field and are never unified. `warnings` carries the trigger's own warnings (T1's `EmptyResult` on an empty `:RfcDoc` set), not the primitive reads'. The composition root prints `violations: N (rule: trigger Tn)`, serialises `rows` + `warnings` as the merged `QueryResult` payload `cfdb check` has always printed and maps `row_count()` to exit 30 / 0. Pinned by `tests/check_report_golden.rs`.

## ClassifyEngine

<!-- parent:rfc:cfdb-059-classify-split#3.1.1 anchor:"pub struct ClassifyEngine<'s, S: GraphBackend> {" -->

The judgment engine over one store — `ClassifyEngine<'s, S: GraphBackend>` holds a `cfdb_eval::QueryEngine<'s, S>` by value (the one way it reaches a keyspace) and exposes `scope(keyspace, context, &ScopeOptions, Option<&ExplainSink>) -> ScopeInventory`, `classify(keyspace, context, &DiffEnvelope) -> ClassifyEnvelope` and `check(keyspace, TriggerId) -> CheckReport`. Dispatch and orchestration only: it validates the context, runs the classifier rules through the `scope` primitives, dispatches a trigger to its `check` runner and assembles the payloads; rule execution, Cypher construction and row projection stay in submodules. Third engine of the `EnrichEngine` / `QueryEngine` family; constructed by `cfdb-cli`'s `compose::classify_engine` and built once per verb invocation, so `check` runs every primitive read of a trigger on one loaded keyspace. Warnings travel inside the payload (`ScopeInventory::warnings`, `CheckReport::warnings`), exactly as the CLI has always emitted them.

## ClassifyEnvelope

<!-- parent:rfc:cfdb-059-classify-split#3.1.3 anchor:"the versioned ClassifyEnvelope" -->

The JSON wire envelope emitted by `cfdb classify` (#213) — `{schema_version, inventory: ScopeInventory, diff_source: DiffSourceMeta}`. Composes a classifier-populated `ScopeInventory` (findings restricted to qnames in the upstream diff) with a `DiffSourceMeta` that identifies the source diff. `schema_version` is `CLASSIFY_ENVELOPE_SCHEMA_VERSION` (`"v1"`) — bumped independently of `DiffEnvelope::schema_version` and `cfdb_core::SchemaVersion`. Consumed by qbot-core #3736's per-PR drift gate. Routing from `DebtClass` → skill is external to cfdb (the consumer's concern) per RFC-cfdb.md §A2.3.

## ClassifyError

<!-- parent:rfc:cfdb-059-classify-split#3.1.11 anchor:"one `#[non_exhaustive]` `thiserror` enum" -->

Everything a `scope` / `classify` / `check` run can fail with — `Store(StoreError)` (a query the engine cannot degrade), `Parse { rule, source }` (an embedded rule or trigger read failed to parse: a build defect), `UnknownContext { context, known }` (the requested bounded context is not a `:Context` node; renders `unknown context `x`; known contexts: [a, b]`). `#[non_exhaustive]`; the CLI maps `Store` to its store error and everything else to a usage error, so exit codes and messages are unchanged by the crate move. An unknown trigger id never reaches the engine — `TriggerId` is parsed at the CLI boundary (`UnknownTriggerId`).

## DebtClass

<!-- parent:rfc:cfdb-059-classify-split#4.8 anchor:"The taxonomy is closed and unchanged" -->

The six-variant canonical debt taxonomy used by the `cfdb scope` verb (`DuplicatedFeature`, `ContextHomonym`, `UnfinishedRefactor`, `RandomScattering`, `CanonicalBypass`, `Unwired`). Serde key naming is snake_case to match the RFC-029 addendum §A2.1 JSON schema.

## DiffSourceMeta

<!-- parent:spec:ClassifyEnvelope -->

Projection of the upstream `DiffEnvelope`'s identity — `{a, b, restrict_count}`. Carried on every `ClassifyEnvelope` (#213) so consumers can correlate classify output with the specific diff that drove the restriction. `restrict_count` is the cardinality of the qname set derived from the diff's `added` ∪ `changed` facts.

## ExplainSink

The `--explain` accumulator handed to `ClassifyEngine::scope`: interior-mutable (`&self` everywhere) so every scope helper can share one `&ExplainSink`. `enabled()` collects one `ExplainRow` batch per query run through it (via `QueryEngine::execute_explained`); `disabled()` takes the plain `execute` path with zero overhead; `drain()` hands the rows to the caller once. The rows themselves are `cfdb_eval::explain::ExplainRow`.

## Finding

<!-- parent:spec:ScopeInventory -->

A structured debt finding — qname, pattern, class (`DebtClass`), confidence, canonical side, other sides, evidence, age delta, RFC references, bounded contexts. Emitted by the classifier (Phase B / RFC-032 Group D #48).

## ReachabilityEntry

<!-- parent:spec:ScopeInventory -->

An entry in the reachability map — item qname, `reachable_from_entry` boolean, entry-point count.

## ScopeInventory

<!-- parent:rfc:cfdb-059-classify-split#3.1.2 anchor:"the §A3.3 infection inventory for one bounded context" -->

The JSON envelope returned by `cfdb scope` — findings grouped by `DebtClass`, canonical candidates, reachability map, LoC per crate, plus warnings. Consumed by `/operate-module` and similar skills.

## ScopeOptions

Knobs for a `scope` run — today only `production_only: bool`, which swaps the `Unwired` classifier rule to its production-only variant (`reachable_from_production_entry`); `cfdb classify` never sets it. Every knob defaults to off (`Default`).

## TriggerId

<!-- parent:rfc:cfdb-059-classify-split#3.1.4 anchor:"the closed trigger registry (T1, T3)" -->

Editorial-drift trigger identifier used by the `cfdb check --trigger <ID>` verb (qbot-core council-4046 Phase 2 naming) — a closed enum, `T1` (concept-declared-in-TOML-but-missing-in-code) and `T3` (concept-name-in-≥2-crates). `TriggerId::variants()` is the single source of truth for valid values — the `FromStr` impl iterates it and the `UnknownTriggerId::Display` impl enumerates it, so the valid-values list in parse-error strings never diverges from the enum (global `CLAUDE.md` §7 MCP/CLI boundary-fix AC). DDD homonym of `check_prelude_triggers::TriggerId` (the `C1..C9` mechanical pre-council triggers, `specs/tools/check-prelude-triggers.md`): different bounded contexts, different serialization namespaces, independent change vectors.

## UnknownDebtClass

Error type for unrecognised `DebtClass` string values during deserialisation.

## UnknownTriggerId

<!-- parent:spec:ClassifyError -->

Parse error for `TriggerId::from_str`. Carries the rejected input string so the `Display` impl can produce a `unknown TriggerId 'X' — valid values: T1, …` message whose valid-values list is derived live from `TriggerId::variants()` (no hardcoded enumeration). Returned by clap's `value_parser!(TriggerId)` wiring in `cfdb-cli`; the CLI dispatcher maps it to `CfdbCliError::Usage` at the boundary.
