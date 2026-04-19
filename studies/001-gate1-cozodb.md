# Gate 1 — CozoDB

**Candidate:** CozoDB
**Query language:** CozoScript (Datalog-family, semi-naïve fixed-point)
**Version evaluated:** 0.7.6 (latest crates.io release; last tagged 2023-12-11)
**Docs sources:**
- https://github.com/cozodb/cozo (main branch, last commit 2024-12-04)
- https://github.com/cozodb/cozo-docs (source/*.rst)
- https://raw.githubusercontent.com/cozodb/cozo-docs/main/source/tutorial.ipynb
- https://raw.githubusercontent.com/cozodb/cozo-docs/main/source/stored.rst
- https://raw.githubusercontent.com/cozodb/cozo-docs/main/source/queries.rst
- https://raw.githubusercontent.com/cozodb/cozo-docs/main/source/aggregations.rst
- https://raw.githubusercontent.com/cozodb/cozo-docs/main/source/functions.rst
- https://raw.githubusercontent.com/cozodb/cozo-docs/main/source/execution.rst
- https://raw.githubusercontent.com/cozodb/cozo-docs/main/source/sysops.rst
- https://docs.rs/cozo/0.7.6/cozo/ (DbInstance, ScriptMutability, run_script)
- https://raw.githubusercontent.com/cozodb/cozo/main/cozo-core/src/lib.rs (run_script signature)
**Rust binding:** `cozo` crate, pure Rust. RocksDB backend pulls `cozorocks` (C++). `storage-mem`, `storage-sled`, `storage-sqlite` are Rust-only. Synchronous API.
**Date:** 2026-04-13

## Summary

| Axis | Score | Threshold | Verdict |
|---|---|---|---|
| Features (F1–F9) | 8.5 / 9 | ≥ 7 | PASS |
| Schema (S1–S7) | 6 / 7 | ≥ 6 | PASS |
| **Gate 1** | | | **ADVANCE** |

## Structural note

Cozo is Datalog, not Cypher. There is no `:Node` / `:Edge` label notion — everything is **stored relations** (typed tables) with `key_columns => value_columns` shape. You model cfdb's "node labels" by creating one relation per item kind (e.g. `:create fn {id: String => crate: String, name: String, vis: String, …}`) and model "edge labels" by creating one relation per edge kind (e.g. `:create calls {from: String, to: String, call_site: String => inline: Bool}`). This is excellent for cfdb's ~15k items + ~80k edges because the ingestion side already knows the typed kind of every fact — you are not fighting a property-graph model that wants untyped labels at runtime.

Variable-length paths are where Cozo *beats* Cypher: fixed-point recursion is the native primitive, not a bolted-on `*n..m` syntax. The 9 patterns map cleanly with one genuine weakness (F4 — there is no first-class OPTIONAL MATCH; it must be built via disjunction with explicit null fills).

## Features

### F1 — Fixed-hop label + property match
**Score:** 1
**On-paper test query:**
```cozoscript
?[caller, callee] :=
    *fn{id: caller, vis: 'pub'},
    *calls{from: caller, to: callee},
    *fn{id: callee, name}, starts_with(name, 'unsafe_')
```
**Notes:** Classic Datalog inner join — shared variables across `*relation{...}` atoms are the join keys. "Repeated use of the same variable in named rules corresponds to inner joins in relational algebra" (queries.rst). Property match is a filter applied in the same conjunction. Direct, idiomatic.

### F2 — Variable-length path
**Score:** 1
**On-paper test query:**
```cozoscript
reaches[from, to] := *calls{from, to}
reaches[from, to] := reaches[from, mid], *calls{from: mid, to}

?[target] := reaches['crate::root::entry', target]
```
**Notes:** Native fixed-point recursion via semi-naïve evaluation. This is strictly more powerful than Cypher's `-[*]->`: you can add depth counters, cost aggregations (`min(distance)` is a worked tutorial example — `shortest_distance[destination, min(distance)] := shortest_distance[existing_node, prev_distance], route{source: existing_node, distance: route_distance}, distance = prev_distance + route_distance`), or intermediate predicates in a single rule. Cfdb's co-occurrence / raid-plan patterns (G and I) benefit the most here. Stratification prohibits recursion through negation/non-semi-lattice aggregations but that is the correct Datalog semantics, not a gap.

### F3 — Property regex in WHERE
**Score:** 1
**On-paper test query:**
```cozoscript
?[id, name] :=
    *fn{id, name},
    regex_matches(name, '^unsafe_[a-z_]+$')
```
**Notes:** `regex_matches(x, reg)` is a built-in (functions.rst). Also `regex_replace`, `regex_replace_all`, `regex_extract`, `regex_extract_first`. Plus `starts_with`, `ends_with`, `str_includes`. All first-class boolean filters usable inside the rule body. Directly maps to cfdb's forbidden-fn and required-property patterns (D, E).

### F4 — OPTIONAL MATCH
**Score:** 0.5
**On-paper test query:**
```cozoscript
with_doc[id, name, doc] := *fn{id, name}, *doc{id, text: doc}
with_doc[id, name, doc] := *fn{id, name}, not has_doc[id], doc = null
has_doc[id] := *doc{id, text: _}

?[id, name, doc] := with_doc[id, name, doc]
```
**Notes:** There is **no first-class `left-join` / OPTIONAL MATCH** in CozoScript. The documentation does not surface such a keyword; queries.rst's full semantics are disjunction + negation. The workaround is the classic Datalog idiom: two rules with the same head, one for the "matched" case and one for the `not exists` case with an explicit `null` fill. It works, it is well-typed, but it is non-obvious compared to Cypher's `OPTIONAL MATCH`. Docs (execution.rst) confirm multiple rules with the same head name are rewritten into disjunction via DNF — so this is the idiomatic solution, not a hack. Scoring 0.5 because the author must write two rules and a helper predicate every time.

### F5 — External parameter sets (`in`)
**Score:** 1
**On-paper test query:**
```cozoscript
?[id, name] :=
    id in $target_ids,
    *fn{id, name}
```
with Rust side binding `params.insert("target_ids".into(), DataValue::List(vec![DataValue::from("fn::a"), DataValue::from("fn::b")]))`.
**Notes:** `in` is a native list-membership predicate (queries.rst: "a in [x, y, z]"). Combined with `$param` binding (confirmed via `?[…] <- $input_data` examples in stored.rst lines 94, 117, 124, 140, 303, 312) this gives exactly the parameter-set filter cfdb needs. Clean.

### F6 — NOT EXISTS / negation
**Score:** 1
**On-paper test query:**
```cozoscript
?[id, name] :=
    *fn{id, name},
    not *test_cover{fn_id: id}
```
**Notes:** `not` is a first-class keyword applicable to both inline atoms and stored-relation atoms (queries.rst: "Atoms in inline rules may be negated by putting `not` in front of them"). Safety rule enforced at compile time: "at least one binding must be bound somewhere else in the rule in a non-negated context." Recursion through negation is forbidden (stratification), which is the correct Datalog semantics and not relevant to cfdb's flat fact-base queries.

### F7 — Aggregation + grouping
**Score:** 1
**On-paper test query:**
```cozoscript
?[crate_name, count(fn_id)] :=
    *fn{id: fn_id, crate: crate_name}
```
**Notes:** Aggregation is declared in the rule head; non-aggregated head variables define the implicit GROUP BY. Supported aggregations (aggregations.rst): `count`, `count_unique`, `collect`, `unique`, `group_count`, `sum`, `mean`, `product`, `variance`, `std_dev`, `min`, `max`, `and`, `or`, `union`, `intersection`, `min_cost`, `shortest`, `latest_by`, `smallest_by`. Semi-lattice aggregations (min/max/and/or/union/intersection/min_cost/shortest) can participate in recursive rules — directly relevant to cfdb's F (money-path shortest chain) and I (raid-plan) patterns.

### F8 — Parameterized queries (no string-building)
**Score:** 1
**On-paper test query:** Rust side:
```rust
let mut params: BTreeMap<String, DataValue> = BTreeMap::new();
params.insert("target_crate".into(), DataValue::from("qbot_domain"));
let rows = db.run_script(
    "?[id, name] := *fn{id, name, crate: $target_crate}",
    params,
    ScriptMutability::Immutable,
)?;
```
**Notes:** Confirmed from `cozo-core/src/lib.rs` at main branch: `pub fn run_script(&self, payload: &str, params: BTreeMap<String, DataValue>, mutability: ScriptMutability) -> Result<NamedRows>`. Sync API. `DataValue` enum covers String / Num / List / Bool / Null. Parameters bind via `$name` substitution inside the script body. No string concatenation required; no injection surface. Note: `run_script_str` also exists taking a JSON-encoded params string — we want the typed variant.

### F9 — Multi-valued repeated edges
**Score:** 1
**On-paper test query:**
```cozoscript
:create calls {
    from: String,
    to: String,
    call_site: String,
    =>
    inlined: Bool,
    kind: String,
}
```
**Notes:** Cozo relations are keyed tables: the primary key is the tuple formed by columns before `=>`. To store multiple edges between the same `(from, to)` pair, include a disambiguator column (e.g. `call_site: String`, or a monotonic `edge_seq: Int`) in the key. stored.rst worked example: `source: String, target: String, edge_id: Int, => weight: Float, label: String` — the docs explicitly call out this pattern as "The composite key `(source, target, edge_id)` permits multiple edges between nodes." Cfdb already has natural disambiguators (call-site span, impl-block ID, generic-arg position) so this is ergonomic, not a compromise.

## Schema requirements

### S1 — Typed node labels
**Score:** 1
**Evidence:** Each cfdb item kind becomes its own `:create` relation. Types are declared in the relation schema: `column: Type` with the `=>` separator between key and value columns (stored.rst). Example verbatim from tutorial.ipynb: `:create dept_info { company_name: String, department_name: String, => head_count: Int default 0, address: String, }`. The "one relation per label" mapping is actually an improvement for cfdb because ingestion already partitions facts by kind — no runtime label dispatch, no heterogeneous node set to scan.

### S2 — Typed edge labels
**Score:** 1
**Evidence:** Same mechanism — one relation per edge kind. `:create calls {from: String, to: String => …}`, `:create implements {impl_id: String, trait_id: String => …}`, etc. Docs explicitly model this in the multi-edges example in stored.rst. Types on endpoint columns (String/Int) are enforced at `:put` time.

### S3 — Node properties with String / Int / Float / Bool
**Score:** 1
**Evidence:** stored.rst lists supported types as `String`, `Int`, `Float`, `Bool`, `Any?` (default). "Types coerce automatically where possible; mismatches abort the query." Value columns are freely typed with these primitives. Fully covers cfdb's expected per-item property set (visibility string, lines-of-code int, async bool, etc.).

### S4 — Edge properties
**Score:** 1
**Evidence:** Because edges are just relations, everything after the `=>` on an edge relation is an edge property. Tutorial example verbatim: `=> weight: Float, label: String`. No schema distinction between node-property and edge-property storage.

### S5 — Multi-valued repeated edges
**Score:** 1
**Evidence:** This is the row that "looked scary" up front but actually checks out cleanly. Because the primary key includes only the columns listed before `=>`, and stored.rst explicitly demonstrates the pattern with `source, target, edge_id` as the composite key, two rows with the same `(from, to)` pair and distinct `edge_id` (or `call_site`, or any disambiguator column) are fully supported. The only subtlety: the cfdb ingester MUST emit a disambiguator column. If it ever emits two edge facts with identical keys, `:put` upserts and the second wins silently. This is a schema-discipline requirement, not a Cozo limitation. Scoring 1 because the primitive is correct; cfdb's fact extractor is responsible for choosing the disambiguator.

### S6 — Bulk insert
**Score:** 1
**Evidence:** `:put` (upsert), `:insert` (fail-on-exists), `:replace` (truncate + insert) are the three bulk operations (stored.rst). Bulk shape from tutorial: `?[a, b, c] <- [[1, 'a', 'A'], [2, 'b', 'B'], …]; :put rel {a, b => c}` takes an inline constant relation and bulk-puts it into storage. For Rust, combine with `$input_data` parameter binding: `?[…] <- $input_data; :put rel {…}`. Also `import_relations(BTreeMap<String, NamedRows>)` on DbInstance (lib.rs:343) for direct host-side bulk load bypassing CozoScript parsing. This is fast enough for cfdb's 15k+80k fact footprint; the RocksDB backend's write path is the usual LSM bulk-load profile.

### S7 — Read-only mode / snapshot isolation
**Score:** 0.5
**Evidence:** Two separate mechanisms, both real but neither is the "open read-only" flag a spike needs.
1. **Per-query immutability:** `ScriptMutability::Immutable` on `run_script` prevents that specific call from mutating state. Confirmed in cozo-core/src/lib.rs.
2. **Instance-wide access level:** `::access_level read_only` is a system command (sysops.rst): "read_only additionally disallows any mutations and setting triggers." This is a runtime toggle, not an open-flag.
3. **Snapshot isolation:** stored.rst: "When a transaction starts, a snapshot is used, so that only already committed data...are visible to queries." Each script runs in its own transaction. This IS the isolation cfdb needs for the extraction-then-query workflow.

Scoring 0.5 because the two mechanisms together cover the need, but there is no single documented "open database read-only on process A while process B is writing" pattern. For cfdb's per-developer-embedded model, this is likely fine (the writer and reader are the same process), but a multi-process read-parallelism workflow would need the integration spike to verify.

## Detailed notes

- **Schema model — relations, not labeled nodes.** Cfdb fact extraction already knows every item's kind at emission time. Mapping `kind -> stored relation` is a pure win: schema enforcement happens at `:put`, query atoms pick the relation by name (`*fn{…}` vs `*struct{…}`), and there is no polymorphic label-filter overhead at query time. The only cost is that a query that genuinely wants "any item kind with name starting with `unsafe_`" must be written as a disjunction over every kind relation — but cfdb's 9 patterns do not include such a query.

- **Multi-edge semantics (S5 deep-dive).** The correct mental model is: "a Cozo stored relation is a B-tree keyed on the columns before `=>`." If cfdb emits `calls(from, to)` with no disambiguator, the SECOND call-edge between the same pair silently clobbers the first. Therefore cfdb's `:create calls` MUST include either `call_site: String` (source span) or a monotonic `seq: Int` in the key. The stored.rst docs call this out explicitly with a worked `edge_id` example, so it is the intended usage pattern. Build the disambiguator policy into cfdb's fact emitter now, not later.

- **Rust binding API.** Sync. `DbInstance::run_script(payload: &str, params: BTreeMap<String, DataValue>, mutability: ScriptMutability) -> Result<NamedRows>`. `NamedRows` is a struct with `headers: Vec<String>` and `rows: Vec<Vec<DataValue>>`. Constructors: `DbInstance::new("mem", "", "")` / `"sqlite"` / `"rocksdb"`. For cfdb's per-developer-embedded target, `rocksdb` is the candidate (durable + fast range scans for variable-length path queries), but it pulls `cozorocks` (C++ bundled build). If pure-Rust is a hard requirement, `sled` or `sqlite` are the Rust-only alternatives — verify write perf in the spike. Sync-only API means cfdb's query layer wraps `run_script` in `tokio::task::spawn_blocking` if it lives inside an async context.

- **Maintenance status — yellow flag, not red.** Latest tagged release is v0.7.6 (2023-12-11) and that is what crates.io still serves. BUT: the main branch has active commits as recent as 2024-12-04, merged from a `cozo-community` fork (a downstream fix for a regression in `newrocks.rs` and a fix to `stored relation prefix_join on key range`). The original maintainer (zh217) appears less active; the community fork is carrying patches. For cfdb's evaluation this means: (a) crates.io v0.7.6 is usable as-is, (b) if spike reveals a 0.7.6 bug, upgrading to a git dep on the community fork is the likely recovery, (c) a 1.0 release is NOT imminent — the README warns "Versions before 1.0 do not promise syntax/API stability or storage compatibility." The cfdb spike MUST pin an exact version and test the specific queries it needs.

- **Docs completeness.** The Sphinx/.rst source is thorough for CozoScript semantics (queries.rst, stored.rst, aggregations.rst, functions.rst, execution.rst). The live cozodb.org site returned 403 during research, forcing me to pull directly from the cozo-docs GitHub source — the raw .rst files were readable and authoritative. The Rust API side is under-documented on docs.rs (doc comments are sparse) so I had to read `cozo-core/src/lib.rs` on main to confirm the `run_script` signature. Expect the integration spike to spend a day re-deriving idioms that a more polished project would give you in the tutorial.

- **Forbidden-Prometheus check.** Cozo has zero Prometheus or OTel surface. It is a library, not a server with a metrics endpoint. Clean on the project's observability policy.

## Verdict

**ADVANCE to integration spike.** CozoDB scores 8.5/9 on features and 6/7 on schema, clearing both thresholds. Its Datalog core is a strategic fit for cfdb's recursive path queries (F2, and by extension the money-path F and raid-plan I patterns) — fixed-point recursion with semi-lattice aggregations in the rule head is strictly more expressive than Cypher `-[*]->`. The relations-not-labels schema model is not a regression for cfdb because fact extraction already partitions by kind at emission time. The two soft points are F4 (OPTIONAL MATCH requires a two-rule disjunction idiom, solvable but ergonomically noisy) and S7 (read-only mode is per-query or runtime-toggle, not an open-flag). Neither is a blocker. The real integration risks are maintenance trajectory (no release since 2023-12, though a community fork is carrying patches) and the S5 discipline requirement that cfdb's fact emitter must include a disambiguator column in every edge relation's key. Both should be exercised in the spike: (a) pin v0.7.6 from crates.io, build the cfdb schema with explicit disambiguators, ingest qbot-core's fact set, and run each of the 9 patterns as a literal CozoScript query; (b) measure rocksdb ingest throughput for the 80k-edge bulk load; (c) decide whether rocksdb (C++ via cozorocks) or sled (pure Rust) is the production backend.
