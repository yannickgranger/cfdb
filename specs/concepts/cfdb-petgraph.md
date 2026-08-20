# Spec: cfdb-petgraph

The `StoreBackend` + `GraphBackend` implementation backed by `petgraph::StableDiGraph`. The only concrete graph store shipped with cfdb v0.1. Depends on `cfdb-core`; no other workspace dependency. Storage only: query evaluation is `cfdb-eval`'s `QueryEngine`, which reads a keyspace through the `GraphReader` port — this crate never links the evaluator (pinned by `tests/architecture_dep_rule.rs`).

## PetgraphStore

The concrete `StoreBackend` implementor. Holds one `StableDiGraph` per keyspace, keyed by `Keyspace`. The five determinism guarantees (G1–G5 in RFC-029 §6) are implemented here. Also the sole `GraphBackend` implementor: each keyspace state implements `GraphView` (read/write, id-based) and `GraphReader` (read-only, handle-based — handles wrap the `u32` petgraph indices 1:1, so handle order is index order).

## KeyspaceFile

The on-disk persistence envelope for a serialised keyspace. Wraps the canonical JSON dump with a schema-version header so the loader can detect version mismatches before touching the graph, plus the identity-contention diagnostics (RFC-054 54-A; capped, contention-kind only — the per-edge ingest log stays process-local). See `KeyspaceFile::contention_warnings` for the compat contract.

## IndexSpec

<!-- parent:rfc:cfdb-035-persistent-inverted-indexes#3.1 anchor:"backend-optimisation artefacts with no stable abstract meaning" -->

Parsed `.cfdb/indexes.toml` — the set of `(Label, prop)` or `(Label, computed-key)` pairs that the build pass (RFC-035 slice 2) should materialise into an inverted posting-list index at ingest time. Owned by `cfdb-petgraph` per RFC-035 R1 B1 — backend-optimisation artefact, not a stable abstraction over `cfdb-core`. Missing `.cfdb/indexes.toml` yields an empty spec (no error).

## IndexEntry

<!-- parent:rfc:cfdb-035-persistent-inverted-indexes#3.2 anchor:"The required `notes` string on each entry documents the rationale" -->

A single `[[index]]` TOML row. Two shapes — plain prop (`label` + `prop` + `notes`) or computed key (`label` + `computed` + `notes`). The `notes` string is required and documents the rationale per RFC-035 R1 R2; an entry missing it is rejected at parse time.

## ComputedKey

<!-- parent:rfc:cfdb-035-persistent-inverted-indexes#3.3 anchor:"wrappers around canonical qname-formula functions" -->

The closed `const`-sized allowlist of pure functions that may be used as a computed index key. v0.1 ships only `LastSegment` (`last_segment(qname)`). Each variant wraps a canonical `cfdb-core::qname::*` helper (RFC-035 §3.3); extending the allowlist is an RFC-gated change per RFC-035 §3.4.

## UnknownComputedKey

Error raised when an `indexes.toml` `computed = "…"` string is not in the `ComputedKey` allowlist. Carries the offending string verbatim so the parse error can name the rejected key.

## IndexSpecLoadError

Error returned by `IndexSpec::from_path` and `IndexSpec::from_toml_str`. Distinguishes filesystem errors (`Io`) from TOML parse failures (`Toml`) including missing required fields, both-set `prop`+`computed`, and unknown computed keys.

