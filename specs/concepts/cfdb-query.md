# Spec: cfdb-query

Cypher-subset parser (chumsky 0.10) plus a Rust builder API. Both produce the same `cfdb_core::Query` AST. Also hosts the verb-level query composers and the debt-class taxonomy moved from `cfdb-core` per RFC-031 §3 (CRP — composer and taxonomy change with the CLI verb surface, not with the schema).

## CanonicalCandidate

A candidate for the canonical form of a duplicated concept — qname, usage count, owning crate. Populated by `cfdb scope` from Pattern A (horizontal split-brain) findings.

## ChangedFact

One row of the `DiffEnvelope::changed` list — carries both the `a` (before) and `b` (after) canonical-dump envelopes for a fact whose key exists on both sides but whose envelope JSON differs (typically `props` drift). Consumers diff at whatever granularity they need. Emitted by `cfdb diff` (#212).

## DebtClass

The six-variant canonical debt taxonomy used by the `cfdb scope` verb (`DuplicatedFeature`, `ContextHomonym`, `UnfinishedRefactor`, `RandomScattering`, `CanonicalBypass`, `Unwired`). Serde key naming is snake_case to match the RFC-029 addendum §A2.1 JSON schema.

## DiffEnvelope

The JSON wire envelope emitted by `cfdb diff` (#212) — `{a, b, schema_version, added, removed, changed, warnings}`. Carries a two-keyspace delta over the canonical sorted-JSONL dump (RFC-cfdb.md §12.1). `schema_version` is `ENVELOPE_SCHEMA_VERSION` (`"v1"`) — bumped independently of `cfdb_core::SchemaVersion` (envelope wire contract ≠ on-disk keyspace contract). Consumed by qbot-core #3736's per-PR drift gate and by `cfdb classify` (#213).

## DiffError

Error type for `compute_diff` and `KindsFilter::from_str` — `Parse { side, line_number, source }` for bad JSON with 1-based line diagnostics, `InvalidEnvelope { side, line_number, reason }` for JSON that lacks the required canonical-dump fields, `UnknownKind { token }` for `--kinds` values other than `node`/`edge`.

## DiffFact

One row of `DiffEnvelope::added` or `removed` — `{kind, envelope}` where `envelope` is the full canonical-dump JSON object (`{id, kind:"node", label, props}` for nodes, `{dst_qname, kind:"edge", label, props, src_qname}` for edges). `kind` is hoisted out of the envelope so consumers can filter without re-parsing.

## Finding

A structured debt finding — qname, pattern, class (`DebtClass`), confidence, canonical side, other sides, evidence, age delta, RFC references, bounded contexts. Emitted by the classifier (Phase B / RFC-032 Group D #48).

## KindsFilter

Filter on the `kind` discriminator for `cfdb diff --kinds`. Parsed from a comma-separated string (`node`, `edge`, `node,edge`); `FromStr` rejects unknown tokens with `DiffError::UnknownKind`. Restricts `compute_diff` to node rows, edge rows, or both — the taxonomy here is dump-line `kind` (`node`/`edge`), NOT the schema-level `ItemKind` used by `list-items-matching`.

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

## SkillRoute

One routing decision loaded from `.cfdb/skill-routing.toml` — class → concrete Claude skill name, `council_required` flag, optional `mode` variant (e.g. `--mode=port`), optional free-form `notes`. DIP-clean: the classifier only emits `DebtClass`; mapping to a skill is external policy per RFC-029 addendum §A2.3.

## SkillRoutingTable

Parsed `.cfdb/skill-routing.toml` content — `schema_version` plus a `classes` map keyed by `DebtClass` snake-case spelling. Consumed by downstream orchestration skills (`/operate-module`, `/boy-scout --from-inventory`) to decide how to act on a `Finding`. Pinned by the `finding_no_skill_field` architecture test: `Finding` MUST NOT carry any skill-related column.

## SkillRoutingLoadError

Error type for `SkillRoutingTable::from_path` / `from_toml_str` — separates filesystem (`Io`) from TOML-parse (`Toml`) failures so the CLI surface can distinguish missing-file from malformed-policy.

## UnknownDebtClass

Error type for unrecognised `DebtClass` string values during deserialisation.
