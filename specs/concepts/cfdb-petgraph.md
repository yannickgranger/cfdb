# Spec: cfdb-petgraph

The `StoreBackend` implementation backed by `petgraph::StableDiGraph`. The only concrete graph store shipped with cfdb v0.1. Depends on `cfdb-core`; no other workspace dependency.

## PetgraphStore

The concrete `StoreBackend` implementor. Holds one `StableDiGraph` per keyspace, keyed by `Keyspace`. The five determinism guarantees (G1–G5 in RFC-029 §6) are implemented here.

## KeyspaceFile

The on-disk persistence envelope for a serialised keyspace. Wraps the canonical JSON dump with a schema-version header so the loader can detect version mismatches before touching the graph.

## IndexSpec

Parsed `.cfdb/indexes.toml` — the set of `(Label, prop)` or `(Label, computed-key)` pairs that the build pass (RFC-035 slice 2) should materialise into an inverted posting-list index at ingest time. Owned by `cfdb-petgraph` per RFC-035 R1 B1 — backend-optimisation artefact, not a stable abstraction over `cfdb-core`. Missing `.cfdb/indexes.toml` yields an empty spec (no error).

## IndexEntry

A single `[[index]]` TOML row. Two shapes — plain prop (`label` + `prop` + `notes`) or computed key (`label` + `computed` + `notes`). The `notes` string is required and documents the rationale per RFC-035 R1 R2; an entry missing it is rejected at parse time.

## ComputedKey

The closed `const`-sized allowlist of pure functions that may be used as a computed index key. v0.1 ships only `LastSegment` (`last_segment(qname)`). Each variant wraps a canonical `cfdb-core::qname::*` helper (RFC-035 §3.3); extending the allowlist is an RFC-gated change per RFC-035 §3.4.

## UnknownComputedKey

Error raised when an `indexes.toml` `computed = "…"` string is not in the `ComputedKey` allowlist. Carries the offending string verbatim so the parse error can name the rejected key.

## IndexSpecLoadError

Error returned by `IndexSpec::from_path` and `IndexSpec::from_toml_str`. Distinguishes filesystem errors (`Io`) from TOML parse failures (`Toml`) including missing required fields, both-set `prop`+`computed`, and unknown computed keys.

## ExplainRow

One observability row emitted by `PetgraphStore::execute_explained` (RFC-035 slice 7 / #186). Carries the rendered `(var:Label)` pattern string and a `hit: ExplainHit` tag naming whether the evaluator's `candidate_nodes` invocation was satisfied through the `by_prop` fast path or fell back to a full label scan. Stable side-band from `QueryResult` — no explain rows leak into the canonical dump or the keyspace wire format, preserving the RFC-035 §4 determinism invariant. The renderer (`format_line`) is the stable contract consumed by `cfdb scope --explain` dogfood tests.

## ExplainHit

The closed two-variant enum tagging one `ExplainRow`. `Indexed` means the slice-5/6 `by_prop` fast path fired; `Fallback` means the evaluator used `nodes_with_label` (or `all_nodes_sorted` for label-less patterns). Dogfood tests grep on the arrow-form rendering (`→ indexed` / `→ fallback`) so both variants are load-bearing test primitives for self-dogfood + target-dogfood hit-rate measurements.
