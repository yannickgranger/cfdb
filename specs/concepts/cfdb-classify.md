# Spec: cfdb-classify

The judgment layer over cfdb's code facts: the six-class `DebtClass` taxonomy, `Finding` rows, the `ScopeInventory` / `ClassifyEnvelope` wire envelopes. One section per `pub` type (graph-specs gate). Skill routing is external to cfdb (RFC-cfdb §A2.3): no routing table or loader lives here (`tests/finding_no_skill_field.rs`, `cfdb-query/tests/skill_routing_deleted.rs`).

## CanonicalCandidate

A candidate for the canonical form of a duplicated concept — qname, usage count, owning crate. Populated by `cfdb scope` from Pattern A (horizontal split-brain) findings.

## ClassifyEnvelope

The JSON wire envelope emitted by `cfdb classify` (#213) — `{schema_version, inventory: ScopeInventory, diff_source: DiffSourceMeta}`. Composes a classifier-populated `ScopeInventory` (findings restricted to qnames in the upstream diff) with a `DiffSourceMeta` that identifies the source diff. `schema_version` is `CLASSIFY_ENVELOPE_SCHEMA_VERSION` (`"v1"`) — bumped independently of `DiffEnvelope::schema_version` and `cfdb_core::SchemaVersion`. Consumed by qbot-core #3736's per-PR drift gate. Routing from `DebtClass` → skill is external to cfdb (the consumer's concern) per RFC-cfdb.md §A2.3.

## DebtClass

The six-variant canonical debt taxonomy used by the `cfdb scope` verb (`DuplicatedFeature`, `ContextHomonym`, `UnfinishedRefactor`, `RandomScattering`, `CanonicalBypass`, `Unwired`). Serde key naming is snake_case to match the RFC-029 addendum §A2.1 JSON schema.

## DiffSourceMeta

Projection of the upstream `DiffEnvelope`'s identity — `{a, b, restrict_count}`. Carried on every `ClassifyEnvelope` (#213) so consumers can correlate classify output with the specific diff that drove the restriction. `restrict_count` is the cardinality of the qname set derived from the diff's `added` ∪ `changed` facts.

## Finding

A structured debt finding — qname, pattern, class (`DebtClass`), confidence, canonical side, other sides, evidence, age delta, RFC references, bounded contexts. Emitted by the classifier (Phase B / RFC-032 Group D #48).

## ReachabilityEntry

An entry in the reachability map — item qname, `reachable_from_entry` boolean, entry-point count.

## ScopeInventory

The JSON envelope returned by `cfdb scope` — findings grouped by `DebtClass`, canonical candidates, reachability map, LoC per crate, plus warnings. Consumed by `/operate-module` and similar skills.

## UnknownDebtClass

Error type for unrecognised `DebtClass` string values during deserialisation.
