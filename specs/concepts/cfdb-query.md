# Spec: cfdb-query

Cypher-subset parser (chumsky 0.10) plus a Rust builder API. Both produce the same `cfdb_core::Query` AST. Also hosts the verb-level query composers (`impact`, `list_items_matching`, `diff`) and the backend-agnostic `shape_lint`. The debt-class taxonomy and the scope/classify envelopes live in `cfdb-classify` (RFC-059).

### ChangedFact

<!-- parent:spec:DiffEnvelope -->

One row of the `DiffEnvelope::changed` list — carries both the `a` (before) and `b` (after) canonical-dump envelopes for a fact whose key exists on both sides but whose envelope JSON differs (typically `props` drift). Consumers diff at whatever granularity they need. Emitted by `cfdb diff` (#212).

## DiffEnvelope

<!-- parent:rfc:cfdb-059-classify-split#6.3 anchor:"a snapshot delta over L1 facts" -->

The JSON wire envelope emitted by `cfdb diff` (#212) — `{a, b, schema_version, added, removed, changed, warnings}`. Carries a two-keyspace delta over the canonical sorted-JSONL dump (RFC-cfdb.md §12.1). `schema_version` is `ENVELOPE_SCHEMA_VERSION` (`"v1"`) — bumped independently of `cfdb_core::SchemaVersion` (envelope wire contract ≠ on-disk keyspace contract). Consumed by qbot-core #3736's per-PR drift gate and by `cfdb classify` (#213).

### DiffError

<!-- parent:spec:DiffEnvelope -->

Error type for `compute_diff` and `KindsFilter::from_str` — `Parse { side, line_number, source }` for bad JSON with 1-based line diagnostics, `InvalidEnvelope { side, line_number, reason }` for JSON that lacks the required canonical-dump fields, `UnknownKind { token }` for `--kinds` values other than `node`/`edge`.

### DiffFact

<!-- parent:spec:DiffEnvelope -->

One row of `DiffEnvelope::added` or `removed` — `{kind, envelope}` where `envelope` is the full canonical-dump JSON object (`{id, kind:"node", label, props}` for nodes, `{dst_qname, kind:"edge", label, props, src_qname}` for edges). `kind` is hoisted out of the envelope so consumers can filter without re-parsing.

### KindsFilter

<!-- parent:spec:DiffEnvelope -->

Filter on the `kind` discriminator for `cfdb diff --kinds`. Parsed from a comma-separated string (`node`, `edge`, `node,edge`); `FromStr` rejects unknown tokens with `DiffError::UnknownKind`. Restricts `compute_diff` to node rows, edge rows, or both — the taxonomy here is dump-line `kind` (`node`/`edge`), NOT the schema-level `ItemKind` used by `list-items-matching`.

## ParseError

The parser's error type — carries source span, expected token set, and the raw Cypher input for user-facing diagnostics.

## QueryBuilder

A fluent Rust API that constructs a `cfdb_core::Query` programmatically, as an alternative to parsing a Cypher string. Primary consumers are the verb composers (e.g. `list_items_matching`) and integration tests that need to build a query without round-tripping through source text.

## ShapeLint

A shape-lint finding emitted during parse — flags queries whose shape is likely a mistake (e.g. cartesian function-equality — the main v0.1 example). Non-fatal; surfaced to the caller as warnings rather than errors.

