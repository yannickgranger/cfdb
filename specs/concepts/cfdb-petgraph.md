# Spec: cfdb-petgraph

The `StoreBackend` implementation backed by `petgraph::StableDiGraph`. The only concrete graph store shipped with cfdb v0.1. Depends on `cfdb-core`; no other workspace dependency.

## PetgraphStore

The concrete `StoreBackend` implementor. Holds one `StableDiGraph` per keyspace, keyed by `Keyspace`. The five determinism guarantees (G1–G5 in RFC-029 §6) are implemented here.

## KeyspaceFile

The on-disk persistence envelope for a serialised keyspace. Wraps the canonical JSON dump with a schema-version header so the loader can detect version mismatches before touching the graph.
