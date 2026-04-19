# Spec: cfdb-concepts

The shared bounded-context resolver — a pure-library crate (zero heavy deps) that loads `.cfdb/concepts/<name>.toml` override files and computes a crate→bounded-context mapping using a two-layer strategy (explicit TOML overrides win over crate-prefix heuristic).

Extracted from `cfdb-extractor/src/context.rs` in Issue #3 so that the future `cfdb-query` DSL evaluator can share the same loader without transitively pulling in `syn` + `cargo_metadata`. The crate is the Rust-level implementation of the Conformist pattern ratified in council-cfdb-wiring §B.1 (was qbot-core #3841).

Dependency discipline: no `syn`, no `cargo_metadata`, no `ra-ap-hir`. Pure TOML + serde. Consumed by `cfdb-extractor` today and by `cfdb-query` once `ContextMap` lands (per issue #49).

## ConceptOverrides

Loaded override map — reverse lookup from crate name to the owning `ContextMeta`. Returned by `load_concept_overrides`; consumed by `compute_bounded_context`. Internally a sorted `BTreeMap` for determinism. Provides `lookup(crate_name)` for per-crate resolution and `declared_contexts()` for enumerating every context declared across all TOML files.

## ContextMeta

The resolved context metadata for one bounded context. Carries the context `name`, an optional `canonical_crate` (the crate that holds the canonical implementation of the context's concepts), and an optional `owning_rfc` reference (doc path). Emitted as a `:Context` node during extraction; also used as the per-Crate `BELONGS_TO` edge target and as the source of `Item.bounded_context` stamping.

## LoadError

Error type produced by `load_concept_overrides` — covers filesystem access failures (`Io` with the offending path) and TOML parse errors (`Toml` with the offending path plus the boxed `toml::de::Error`). Propagated to the caller so malformed concept files fail loudly rather than silently falling back to the heuristic.
