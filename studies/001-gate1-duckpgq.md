# Gate 1 — DuckDB + DuckPGQ

**Candidate:** DuckDB 1.5.1 + DuckPGQ extension v1.2.2 (community extension)
**Query language:** SQL + SQL/PGQ `MATCH` inside `GRAPH_TABLE(...)` (SQL:2023 standard)
**Version evaluated:** DuckDB 1.5.1 (via duckdb-rs 1.10501.0), DuckPGQ v1.2.2
**Docs sources:**
- https://duckdb.org/docs/current/
- https://duckdb.org/docs/current/sql/query_syntax/prepared_statements.html
- https://duckdb.org/docs/current/connect/concurrency.html
- https://duckdb.org/docs/current/data/appender.html
- https://duckdb.org/docs/lts/guides/sql_features/graph_queries
- https://duckdb.org/community_extensions/extensions/duckpgq
- https://duckpgq.org/documentation/sql_pgq/
- https://duckpgq.org/documentation/property_graph/
- https://duckpgq.org/documentation/graph_functions/
- https://github.com/cwida/duckpgq-extension
- https://duckdb.org/2025/10/22/duckdb-graph-queries-duckpgq
- https://duckdb.org/science/bamberg-using-key-sigmod/ (SIGMOD 2025 "USING KEY" — see notes: this is a recursive-CTE paper, NOT a DuckPGQ optimization)
**Rust binding:** `duckdb-rs` 1.10501.0 — mature (Appender, prepared statements, `params!` macro). Runtime extension loading is confirmed for bundled extensions (`INSTALL icu; LOAD icu;` pattern). Loading community extensions like DuckPGQ from a Rust process is not explicitly documented in the duckdb-rs README; the SQL-level `INSTALL duckpgq FROM community; LOAD duckpgq;` should work via `conn.execute_batch(..)` but this is **unverified by primary docs** and must be proven in the integration spike.
**Date:** 2026-04-13

## Summary

| Axis | Score | Threshold | Verdict |
|---|---|---|---|
| Features (F1–F9) | 6.5 / 9 | ≥ 7 | FAIL |
| Schema (S1–S7) | 6.5 / 7 | ≥ 6 | PASS |
| **Gate 1** | | | **DROP** (features threshold missed; hard-blockers on F4 and F6) |

## Features

### F1 — Fixed-hop label + property match
**Score:** 1
**On-paper test query:**
```sql
FROM GRAPH_TABLE (code_graph
  MATCH (a:Item)-[]-(b:Item)
  WHERE a.name = b.name AND a.crate <> b.crate
  COLUMNS (a.qname AS a_q, b.qname AS b_q)
);
```
**Notes:** DuckPGQ documents `(a:Label WHERE a.prop = ...)` node-filter syntax and a top-level `WHERE` on vertex/edge columns. Cross-row property equality (`a.name = b.name`) is a vanilla WHERE predicate and works the same way any SQL equi-join works once the MATCH produces a binding tuple. (Source: duckpgq.org/documentation/sql_pgq/.)

### F2 — Variable-length path `[:X*1..N]`
**Score:** 1
**On-paper test query:**
```sql
FROM GRAPH_TABLE (code_graph
  MATCH (ep:EntryPoint)-[:CALLS]->{1,10}(fn:Item)
  COLUMNS (ep.qname AS ep_q, fn.qname AS fn_q)
);
```
**Notes:** DuckPGQ supports `+` (1+), `*` (0+), `{n,m}`, `{,m}`, `{n,}` quantifiers on edge elements. Directly covers the 1..N case (Source: duckpgq.org/documentation/sql_pgq/). Ordering/determinism is NOT guaranteed — see Detailed notes.

### F3 — Property regex in WHERE
**Score:** 1
**On-paper test query:**
```sql
FROM GRAPH_TABLE (code_graph
  MATCH (caller:Item)-[:CALLS]->(callee:Item)
  WHERE regexp_matches(callee.qname, 'chrono::Utc::now')
  COLUMNS (caller.qname AS src, callee.qname AS dst)
);
```
**Notes:** DuckDB ships `regexp_matches`, `~`, and `LIKE` as core scalar functions; they are usable inside DuckPGQ's MATCH WHERE because properties resolve to base-table columns of normal SQL types. No extension needed. For the forbidden-fn check a simple literal equality is sufficient anyway.

### F4 — OPTIONAL MATCH / left join
**Score:** 0
**On-paper test query:** (not expressible directly)
```sql
-- Intended:
-- MATCH (c:Item) OPTIONAL MATCH (canonical:Item)-[:CANONICAL_FOR]->(c)
```
**Notes:** **Hard blocker.** duckpgq.org/documentation/sql_pgq/ explicitly states: *"OPTIONAL MATCH is currently not supported but will be in a future update."* Workaround would be to run two separate `GRAPH_TABLE(..)` queries and `LEFT JOIN` them at the outer SQL level on the node id column — this works but forfeits SQL/PGQ's compositional matcher and pushes join logic into cfdb application code, doubling query planning surface for patterns C and G. The scoring rubric is explicit: "Possible with custom function" is 0.5. Requiring two graph scans and an outer LEFT JOIN in user code is beyond that threshold — this is not a workaround, it's "re-implement optional semantics yourself". Score 0.

### F5 — External parameter sets
**Score:** 1
**On-paper test query:**
```sql
PREPARE raid_check AS
FROM GRAPH_TABLE (code_graph
  MATCH (caller:Item)-[:CALLS]->(callee:Item)
  WHERE callee.qname IN (SELECT unnest($drops))
  COLUMNS (caller.qname AS src, callee.qname AS dst)
);
EXECUTE raid_check(drops := ['foo::bar', 'baz::quux']);
```
**Notes:** DuckDB supports named-parameter prepared statements (`$param`) with `EXECUTE ... (name := value)` assignment syntax. Lists bind through the standard DuckDB LIST parameter path and are consumed via `unnest(...)`. Since MATCH's WHERE is a normal SQL predicate, parameters flow through transparently.

### F6 — NOT EXISTS / anti-join
**Score:** 0.5
**On-paper test query:**
```sql
-- There is NO documented way to write NOT EXISTS { (i)-[:CALLS]->(fallback) }
-- inside the DuckPGQ MATCH body. Workaround pushes the anti-join to outer SQL:
WITH has_fallback AS (
  FROM GRAPH_TABLE (code_graph
    MATCH (safe:Item)-[:CALLS]->(fb:Item)
    WHERE fb.kind = 'fallback'
    COLUMNS (safe.qname AS q)
  )
)
FROM GRAPH_TABLE (code_graph
  MATCH (i:Item) WHERE i.kind = 'safe_path'
  COLUMNS (i.qname AS q)
) base
WHERE NOT EXISTS (SELECT 1 FROM has_fallback h WHERE h.q = base.q);
```
**Notes:** Pattern-level `NOT EXISTS { ... }` sub-match is not documented in any of: sql_pgq/, property_graph/, graph_functions/, the community-extension page, or the October 2025 DuckDB blog post. Relational `NOT EXISTS` at the outer SQL level is of course fully supported by DuckDB core. So the anti-join is expressible, but it requires two `GRAPH_TABLE` scans and materialization through a CTE — that is exactly the "non-obvious workaround" the rubric defines as 0.5. Patterns C, G, H all depend on this row, so the 0.5 is load-bearing.

### F7 — Aggregation + grouping
**Score:** 1
**On-paper test query:**
```sql
SELECT crate, COUNT(*) AS n
FROM GRAPH_TABLE (code_graph
  MATCH (a:Item)-[]-(b:Item)
  WHERE a.name = b.name AND a.crate <> b.crate
  COLUMNS (a.crate AS crate)
)
GROUP BY crate
HAVING COUNT(*) > 1;
```
**Notes:** DuckPGQ's `GRAPH_TABLE(..)` returns a normal DuckDB relation — any SQL aggregate, `GROUP BY`, `HAVING`, window function, etc. applies. The October 2025 DuckDB blog explicitly shows `GROUP BY ALL ... HAVING avg_amount < 50_000` composed on top of a MATCH. Clean pass.

### F8 — Parameterized queries (no string-building)
**Score:** 1
**On-paper test query:**
```rust
let mut stmt = conn.prepare(
    "FROM GRAPH_TABLE (code_graph
       MATCH (i:Item) WHERE i.qname = $qname
       COLUMNS (i.qname AS q))"
)?;
let rows = stmt.query(duckdb::params![qname])?;
```
**Notes:** DuckDB supports `?`, `$1`, and `$name` parameter styles; duckdb-rs exposes them via `prepare()` + `params!`. Since `GRAPH_TABLE(..)` is parsed as a table-producing expression, bind parameters in its WHERE clause follow the standard DuckDB prepared-statement path. No string concatenation required. Safety invariant holds.

### F9 — Multi-valued repeated edges
**Score:** 1
**On-paper test query:**
```sql
-- Edge table CallSite(src_id, dst_id, file, line) can have many rows per (src_id, dst_id):
FROM GRAPH_TABLE (code_graph
  MATCH (caller:Item)-[c:CALLS]->(callee:Item)
  COLUMNS (caller.qname AS src, callee.qname AS dst, c.file AS file, c.line AS line)
);
```
**Notes:** DuckPGQ edge tables are backed by regular DuckDB tables declared with `SOURCE KEY` / `DESTINATION KEY`. There is no uniqueness constraint on (src,dst); multiple rows with the same endpoint pair and distinct property columns are a first-class modeling choice. Each MATCH binding materializes one edge row, so distinct call sites appear as distinct result rows. Property access on the edge variable `c.file`, `c.line` is documented in property_graph/.

**Features total: 1 + 1 + 1 + 0 + 1 + 0.5 + 1 + 1 + 1 = 6.5 / 9** (below 7 threshold)

## Schema requirements

### S1 — Typed node labels
**Score:** 1
**Evidence:** `CREATE PROPERTY GRAPH ... VERTEX TABLES (Person, Account LABEL account, ...)` from duckpgq.org/documentation/property_graph/. Labels are first-class, and an optional `LABEL table IN typemask(...)` clause gives inheritance/discriminator semantics. One DuckDB table = one node label, which matches cfdb's `Item` / `Crate` / `EntryPoint` modeling exactly.

### S2 — Typed edge labels
**Score:** 1
**Evidence:** Edge tables declared with `<table> SOURCE KEY (c) REFERENCES ... DESTINATION KEY (c) REFERENCES ... LABEL <label>` (same doc). Labels are part of the CREATE PROPERTY GRAPH DDL and referenced in MATCH as `-[x:label]->`. Verified in every example on sql_pgq/.

### S3 — Node properties (string + numeric + boolean)
**Score:** 1
**Evidence:** Node properties are literally columns of a DuckDB table. DuckDB's documented type system includes VARCHAR, INTEGER/BIGINT/HUGEINT, BOOLEAN, DECIMAL, DATE, TIMESTAMP, LIST, STRUCT, UUID. `PROPERTIES (col [, col])` or `PROPERTIES ALL COLUMNS` inside VERTEX TABLES declares which columns are exposed to MATCH.

### S4 — Edge properties
**Score:** 1
**Evidence:** property_graph/ documents `PROPERTIES (...)` / `PROPERTIES ALL COLUMNS [EXCEPT (...)]` / `NO PROPERTIES` on edge tables using the same grammar as vertex tables. Edges can carry arbitrary columns (e.g., `file`, `line`, `call_kind` for a `:CALLS` edge).

### S5 — Multi-valued repeated edges
**Score:** 1
**Evidence:** Edge tables are plain DuckDB tables. There is no uniqueness constraint on (source_key, destination_key) — the `SOURCE KEY`/`DESTINATION KEY` clauses only establish referential integrity to the vertex table PKs, not a composite uniqueness on the edge row. Therefore a bag of `:CALLS` edges between the same caller→callee with distinct `(file,line)` columns is natively supported. Each row becomes an independent MATCH binding.

### S6 — Bulk insert millions of facts in seconds
**Score:** 1
**Evidence:** DuckDB Appender API is documented as the canonical bulk-ingest path, with a default auto-commit every 204,800 rows, explicit `Flush()`/`Close()` control, and first-class Rust support (listed in the API matrix alongside C/Go/Java/Julia/Node). Alternatives: `COPY ... FROM 'file.parquet'`, `INSERT INTO ... SELECT * FROM read_parquet(..)`. DuckDB's columnar storage and vectorized execution make 15k items + 80k edges ingest trivially sub-second. (Source: duckdb.org/docs/current/data/appender.html.)

### S7 — Read-only mode / snapshot isolation
**Score:** 0.5
**Evidence:** DuckDB supports `access_mode = 'READ_ONLY'` at connection time, and in that mode "multiple processes can read from the database, but no processes can write" (duckdb.org/docs/current/connect/concurrency.html). That covers the cfdb per-developer embedded read scenario cleanly. However: within a writer process, DuckDB uses MVCC + optimistic concurrency control and explicitly warns that multi-process writing is not supported and that it is optimized for bulk ops, not many small transactions. For cfdb's "extract once, query many times read-only" pattern this is fine — but "snapshot isolation against a concurrent writer" is not the model DuckDB offers (writer-and-readers is single-process only). Score 0.5 because read-only mode is fully supported but the read-side-isolation-against-a-live-writer story is weaker than LMDB/SQLite-WAL. Cfdb's offline-extract-then-query model doesn't need it, so this is not blocking.

**Schema total: 1 + 1 + 1 + 1 + 1 + 1 + 0.5 = 6.5 / 7** (above 6 threshold)

## Detailed notes

- **DuckPGQ loading model:** Community extension published on the DuckDB Community Extensions repo (since summer 2024). Load via `INSTALL duckpgq FROM community; LOAD duckpgq;` — no `unsigned` flag required since DuckDB 1.0.0. It is NOT a fork; it is a native extension binary built against a specific DuckDB ABI version, so DuckDB version and DuckPGQ version must match (v1.2.2 targets DuckDB 1.x). From Rust this translates to `conn.execute_batch("INSTALL duckpgq FROM community; LOAD duckpgq;")?` — plausible from the duckdb-rs README pattern shown for ICU but **not explicitly documented** for community extensions. This is the first thing the integration spike has to verify.
- **Version pinning risk:** Because DuckPGQ ships as ABI-matched binary, bumping DuckDB will require a coordinated DuckPGQ release. cfdb is embedded per-developer — the pinning constraint is a real supply-chain concern. Both projects are actively developed; expect 2-3 month sync lag at worst.
- **Current known limitations vs. full SQL/PGQ spec:**
  1. **OPTIONAL MATCH is not supported** (documented explicitly).
  2. Only `ANY SHORTEST` path is supported — no `ALL SHORTEST`, no deterministic shortest path, no `ANY`-without-SHORTEST.
  3. Known bug: graph-algorithm functions (PageRank, LCC, WCC) may return `csr_cte does not exist` error (acknowledged in duckdb.org/docs/lts/guides/sql_features/graph_queries). Not blocking for cfdb (we don't need PageRank).
  4. Project is self-described as "a research project and a work in progress" with "bugs, incomplete features, or unexpected behaviour".
  5. No documentation of pattern-level `NOT EXISTS { ... }`, no documentation of parameterized MATCH pattern elements (parameters flow through WHERE, not through pattern shape).
- **How SQL/PGQ treats edge labels:** Edge labels in DuckPGQ are declared at DDL time as the `LABEL` clause on an edge *table* — i.e., label = table name (or alias). This means one DuckDB table per edge label. For cfdb with ~10–15 distinct edge types (:CALLS, :CANONICAL_FOR, :HAS_IMPL, :REEXPORTS, :DEFINES, :OWNED_BY, :REFERS_TO, …) that is 10–15 edge tables — manageable but DDL is load-bearing and schema migrations are heavier than a node/edge-bag model.
- **Determinism / ordering:** SQL/PGQ does not guarantee ordering of MATCH results (same as Cypher). `ANY SHORTEST` is explicitly flagged non-deterministic. cfdb callers must add `ORDER BY` at the outer SQL level whenever result stability matters (same discipline as Postgres).
- **GRAPH_TABLE composition is the killer feature:** Once MATCH lands inside `GRAPH_TABLE(..)` it becomes a table expression, and all of DuckDB's SQL — CTEs, window functions, aggregates, joins, recursive CTEs, list/struct types, `unnest`, `regexp_matches` — composes with it. This is why F7 and F8 are straightforward 1's.
- **SIGMOD 2025 "USING KEY" paper — misattribution warning:** The paper "How DuckDB is USING KEY to Unlock Recursive Query Performance" (Bamberg, Hirn, Grust, SIGMOD '25) is about a new `WITH RECURSIVE ... USING KEY` variant of recursive CTEs that lets intermediate results be "overwritten" instead of accumulated. It benchmarks on LDBC graphs, which is presumably the source of the confusion, but it is a **core DuckDB recursive-CTE feature**, not a DuckPGQ MATCH optimization. This matters for cfdb: Pattern I (entry-point reachability) can benefit from USING KEY regardless of whether we pick DuckPGQ, because `WITH RECURSIVE` works on plain DuckDB tables. It does NOT validate DuckPGQ specifically.

## Verdict

**DROP.** DuckDB + DuckPGQ fails the feature threshold at 6.5/9, not 7. The two load-bearing losses are both structural to DuckPGQ's current research-extension status, not incidental bugs: F4 OPTIONAL MATCH is explicitly unsupported ("will be in a future update", no ETA), and F6 NOT EXISTS anti-joins have to be expressed as two separate `GRAPH_TABLE(..)` scans joined in outer SQL. Together, F4 and F6 cover cfdb patterns C (canonical-bypass), G (concept co-occurrence), and H (fallback-path missing) — three of the nine target patterns. Workaround exists (author the anti-join / optional-left-join at the outer SQL layer), but that means cfdb's query layer has to know which graph-queries need the workaround and assemble them from multiple `GRAPH_TABLE` calls, which is exactly the "stringly-typed query builder" cost the study is trying to avoid.

Schema side is strong (6.5/7): typed labels, typed edges, edge properties, multi-edges, and blazing bulk ingest via the DuckDB Appender all come for free. S7 is a soft 0.5 but doesn't block cfdb's offline-extract model. If OPTIONAL MATCH lands in DuckPGQ and a pattern-level `NOT EXISTS` sub-match is added, this candidate jumps to 8.5/9 features + 6.5/7 schema and becomes the obvious pick — it deserves a re-evaluation at the next DuckPGQ minor release. For Study 001 right now it does not clear Gate 1. Do not advance to integration spike.
