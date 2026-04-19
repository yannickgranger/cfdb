---
crate: cfdb-petgraph
rfc: RFC-029, RFC-030
status: approved
---

# Spec: cfdb-petgraph

The `StoreBackend` implementation backed by `petgraph::StableDiGraph`. The only concrete graph store shipped with cfdb v0.1. Depends on `cfdb-core`; no other workspace dependency.

## Store

### PetgraphStore

The concrete `StoreBackend` implementor. Holds one `StableDiGraph` per keyspace, keyed by `Keyspace`. All five determinism guarantees (G1–G5 in RFC-029 §6) are implemented here.

## Persistence

### KeyspaceFile

The on-disk persistence envelope for a serialised keyspace. Wraps the JSONL canonical dump with a schema version header so the loader can detect version mismatches before touching the graph.
