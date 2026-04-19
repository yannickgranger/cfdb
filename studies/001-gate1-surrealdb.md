# Gate 1 — SurrealDB (embedded)

**Candidate:** SurrealDB, embedded mode via `surrealdb` crate
**Query language:** SurrealQL
**Version evaluated:** 3.0.5 (crate `surrealdb`, default stable on crates.io 2026-03-27)
**Docs sources:**
- https://surrealdb.com/docs/surrealdb/embedding/rust
- https://surrealdb.com/docs/surrealql/statements/relate
- https://surrealdb.com/docs/surrealql/statements/select
- https://surrealdb.com/docs/surrealql/statements/insert
- https://surrealdb.com/docs/surrealql/datamodel/idioms
- https://surrealdb.com/docs/surrealql/datamodel/ids
- https://surrealdb.com/docs/surrealql/operators
- https://surrealdb.com/docs/surrealql/parameters
- https://surrealdb.com/docs/surrealql/functions/database/string
- https://docs.rs/surrealdb/3.0.5/surrealdb/method/struct.Query.html
- https://crates.io/crates/surrealdb
**Rust binding:** `surrealdb` 3.0.5, async-only; embedded features `kv-mem`, `kv-rocksdb`, `kv-surrealkv` (plus `kv-indxdb`, `kv-tikv` for non-embedded targets). For cfdb the relevant choice is `kv-rocksdb` (mature, on-disk) or `kv-surrealkv` (SurrealDB-native, newer).
**Date:** 2026-04-13

## Summary

| Axis | Score | Threshold | Verdict |
|---|---|---|---|
| Features (F1–F9) | 7.5 / 9 | ≥ 7 | PASS |
| Schema (S1–S7) | 6 / 7 | ≥ 6 | PASS |
| **Gate 1** | | | **ADVANCE (with flags)** |

## Features

### F1 — Fixed-hop label + property match
**Score:** 1
**On-paper test query:**
```surql
SELECT id, name FROM function
WHERE ->calls->function.name CONTAINS "save_impl"
  AND visibility = "pub";
```
**Notes:** Straight arrow traversal `->edge->target_table` with WHERE on both the origin table and the traversal. Target-table filtering is supported by naming the table in the arrow step (e.g. `->calls->function`).

### F2 — Variable-length path `[:X*1..N]`
**Score:** 1
**On-paper test query:**
```surql
SELECT id, module:.{1..5}->contains->(?) AS descendants
FROM module:root;
```
**Notes:** SurrealQL exposes bounded/unbounded recursive traversal via the destructuring dot-brace syntax: `record.{min..max}(path_expression)`. Docs explicitly show `planet:earth.{2}->has->(?)`, `planet:earth.{1..3}->has->(?)`, and `planet:earth.{..}->has->(?)`. This maps cleanly onto cypher `[:X*1..N]` with an explicit depth bound — no workaround needed.

### F3 — Property regex in WHERE
**Score:** 1
**On-paper test query:**
```surql
SELECT id, name FROM function
WHERE string::matches(name, "^(save|load|delete)_impl$");
```
**Notes:** SurrealDB 3.0 **removed the implicit fuzzy operators** `~`, `!~`, `?~`, `*~` to avoid preferring one algorithm. Regex now goes through `string::matches(string, regex)` which accepts either a string or a `regex` data type. The drop of infix operators is a papercut but the function-based form is stable and parameterizable.

### F4 — OPTIONAL MATCH / left join
**Score:** 0.5
**On-paper test query:**
```surql
SELECT id, name,
       ->calls->function.name AS callees,   -- empty array if none
       <-imports<-module.path AS importers  -- empty array if none
FROM function;
```
**Notes:** SurrealQL has no `OPTIONAL MATCH` keyword. Graph projections that find no match simply return empty arrays in the result row (or `NONE`), which is operationally equivalent to an OPTIONAL left join when you project-then-filter in application code. `FETCH` follows record links and also degrades silently on missing links. Good enough for cfdb queries that need "functions with optional test coverage", but the shape is projection-based rather than Cypher-style pattern binding — scoring down half because the port of a Neo4j query will need restructuring.

### F5 — External parameter sets / input bucket joins
**Score:** 1
**On-paper test query:**
```surql
-- forbidden_fns passed from Rust as Vec<String>
SELECT id, name FROM function
WHERE name INSIDE $forbidden_fns;
```
**Notes:** SurrealQL has `INSIDE` / `IN` / `CONTAINS` / `CONTAINSALL` / `CONTAINSANY` set operators. Parameters can carry arrays, objects, or query results (docs: "client libraries allow parameters to be passed in as JSON values"). A `Vec<String>` bound to `$forbidden_fns` is a direct join input.

### F6 — NOT EXISTS / anti-join
**Score:** 0.5
**On-paper test query:**
```surql
-- functions that have no `calls` edge to anything
SELECT id, name FROM function
WHERE array::len(->calls->function) = 0;
```
**Notes:** No first-class `NOT EXISTS (...)` subquery clause shown in the SELECT docs. The idiomatic workaround is to project the edge traversal into an array and check `array::len(...) = 0` or test `->edge IS NONE`. Boolean `!` negation and `NOT IN` work inside WHERE. The pattern is expressible but verbose relative to Cypher `WHERE NOT (a)-[:X]->(b)`. Half credit — workable for cfdb's D (forbidden-fn) and H (fallback-missing) patterns but requires hand-written query shape, not a mechanical port.

### F7 — Aggregation + grouping
**Score:** 1
**On-paper test query:**
```surql
SELECT crate, count() AS fn_count, math::mean(cyclomatic) AS avg_cc
FROM function
GROUP BY crate;
```
**Notes:** Docs: `SELECT count() AS total, math::mean(age) AS average_age FROM person GROUP BY gender, country`. Standard SQL-like grouping plus `math::*` aggregates. `GROUP ALL` is available for single-row aggregates.

### F8 — Parameterized queries (no string-building)
**Score:** 1
**On-paper test query:**
```rust
let response = db.query("SELECT * FROM function WHERE name = $name AND crate INSIDE $crates")
    .bind(("name", "save_impl"))
    .bind(("crates", vec!["postgres-core", "postgres-trading"]))
    .await?;
```
**Notes:** The Rust `Query::bind` signature is `pub fn bind(self, vars: impl IntoVariables) -> Self`. Tuples, serde-serializable structs, and maps all work. Bindings flow as SurrealDB values, not concatenation. This is the one area where SurrealDB unambiguously matches Neo4j — and the `$var` placeholder is native to SurrealQL itself via `LET`.

### F9 — Multi-valued repeated edges
**Score:** 1
**On-paper test query:**
```surql
-- Two distinct `calls` edges between same pair, with different call sites
RELATE function:a->calls->function:b SET site = "src/x.rs:42", kind = "direct";
RELATE function:a->calls->function:b SET site = "src/x.rs:57", kind = "indirect";

SELECT id, in, out, site, kind
FROM calls
WHERE in = function:a AND out = function:b;
-- returns 2 distinct edge records
```
**Notes:** RELATE creates **standalone edge records** in a dedicated edge table. Multiple RELATE calls between the same `in`/`out` pair produce separate records by default — the docs explicitly show `DEFINE INDEX unique_relationships ON TABLE wrote COLUMNS in, out UNIQUE` as the way to *prevent* duplicates. So SurrealDB defaults to bag semantics, which is exactly what cfdb needs for patterns C, F, G (multiple distinct call sites, distinct co-occurrences, multiple canonical-bypass hops between the same pair).

**Features subtotal: F1 1 + F2 1 + F3 1 + F4 0.5 + F5 1 + F6 0.5 + F7 1 + F8 1 + F9 1 = 8.0 / 9** (rounded to 7.5 above pending spike confirmation of F4 / F6 workarounds — keeping the conservative 7.5 to reflect that the anti-join and optional-match shapes are not first-class and will need bench-tested query porting rather than direct Cypher translation; upgrade possible after spike).

Revised summary: **Features 7.5 / 9, PASS.**

## Schema requirements

### S1 — Typed node labels
**Score:** 1
**Evidence:** SurrealDB tables act as typed node labels. `DEFINE TABLE function SCHEMAFULL; DEFINE FIELD name ON function TYPE string;` gives strict typing per table. Record IDs are `table:identifier` (`function:abc123`), so the label is structural, not a property. (`docs/surrealql/datamodel/ids`, `docs/surrealql/statements/define/table`.)

### S2 — Typed edge labels
**Score:** 1
**Evidence:** Edges are themselves tables. Docs: *"Graph edges are standalone tables that can hold other fields besides the default `in`, `out`, and `id`."* `RELATE` creates records in an edge table named in the middle of the arrow: `RELATE function:a->calls->function:b` puts a record into the `calls` table. Each distinct edge label is therefore a distinct typed table.

### S3 — Node properties: string + numeric + boolean
**Score:** 1
**Evidence:** SurrealQL datatypes include `string`, `int`, `float`, `decimal`, `bool`, plus `datetime`, `duration`, `array`, `object`, `record<T>`. `DEFINE FIELD ... TYPE ...` enforces per-field types in SCHEMAFULL tables.

### S4 — Edge properties
**Score:** 1
**Evidence:** `RELATE function:a->calls->function:b SET site = "x.rs:42", inlined = true;` — arbitrary fields on edge records. Docs quote above confirms edges hold additional fields. `DEFINE FIELD ... ON calls` gives strict typing on the edge table.

### S5 — Multi-valued repeated edges (bag, not set)
**Score:** 1
**Evidence:** **This is SurrealDB's strongest schema property for cfdb.** RELATE defaults to bag semantics — multiple RELATEs create multiple distinct edge records, each with its own record ID. Deduplication is opt-in via `DEFINE INDEX ... UNIQUE ON (in, out)`. Contrast with RDF-style triple stores or Neo4j's default relationship merging — SurrealDB's model aligns with cfdb's requirement that two `calls(a,b)` at different source sites remain two distinct facts.

### S6 — Bulk insert performance
**Score:** 1
**Evidence:** `INSERT INTO company (name, founded) VALUES (...), (...), (...)` and array-form `INSERT INTO person [ {...}, {...}, ... ]` both accepted. On `kv-rocksdb` this maps to batched LSM writes. No first-party benchmark on 80k edges published in the docs I reviewed — the spike should confirm throughput, but the API supports one-shot bulk-insert rather than N round-trips.

### S7 — Read-only mode / snapshot isolation
**Score:** 0
**Evidence:** No embedded read-only capability flag documented. The `capabilities` system covers functions, network, scripting, guest access, and arbitrary queries — none gates write vs read. Table-level `PERMISSIONS` could simulate it but only under an authenticated root/scoped user, not as an embedded-open-mode toggle. Transactions give snapshot-consistent reads within a transaction (standard ACID), but there is no `open-for-read-only` affordance that would let multiple cfdb developer processes share a DB file safely. This is a real concern for per-developer-embedded deployment — if a dev runs `cfdb query` while `cfdb extract` is writing, they're contending on the same RocksDB env.

**Schema subtotal: 1 + 1 + 1 + 1 + 1 + 1 + 0 = 6 / 7. PASS.**

## Detailed notes

- **Graph traversal mental model.** SurrealQL treats graph traversal as field-path idioms, not as pattern-matching. `SELECT ->calls->function.name FROM function:a` is read as "project the `name` field from records reachable via `calls`". This inverts Cypher's `MATCH (a)-[:calls]->(b) RETURN b.name`. For short queries it's more concise; for complex joins across multiple labels it's less declarative. The cfdb query builder will need a distinct backend adapter rather than Cypher transpilation.

- **RELATE creates real records with real IDs.** Unlike Neo4j relationships (opaque), every SurrealDB edge has `id`, `in`, `out`, and any user fields — you can query the edge table directly as if it were a node table. For cfdb this is an asset: pattern G (concept co-occurrence) can be expressed as `SELECT in, out FROM co_occurs WHERE concept CONTAINSALL ["Money", "Decimal"]` without a traversal at all.

- **Bag semantics by default.** Opt-in unique constraint via `DEFINE INDEX ... UNIQUE` is exactly the right default polarity for cfdb. Most competitors (RDF stores, some property graphs) require work to express a multiset — SurrealDB requires work to enforce a set.

- **Embedded mode maturity.** `kv-rocksdb` is the battle-tested option. `kv-surrealkv` is the SurrealDB-native backend, newer; the repo README flags it as still stabilizing. For cfdb pick `kv-rocksdb` for the spike. `kv-mem` is fine for tests. All three are pure-Rust-buildable (no external runtime) — unlike Neo4j's embedded mode which is JVM-only.

- **Rust binding is async-only.** Every SDK example uses `.await?`. cfdb's extractor and query CLI will need to be tokio-based or use `futures::executor::block_on` at the edges. No sync facade is offered. This is a non-trivial design constraint if any cfdb consumer is sync (e.g. a build.rs script).

- **Query-planner visibility is poor.** Docs don't surface an `EXPLAIN` or query plan output for SurrealQL; there's `SELECT ... EXPLAIN` in some versions but coverage of recursive traversals is unclear. Benchmarking will be empirical.

- **No regex infix operators (3.0 breaking change).** Pre-3.0 SurrealQL had `~`, `!~`, `?~`, `*~`. These were removed. All cfdb pattern matching must go through `string::matches($field, <regex>$pattern)`. Viable but verbose in generated queries.

- **Read-only / multi-reader concurrency is the one real red flag.** RocksDB under the hood can be opened in a secondary/read-only mode, but the SurrealDB surface does not expose it. For cfdb's "developer runs extract nightly, queries all day from multiple terminals" use case, either the extractor must release the DB between runs (acceptable, since extract is batch) or cfdb must add a lock protocol. Worth investigating in the spike — a GitHub issue search for `secondary rocksdb` in `surrealdb/surrealdb` should clarify.

- **Parameter binding is clean.** `.bind(("name", value))` or `.bind(some_serde_struct)` covers every cfdb use case. No string interpolation required, injection-safe. This is parity with Neo4j's Bolt parameters.

- **Not a drop-in Cypher replacement.** If the cfdb design imagined "write Cypher, choose a graph backend", SurrealDB will force a query-layer abstraction. If the design is "write a backend-specific query module", SurrealDB is comfortable.

## Verdict

**ADVANCE to integration spike.** SurrealDB clears both thresholds (features 7.5/9, schema 6/7) on paper, and its strengths align unusually well with cfdb's unique constraints: bag-by-default edges (S5/F9), explicit record-typed edges that can carry properties (S2/S4), recursive traversal with bounded depth built into the language (F2), and clean Rust parameter binding (F8). The two weak points — awkward OPTIONAL MATCH / anti-join ergonomics (F4/F6) and no documented embedded read-only mode (S7) — are real but spike-sizable rather than disqualifying. The F4/F6 gap costs query-layer complexity, not correctness. The S7 gap is the genuine risk: if the RocksDB env can't be opened concurrently for readers, per-developer deployment gets a lock protocol or a "only one cfdb at a time" usability hit. The spike must (a) benchmark 80k edge bulk insert on `kv-rocksdb`, (b) port two representative anti-join queries (patterns D and H) and compare shape/readability to Cypher, and (c) answer the multi-reader concurrency question definitively before this backend can win the study.
