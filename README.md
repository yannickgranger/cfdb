# cfdb — code facts database

Standalone Rust workspace. Extracted from `qbot-core/.concept-graph/cfdb/`.

```bash
cargo build
cargo test
```

## Crates

| Crate | Purpose | Status |
|---|---|---|
| `cfdb-core` | Node/Edge fact types, Query AST, StoreBackend trait, schema vocabulary (RFC §7) | v0.0.1 (scaffold) |
| `cfdb-query` | Cypher-subset parser (chumsky) + Rust builder API — both produce the same `Query` AST | stub |
| `cfdb-petgraph` | StoreBackend impl on `petgraph::StableDiGraph` | stub |
| `cfdb-extractor` | Rust workspace → facts via `syn` + `cargo_metadata` | stub |
| `cfdb-recall` | Recall gate: extractor vs. `rustdoc --output-format=json` ground truth | v0.1 (dogfooded on `cfdb-core`) |
| `cfdb-cli` | `cfdb` binary — wraps the above for the CLI wire form | stub |

## References

- RFC: `docs/RFC-cfdb.md`
- RFC v0.2 addendum (draft): `docs/RFC-cfdb-v0.2-addendum-draft.md`
- Plan: `docs/PLAN-v1-code-facts-database.md`
- Study 001 (backend selection): `studies/001-graph-store-selection.md`
- Gate 3 spike (validated evaluator shape): `studies/spike/petgraph/`
