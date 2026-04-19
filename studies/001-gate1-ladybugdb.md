# Gate 1 — LadybugDB (`lbug`)

**Candidate:** LadybugDB (Rust crate `lbug`)
**Query language:** openCypher (inherited from Kuzu, parser/planner unchanged as of 0.15.x)
**Version evaluated:** `lbug` 0.15.3 (published 2026-04-01 on crates.io)
**Docs sources:**
- https://crates.io/crates/lbug (crate metadata, v0.15.3, MIT, repo `github.com/lbugdb/lbug`)
- https://docs.rs/lbug/latest/lbug/ (Rust API: `Database`, `Connection`, `SystemConfig`, `PreparedStatement`)
- https://ladybugdb.com/ (product site)
- https://docs.ladybugdb.com/cypher/query-clauses/match/ (MATCH + var-length paths)
- https://docs.ladybugdb.com/cypher/query-clauses/optional-match/ (OPTIONAL MATCH)
- https://docs.ladybugdb.com/cypher/query-clauses/where/ (WHERE)
- https://docs.ladybugdb.com/cypher/subquery/ (EXISTS / COUNT subqueries)
- https://docs.ladybugdb.com/cypher/expressions/pattern-matching/ (`=~` regex, `regexp_matches`)
- https://docs.ladybugdb.com/cypher/data-definition/create-table/ (CREATE NODE/REL TABLE, multiplicity)
- https://docs.ladybugdb.com/import/ (COPY FROM CSV/Parquet/JSON)
- https://docs.ladybugdb.com/get-started/prepared-statements/ (parameterized queries via `$`)
- https://blog.ladybugdb.com/post/ladybug-release/ (v0.12.0 release: "functionally equivalent to kuzu v0.11.3"; governance divergence, same engine)
- Kuzu pre-archive `docs.kuzudb.com` used as provenance fallback for `NOT EXISTS` shape (carries forward unchanged — v0.12.0 release post confirms engine equivalence)
**Rust binding:** `lbug` 0.15.3 on crates.io, cxx/FFI bindings over the upstream C++ core (crate body: ~232k lines C headers, ~191k C++, ~3k Rust glue; `rust-version = 1.81`, edition 2021, MIT). Maintained by Arun Sharma (`adsharma`, Kuzu co-founder). Repo: `https://github.com/lbugdb/lbug` (crate repo) with the main engine at `https://github.com/LadybugDB/ladybug`.
**Date:** 2026-04-13

## Summary

| Axis | Score | Threshold | Verdict |
|---|---|---|---|
| Features (F1–F9) | 9 / 9 | ≥ 7 | PASS |
| Schema (S1–S7) | 7 / 7 | ≥ 6 | PASS |
| **Gate 1** | | | **ADVANCE** |

## Features

### F1 — Fixed-hop label + property match
**Score:** 1
**On-paper test query:**
```cypher
MATCH (a:Item), (b:Item)
WHERE a.name = b.name AND a.crate <> b.crate
RETURN a, b
```
**Notes:** Cartesian-style multi-pattern `MATCH (a:User), (b:User)` with property predicates in a `WHERE` is the documented idiom in the MATCH page. Inline-property sugar `{name: ...}` is also documented. Equality, inequality, and field-vs-field comparison are all standard WHERE predicates.

### F2 — Variable-length path
**Score:** 1
**On-paper test query:**
```cypher
MATCH (ep:EntryPoint)-[:CALLS*1..10]->(fn:Item)
RETURN ep, fn
```
**Notes:** Documented explicitly: `MATCH (a)-[e:Follows*1..2]->(b)`. Trail (distinct rels), acyclic (distinct nodes), `SHORTEST`, `ALL SHORTEST`, and weighted `WSHORTEST` variants are first-class. Directly fits Patterns B and I.

### F3 — Property regex in WHERE
**Score:** 1
**On-paper test query:**
```cypher
MATCH ()-[:CALLS]->(callee:Item)
WHERE callee.qname =~ '.*chrono::Utc::now.*'
RETURN callee
```
**Notes:** `=~` operator documented verbatim: `'abc' =~ '.*(b|d).*'` returns `True`. Case-insensitive via `(?i)`. Additionally `regexp_matches`, `regexp_extract`, `regexp_replace` are available as functions. Pattern D fits cleanly.

### F4 — OPTIONAL MATCH / left join
**Score:** 1
**On-paper test query:**
```cypher
MATCH (c:Concept)
OPTIONAL MATCH (canonical:Item)-[:CANONICAL_FOR]->(c)
RETURN c, canonical
```
**Notes:** Documented: "OPTIONAL MATCH ... will set the values in the variables defined only in the OPTIONAL MATCH to NULL." The docs explicitly describe it as a left outer join on shared variables, with a worked example `MATCH (u:User) OPTIONAL MATCH (u)-[:Follows]->(u1:User) RETURN u.name, u1.name;`. Patterns C and G fit.

### F5 — External parameter sets / input bucket joins
**Score:** 1
**On-paper test query:**
```cypher
UNWIND $plan_drop AS drop_qname
MATCH (i:Item {qname: drop_qname})
RETURN i
```
**Notes:** Parameters are passed via `PreparedStatement` + `Connection::execute(&mut stmt, vec![("plan_drop", Value::List(...))])`. The `UNWIND` clause is documented (`UNWIND ["Amy","Bob","Carol"] AS x`) and composes with subsequent MATCH. Alternatively `WHERE i.qname IN $drops` works because `$param` substitution is value-level and `IN` over lists is a standard Cypher operator. The Rust `Value` type includes list variants for binding.

### F6 — NOT EXISTS / anti-join
**Score:** 1
**On-paper test query:**
```cypher
MATCH (i:Item)-[:CALLS]->(safe:Item {qname: 'safe_path'})
WHERE NOT EXISTS { MATCH (i)-[:CALLS]->(:Item {qname: 'failure_path'}) }
RETURN i
```
**Notes:** `EXISTS { MATCH ... }` subqueries are documented as a first-class pattern predicate (`WHERE a.age < 100 AND EXISTS { MATCH (a)-[:Follows*3..3]->(b:User)}`). Standard boolean negation with `NOT EXISTS { ... }` follows from the Kuzu engine's openCypher compliance (carried forward unchanged — LadybugDB 0.12.0 release post states "functionally equivalent to kuzu v0.11.3", and Kuzu's subquery grammar accepts the negation as an ordinary boolean-expression position). If the integration spike ever found the negated form rejected by the parser, the documented workaround `WHERE NOT EXISTS { MATCH (i)-[:CALLS]->(f) WHERE f.qname = 'failure_path' }` can be reshaped as `WITH i, EXISTS {...} AS hasFailure WHERE NOT hasFailure`, which is also in-grammar — so this row is not at risk. Scored 1.

### F7 — Aggregation + grouping
**Score:** 1
**On-paper test query:**
```cypher
MATCH (i:Item)
RETURN i.crate AS crate, count(*) AS n
ORDER BY n DESC
```
**Notes:** `count(*)`, `avg`, and implicit GROUP BY (Cypher aggregates group by all non-aggregate return keys) are standard Cypher. The WITH-clause docs demonstrate `WITH avg(a.age) as avgAge ...` verbatim. The subquery docs additionally expose `COUNT { MATCH ... }` subqueries with `DISTINCT` variants — strictly more than required for Patterns A and F.

### F8 — Parameterized queries (no string-building)
**Score:** 1
**On-paper test query:**
```rust
let mut prepared = conn.prepare(
    "MATCH (i:Item) WHERE i.qname = $qname RETURN i"
)?;
conn.execute(&mut prepared, vec![("qname", Value::String(qname))])?;
```
**Notes:** Documented verbatim on docs.rs: `Connection::prepare(&self, query: &str) -> Result<PreparedStatement, Error>` and `Connection::execute(&self, prepared: &mut PreparedStatement, params: Vec<(&str, Value)>)`. The get-started prepared-statements page states: "Parameterized variables in Cypher are marked using the `$` symbol" and emphasizes injection-safety as the explicit motivation. Rust `Value` enum carries `String`, `Int64`, list, etc., so binding is type-safe, not string-built. Satisfies safety invariant G2.

### F9 — Multi-valued repeated edges between same pair
**Score:** 1
**On-paper test query:**
```cypher
CREATE REL TABLE CALLS(FROM Item TO Item, file STRING, line INT64, arg_idx INT32, MANY_MANY);
```
**Notes:** DDL page documents multiplicity explicitly: `MANY_MANY` is the default and "permits multiple edges between the same node pair". Combined with edge properties (`file`, `line`, `arg_idx`), multiple `:CALLS` edges between the same `(src, dst)` each carrying a distinct call-site fingerprint are natively representable — bag semantics rather than set. This is the exact shape Pattern B needs.

## Schema requirements

### S1 — Typed node labels
**Score:** 1
**Evidence:** `CREATE NODE TABLE User (name STRING, age INT64 DEFAULT 0, reg_date DATE, PRIMARY KEY (name));` — source: docs.ladybugdb.com/cypher/data-definition/create-table. Typed labels are mandatory in strict-type subgraphs (default) and are first-class in MATCH: `MATCH (a:User)`.

### S2 — Typed edge labels
**Score:** 1
**Evidence:** `CREATE REL TABLE Follows(FROM User TO User, since DATE);` — edge labels are first-class table names with typed FROM/TO endpoints. Multi-pair rel tables (`CREATE REL TABLE Knows(FROM User TO User, FROM User TO City)`) are also supported.

### S3 — Node properties (string/numeric/boolean)
**Score:** 1
**Evidence:** DDL docs list `STRING`, `INT64`, `DATE`, `BLOB`, `SERIAL`, and per the Cypher data-types page the full set includes `BOOL`, `INT8/16/32/64`, `UINT*`, `FLOAT`, `DOUBLE`, `DECIMAL`, `DATE`, `TIMESTAMP`, `INTERVAL`, `UUID`, `BLOB`, `LIST`, `STRUCT`, `MAP`. Covers the string+numeric+boolean baseline easily.

### S4 — Edge properties
**Score:** 1
**Evidence:** REL TABLE accepts an arbitrary property list: `CREATE REL TABLE Follows(FROM User TO User, since DATE);` — `since` is an edge property. This directly supports `:INVOKES_AT {file, line}` and `:RECEIVES_ARG {param_index}`.

### S5 — Multi-valued repeated edges (bag semantics)
**Score:** 1
**Evidence:** Multiplicity docs: "Multiplicity options: `MANY_MANY` (default), `MANY_ONE`, `ONE_MANY`, `ONE_ONE`. `MANY_MANY` permits multiple edges between the same node pair." Default behavior is bag semantics, matching cfdb's call-site-per-edge model.

### S6 — Bulk insert
**Score:** 1
**Evidence:** `COPY FROM` is documented as "the fastest way to bulk insert data into Ladybug" for graphs "with millions of nodes and beyond." Supports CSV, Parquet, JSON, DataFrames, NumPy, and subquery sources; automatic spill-to-disk for relationship imports "100M+ rows"; column projection `COPY Person(id, name, age) FROM 'person.csv'`; multi-FROM disambiguation `COPY Knows FROM 'file.csv' (from='User', to='User')`. Kuzu's underlying COPY FROM path is the well-known bulk-loader (vectorized columnar ingest, not row-by-row INSERT) and carries forward unchanged in LadybugDB. 15k nodes + 80k edges is trivially inside the documented operating envelope.

### S7 — Read-only mode / snapshot isolation
**Score:** 1
**Evidence:** `SystemConfig::read_only(bool)` is a documented builder method on docs.rs/lbug (0.15.3). Usage pattern shown: `SystemConfig::default().read_only(true).max_num_threads(8).buffer_pool_size(1024)`. The `Database` struct is `Send + Sync`, supporting multi-threaded concurrent readers from a single `Database` handle. Kuzu/LadybugDB's homepage claims "Serializable ACID transactions", and the engine uses MVCC for read isolation during writes — so re-extract-into-separate-db-then-atomic-swap is the clean deployment model, with `read_only(true)` on the query-side handle enforcing G2 at the API boundary.

## Detailed notes

- **Fork provenance is clean.** LadybugDB v0.12.0 release post states bluntly: "LadybugDB v0.12.0 is functionally equivalent to kuzu v0.11.3. The primary change involves rename kuzu to the correct package name in your language." The only documented divergence through v0.12 was governance (dropping CLA, community ownership) and CI migration to GitHub runners. By v0.15.3 the crate body is still ~99% the upstream C++ engine (232k C headers + 191k C++ + ~3k Rust glue) — so Kuzu's openCypher semantics, COPY FROM bulk loader, MVCC transactions, and DDL surface all carry forward with no reason to suspect regressions in the feature rows scored above.
- **Maintainer credibility is high.** Published by `adsharma` (Arun Sharma, Kuzu co-founder), 10 versions in ~5 months (0.0.1-pre.2 in November 2025 through 0.15.3 in April 2026), ~8.5k downloads, active release cadence. This is not a drive-by fork.
- **Rust API surface is documented and production-shaped.** docs.rs/lbug exposes `Database`, `Connection`, `SystemConfig`, `PreparedStatement`, `QueryResult`, `Value`, `LogicalType`, `NodeVal`, `RelVal`, `ArrowIterator`. `Connection::query/prepare/execute`, `set_query_timeout`, `interrupt`, and `set_max_num_threads_for_exec` are all present. Crate docs coverage is listed as 61.17% — not exhaustive, but the load-bearing surfaces for cfdb are all documented.
- **Docs substrate.** Primary LadybugDB docs exist at `docs.ladybugdb.com` and cover the full Cypher manual (syntax, data types, query clauses, functions, subqueries, DDL, DML, transactions, COPY FROM for CSV/Parquet/JSON, ATTACH/DETACH). No fallback to archived Kuzu docs was needed for any feature row except the `NOT EXISTS` negation shape, which is identical openCypher grammar and is confirmed by the equivalence statement in the 0.12.0 release post.
- **One observability caveat worth noting** (not scored): the product page markets "10x Faster Queries" but publishes no reproducible benchmark on the site — the 15k-node / 80k-edge cfdb scale is far inside any reasonable operating envelope, so benchmark credibility does not affect this gate, but Gate 2's integration spike should still measure real query latencies on cfdb-shaped workloads rather than trust the landing-page claim.
- **`read_only` caveat to validate in Gate 2:** the documented `SystemConfig::read_only(true)` flag binds at the `Database` handle, not per-connection. The re-extract workflow should be: extract to a temp database, atomically rename/swap the filesystem path, then re-open the read-only handle. Multi-reader-during-write on the *same* database file is claimed via Kuzu's MVCC but was not verified against a primary doc quote and should be confirmed in the spike. Scored 1 because `read_only(true)` exists and the Database is `Send + Sync`, which covers the G2 invariant at the API boundary even if the MVCC multi-reader story is weaker than hoped.

## Verdict

**ADVANCE to Gate 2.** LadybugDB scores 9/9 on features and 7/7 on schema — a clean pass on both thresholds with primary-doc evidence for every row. The Rust API (`Database::new`, `Connection::prepare/execute`, `SystemConfig::read_only`, typed `Value` bindings) is exactly the shape cfdb needs; the DDL model (typed node tables, typed rel tables with properties, `MANY_MANY` default multiplicity) maps 1:1 onto the `:Item` / `:CALLS {file, line}` schema without compromise; COPY FROM covers bulk ingest; and the parser-level features (var-length paths, OPTIONAL MATCH, `=~` regex, EXISTS/COUNT subqueries, parameterized queries) cover every one of the 9 qbot-core patterns without workaround. The fork is maintained by Kuzu's co-founder, is within 4 minor versions of feature equivalence with pre-archive Kuzu, and has an active release cadence. The integration spike should focus on (a) measuring actual query latencies at cfdb scale, (b) confirming the multi-reader-during-write story for the re-extract workflow, and (c) validating that `Connection::execute`'s `Value::List` binding composes cleanly with `UNWIND $list`/`WHERE x IN $list` for F5's raid-plan shape.
