# cfdb — code facts database

Rust in-tree sub-workspace. **Not part of the main qbot-core workspace.**
Build from `.concept-graph/cfdb/`:

```bash
cd .concept-graph/cfdb
cargo build
cargo test
```

## Crates

| Crate | Purpose | Status |
|---|---|---|
| `cfdb-core` | Node/Edge fact types, Query AST, StoreBackend trait, schema vocabulary (RFC §7) | v0.0.1 (scaffold) |
| `cfdb-query` | Cypher-subset parser (chumsky) + Rust builder API — both produce the same `Query` AST | stub — #3642 |
| `cfdb-petgraph` | StoreBackend impl on `petgraph::StableDiGraph` | stub — #3643 |
| `cfdb-extractor` | Rust workspace → facts via `syn` + `cargo_metadata` | stub — follow-up |
| `cfdb-cli` | `cfdb` binary — wraps the above for the CLI wire form | stub |

## References

- RFC: `.concept-graph/RFC-cfdb.md`
- Study 001 (backend selection): `.concept-graph/studies/001-graph-store-selection.md`
- Gate 3 spike (validated evaluator shape): `.concept-graph/studies/spike/petgraph/`
- Epic: #3622
