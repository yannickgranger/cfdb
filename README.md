# cfdb — code facts database

A local-first, deterministic fact base for Rust workspaces. cfdb walks a Cargo workspace, extracts structural facts (crates, modules, items, fields, call sites, entry points, concepts, visibility, cfg gates) into a typed node/edge graph, and lets you query that graph with either a Cypher subset or a fluent Rust builder API.

It is a library first and a CLI second. Every verb the `cfdb` binary exposes is a function call in `cfdb-query` / `cfdb-petgraph` / `cfdb-extractor` — the binary is a wire form, not the system of record.

## What it is for

- **Architecture ban rules as declarative queries.** Replace handwritten Rust architecture tests (`test_no_utc_now_outside_tests`, `test_no_f64_in_domain`, `test_no_reqwest_client_new`) with Cypher files checked into `.cfdb/queries/`. Run them in CI; any row is a violation.
- **Canonical-bypass detection.** Identify call sites that go around a canonical resolver — reachable, unreachable, dead, and caller-scoped variants — without writing custom Rust each time.
- **Vertical split-brain detection.** Find entry points from which two divergent resolver-shaped items reach the same concept under different names.
- **Signature divergence.** Find function signatures that drift from a declared canonical shape across a workspace.
- **Unresolved and resolved call graphs.** Two extractors coexist: a fast `syn`-based name-level extractor (v0.1) and a `rust-analyzer`-HIR-backed resolver extractor (v0.2, feature-gated). Both emit into the same schema; the `:CallSite.resolver` discriminator (`"syn"` vs `"hir"`) lets queries mix or partition them.
- **Workspace inventory.** List items by name pattern, group by bounded context, enumerate entry points, describe the schema.
- **Recall gating.** `cfdb-recall` measures the extractor against `cargo public-api` / `rustdoc --output-format json` ground truth so the fact base cannot silently under-report.

If you have ever written a `walkdir` + `regex` audit script against a Rust codebase, cfdb is the structured replacement.

## The cfdb / graph-specs duo

cfdb is one half of a paired toolchain:

- **cfdb — the X-ray.** Detect existing drift in a Rust workspace. Answers "what is there, and what of it violates a rule?"
- **[graph-specs-rust](https://github.com/yannickgranger/graph-specs-rust) — the vaccine.** Block new drift at PR time against declared specs in `specs/`. Vendors cfdb as a pinned git dep and consumes its fact stream.

The two are developed in lockstep (RFC-033 cross-dogfood). cfdb's `SchemaVersion` is the wire contract between them; a bump in cfdb requires a matching fixture bump in graph-specs (see [`docs/cross-fixture-bump.md`](docs/cross-fixture-bump.md)). Either tool is useful on its own — the duo lets you combine retrospective (cfdb) and preventive (graph-specs) enforcement on the same codebase.

## Install

cfdb is not (yet) published to crates.io. Use it as a git path dep or from a checkout.

```bash
git clone https://github.com/yannickgranger/cfdb
cd cfdb
cargo build --release -p cfdb-cli
# binary is target/release/cfdb
```

Minimum supported Rust version: 1.85.

The `cfdb-hir-extractor` crate pins `ra-ap-*` crates at exact versions (see [`docs/ra-ap-upgrade-protocol.md`](docs/ra-ap-upgrade-protocol.md)). HIR support is feature-gated so `cfdb-cli`'s default build does not pull the full `rust-analyzer` compile tree.

## CLI quickstart

```bash
# 1. Extract facts from a Cargo workspace into a keyspace.
cfdb extract --workspace /path/to/your/project --db .cfdb/db --keyspace myproj

# 2. Run a Cypher ban rule. Empty output = clean.
cfdb violations --db .cfdb/db --keyspace myproj --rule examples/queries/arch-ban-utc-now.cypher

# 3. Ad-hoc query.
cfdb query --db .cfdb/db --keyspace myproj \
  'MATCH (i:Item) WHERE i.kind = "fn" AND i.is_test = false RETURN i.qname LIMIT 20'

# 4. List all callers of a symbol.
cfdb list-callers --db .cfdb/db --keyspace myproj --qname '.*::now$'

# 5. Dump sorted canonical JSONL (determinism-checkable).
cfdb dump --db .cfdb/db --keyspace myproj > facts.jsonl

# 6. Describe the schema.
cfdb schema-describe
```

The full verb surface (20 verbs across INGEST / RAW / TYPED / SNAPSHOT / SCHEMA) is documented in the `cfdb --help` output and in the module docs of `crates/cfdb-cli/src/main.rs`.

## Library quickstart

cfdb's crates are usable as a library without the CLI. Three patterns:

### A. Fluent builder → in-process evaluator

```rust
use cfdb_core::{ItemKind, StoreBackend};
use cfdb_petgraph::PetgraphStore;
use cfdb_query::builder::Query;

let mut store = PetgraphStore::new();
cfdb_extractor::extract_workspace("/path/to/proj", &mut store, "myproj")?;

let query = Query::match_node("i", "Item")
    .where_eq("i.kind", ItemKind::Fn)
    .where_eq("i.is_test", false)
    .return_prop("i", "qname")
    .limit(20)
    .build();

let result = store.execute(&query, "myproj")?;
for row in result.rows {
    println!("{}", row.get("i.qname").unwrap());
}
```

### B. Cypher parser → same AST → same evaluator

```rust
use cfdb_query::parser::parse_query;

let query = parse_query(r#"
    MATCH (i:Item)
    WHERE i.kind = "fn" AND i.is_test = false
    RETURN i.qname LIMIT 20
"#)?;

let result = store.execute(&query, "myproj")?;
```

Both surfaces produce identical `cfdb_core::Query` values — there is exactly one evaluator, which is an architectural invariant.

### C. Custom backend

`cfdb_core::StoreBackend` is a trait. `cfdb-petgraph` is the reference in-process implementation; alternative backends (SQLite, Kùzu, a remote RPC) can be written by implementing the trait. `cfdb-core` has zero dependencies on the parser, the store, the extractor, or any wire form.

## Schema overview

Facts land in a typed graph. The current schema (covered by `cfdb schema-describe`):

**Nodes:** `Crate`, `Module`, `File`, `Item` (fn / struct / enum / trait / const / type / …), `Field`, `CallSite`, `EntryPoint`, `Concept`, `BoundedContext`.

**Edges:** `IN_CRATE`, `IN_MODULE`, `HAS_FIELD`, `INVOKES_AT` (Item → CallSite), `CALLS` (resolver-emitted, HIR), `CANONICAL_FOR`, `RESOLVES_TO` (concept-level), `IMPORTS`, `CONTAINS`.

Every node/edge carries provenance (`source_file`, `line`, `resolver` where relevant) and an `is_test` flag so rules can scope to prod-only, test-only, or all.

`cfdb_core::SchemaVersion` is the wire contract. Downstream consumers (graph-specs-rust, custom backends) pin this version; breaking changes bump it in a reviewed PR.

## Example queries

See [`examples/queries/`](examples/queries/) for runnable queries, each with a header comment explaining the pattern:

| File | Pattern |
|---|---|
| `arch-ban-utc-now.cypher` | Ban rule — forbid `Utc::now()` in inner-ring prod code |
| `arch-ban-f64-in-domain.cypher` | Ban rule — forbid `f64` in domain types |
| `arch-ban-reqwest-client-new.cypher` | Ban rule — forbid direct `reqwest::Client::new()` |
| `list-callers.cypher` | Find every call site of a symbol matched by regex |
| `hsb-by-name.cypher` | Horizontal split-brain by name |
| `vertical-split-brain.cypher` | Vertical split-brain — two resolvers reachable from one entry point |
| `canonical-bypass-reachable.cypher` | Bypass rule with live user-reachable verdict |
| `canonical-bypass-caller.cypher` | Bypass rule scoped to caller regex |
| `canonical-bypass-dead.cypher` | Bypass rule with dead-code verdict |
| `canonical-unreachable.cypher` | Canonical resolver is unreachable from any entry point |
| `signature-divergent.cypher` | Function signature drifts from declared canonical shape |
| `const-table-overlap.cypher` | Const-literal tables overlap across crates — verdict ladder: `CONST_TABLE_DUPLICATE` (entries_hash equality) → `CONST_TABLE_SUBSET` (one set ⊂ other) → `CONST_TABLE_INTERSECTION_HIGH` (jaccard ≥ 0.5) |

All examples are plain text — copy, adapt parameters, run.

## Cypher subset

The parser implements a deliberate subset of openCypher:

Supported: `MATCH (var:Label)`, edge patterns with direction and labels, `WHERE` with `=`, `<>`, `<`, `<=`, `>`, `>=`, `AND`, `OR`, `NOT`, `IN`, regex `=~`, string functions (`starts_with`, `ends_with`, `contains`), `RETURN` with property projection, `ORDER BY`, `LIMIT`, `SKIP`, `WITH` pipelining, basic aggregations (`count`, `collect`), parameters (`$name`).

Not supported (v0.1): `CREATE`, `MERGE`, `DELETE`, `SET`, `CALL` procedures, variable-length path patterns `*`, shortest-path, graph mutation in general. cfdb is read-only by design — writes happen through `extract` and `enrich-*` verbs, not Cypher.

See `crates/cfdb-query/src/parser/` for the full grammar and [`studies/001-graph-store-selection.md`](studies/001-graph-store-selection.md) §8 for the subset rationale.

## User-defined functions

The evaluator exposes a small stable set of UDFs callable from Cypher — path filters, callee-name extraction, reachability tests, signature hashing. See [`docs/udfs.md`](docs/udfs.md) for the full list and semantics.

## Determinism

`cfdb extract` is byte-stable on an unchanged tree: two consecutive extracts hash-identically (`ci/determinism-check.sh`). The graph store is treated as a cache; the canonical fact format is sorted JSONL keyed by `(node_label, qname)` / `(edge_label, src_qname, dst_qname)`. This is what determinism, diffing, and cross-machine reproducibility test against — not the on-disk backend file.

## Recall

`cfdb-recall` compares the extractor's view of a crate's public API against `rustdoc --output-format json` as ground truth, reports ratios per crate, and fails CI if recall falls below threshold. This is the guard against the extractor silently missing items (macro-expanded types, re-exports, nested `pub mod`). Requires nightly for the rustdoc JSON emitter.

## Dogfood enforcement

Every PR runs cfdb against itself + against the companion at a pinned SHA. The gates:

| Gate | Tool | Question | Failure |
|---|---|---|---|
| Self-hosted ban rules | `cfdb violations` against `examples/queries/arch-ban-*.cypher` | "Does cfdb's own code use forbidden patterns?" | Any new row under a ban rule |
| Enrichment-pass postconditions | `tools/dogfood-enrich` against `.cfdb/queries/self-enrich-*.cypher` (per RFC-039) | "Did each enrichment pass write the attrs/edges its contract requires?" | Any non-zero violation row |
| ↳ `enrich-deprecation` (#343) | Source-grep `#[deprecated]` count vs `:Item.is_deprecated = true` count | "Did the extractor see every `#[deprecated]` annotation in the workspace?" | Extracted count < source-grep count → exit 30 |
| Determinism | `ci/determinism-check.sh` + `ci/dogfood-determinism.sh` | "Is `cfdb extract` byte-stable across two runs?" | sha256 / stdout mismatch |
| Cross-dogfood | `ci/cross-dogfood.sh` against graph-specs-rust at pinned SHA | "Does cfdb produce zero findings on the companion?" | Any rule row → exit 30 |
| Extractor recall | `cfdb-recall` (extractor vs `rustdoc --output-format=json`) | "Does the syn-based extractor see everything rustdoc sees?" | Recall ratio below per-crate threshold |
| No metric ratchets | Repo rule — thresholds are `const` in tool source, raised only by reviewed PR | "Does this PR add a baseline / ceiling / allowlist file?" | PR rejected on sight |

## Crates

| Crate | Role |
|---|---|
| `cfdb-core` | Node/Edge fact types, Query AST, `StoreBackend` + `EnrichBackend` traits, schema vocabulary, `SchemaVersion`. Zero deps on parser / store / extractor — the dependency rule points inward. |
| `cfdb-query` | Cypher-subset parser (chumsky) + Rust builder API. Both produce the same `cfdb_core::Query` AST. Includes shape-level lints. |
| `cfdb-petgraph` | Reference `StoreBackend` on `petgraph::StableDiGraph`. Hosts the query evaluator. |
| `cfdb-extractor` | `syn` + `cargo_metadata` workspace walker. Emits Nodes, Edges, and name-level CallSites. |
| `cfdb-hir-extractor` | `rust-analyzer` HIR-backed resolver extractor. Emits resolved `CALLS`, `INVOKES_AT`, `EntryPoint`. Feature-gated to isolate its compile cost. |
| `cfdb-hir-petgraph-adapter` | Glue between `cfdb-hir-extractor` and `cfdb-petgraph` that keeps the `ra-ap-*` crates out of `cfdb-cli`'s default build. |
| `cfdb-concepts` | Bounded-context resolver — reads `.cfdb/concepts/<context>.toml` and maps crate names to contexts. |
| `cfdb-recall` | Recall gate — extractor vs. rustdoc ground truth. |
| `cfdb-cli` | The `cfdb` binary. Thin wrapper over the library crates. |

## Layout

```
.
├── crates/           # library + CLI
├── examples/queries/ # runnable Cypher examples
├── specs/            # canonical-concept docs, one file per crate (dogfood contract)
├── docs/             # RFCs, protocols, pattern reference
├── studies/          # design spikes and backend selection
├── ci/               # determinism, recall, cross-dogfood scripts
└── tools/            # small helpers (prelude-trigger checker, etc.)
```

## `specs/` — docs that are also the contract

[`specs/concepts/`](specs/concepts/) holds one markdown file per crate (`cfdb-core.md`, `cfdb-query.md`, …). Each file is a human-readable concept dictionary: every public type, trait, and top-level function gets a `## Name` heading and a one-paragraph description of what it means and what invariants it carries. Read them first to orient — they are the shortest path to understanding cfdb's vocabulary without reading source.

They are also the **dogfood contract**. The companion [graph-specs-rust](https://github.com/yannickgranger/graph-specs-rust) tool consumes `specs/concepts/*.md` as the source of truth for what the code is supposed to contain; on every PR it diffs the specs against cfdb's own extracted fact graph and blocks any drift (a spec heading with no matching item, an item with no matching heading, a signature change not reflected in spec). cfdb's own codebase is the first customer of this discipline — the specs are both its documentation and the proof that the toolchain works on its authors' own tree before being pointed at anyone else's.

Adding a new public type to a cfdb crate requires adding its spec heading in the same PR. That is how the specs stay honest.

## Status

Under active development. v0.1 (syn extractor + petgraph store + Cypher subset + 10+ example queries + recall gate) is feature-complete on `develop`. v0.2 (HIR extractor, enrichment verbs, concept resolution) lands incrementally — see the `RFC-cfdb.md` in `docs/` for the roadmap.

The wire schema (`SchemaVersion`) is versioned. Breaking changes bump it and are called out in release notes; non-breaking additions are documented in `SchemaDescribe` output.

## Documentation

- Core RFC: [`docs/RFC-cfdb.md`](docs/RFC-cfdb.md) — rationale, schema, 9 motivating patterns.
- v0.2 addendum: [`docs/RFC-cfdb.md`](docs/RFC-cfdb.md) — HIR extractor, enrichment pipeline.
- Anti-drift gate: [`docs/RFC-030-anti-drift-gate.md`](docs/RFC-030-anti-drift-gate.md) — how cfdb integrates into PR-time gating.
- Cross-dogfood with graph-specs: [`docs/RFC-033-cross-dogfood.md`](docs/RFC-033-cross-dogfood.md), runbook in [`docs/cross-fixture-bump.md`](docs/cross-fixture-bump.md).
- Pattern references: [`docs/cfdb-pattern-b.md`](docs/cfdb-pattern-b.md), [`docs/cfdb-pattern-c.md`](docs/cfdb-pattern-c.md).
- UDF reference: [`docs/udfs.md`](docs/udfs.md).
- `ra-ap-*` upgrade protocol: [`docs/ra-ap-upgrade-protocol.md`](docs/ra-ap-upgrade-protocol.md).

## Contributing

New capabilities (new verb, new fact type, new schema field, new `--flag`, new sub-backend) are RFC-first — see `CLAUDE.md` §2. Bug fixes and mechanical refactors go straight to an issue + PR. Every PR passes the dogfood gate: cfdb's own ban rules run against cfdb itself, and cross-dogfood runs cfdb against graph-specs-rust at a pinned SHA. No metric ratchets, no baseline files — violations are fixed, not accumulated.

## License

Dual-licensed under MIT or Apache-2.0, at your option.
