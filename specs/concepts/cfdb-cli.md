---
crate: cfdb-cli
rfc: RFC-029, RFC-030
status: approved
---

# Spec: cfdb-cli

The `cfdb` binary — entry point for all 16 API verbs. Wires `cfdb-extractor`, `cfdb-petgraph`, `cfdb-query`, and `cfdb-core` into a cohesive CLI. Depends on all four library crates; nothing in the workspace depends on `cfdb-cli`.

This is a binary crate with no public Rust API surface (no downstream Rust consumer). Its "public surface" is the CLI contract: the 16 verbs, their flags, and their exit-code semantics. The spec covers concept ownership and the entry-point contract.

## Entry point

### main (binary entry point)

The `cfdb` binary. Command dispatch via `clap`. Hands off to handler functions in `commands`, `enrich`, `scope`, and `stubs` modules.

## Verb handlers (commands)

The command handlers own the 16 API verbs: `extract`, `query`, `list-callers`, `violations`, `dump`, `list-keyspaces`, `export`, `typed-stub`, `list-items-matching`, `snapshots`, `diff`, `drop-keyspace`, `schema-describe`, `scope`, `enrich`, and any future verbs.

Note (RFC-031 §4): the composition concern (store instantiation, persistence wiring) is currently scattered across handler modules. RFC-031 prescribes introducing a `compose.rs` module as a single construction path. This spec reflects current state; the `compose.rs` concept will be added in the same PR as that change.

## Enrichment

### EnrichVerb

The set of enrichment verbs exposed by the `enrich` sub-command: `Docs`, `Metrics`, `History`, `Concepts`. Maps directly to the four `enrich_*` methods on `StoreBackend`.
