---
crate: cfdb-extractor
rfc: RFC-029, RFC-030
status: approved
---

# Spec: cfdb-extractor

Rust workspace → fact graph, using `syn` for AST traversal and `cargo_metadata` for workspace topology. Produces `Node` and `Edge` values for ingestion into a `StoreBackend`. Depends on `cfdb-core`; no other workspace dependency.

## Extractor

### ExtractError

The error type produced by the extraction pipeline. Covers `cargo_metadata` failures, `syn` parse failures, I/O errors on workspace files, and configuration errors.
