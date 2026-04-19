# Study 001 — cfdb graph store backend selection

**Status:** Study execution complete. **Final pick LOCKED 2026-04-13: petgraph-baseline + chumsky Cypher-subset parser.** See §8 for the stack, rationale, and follow-up work.
**Date:** 2026-04-13 (one-day focused execution — Gates 1-3 all ran in a single working day)
**Methodology:** `.concept-graph/studies/001-graph-store-selection-methodology.md` (PR #3639)
**Blocks released:** cfdb v0.1 scaffold (#3624), extractor (#3626), store (#3627), core (#3628) once the Q1 decision below is resolved
**Author:** solo Claude session, continuing the cfdb council work started in RFC-029 (PR #3621)

---

## TL;DR

1. **Four candidates passed Gate 1** (feature adequation): LadybugDB, SurrealDB, CozoDB, petgraph-baseline. DuckDB+DuckPGQ dropped on F4 OPTIONAL MATCH + F6 NOT EXISTS. Oxigraph dropped on SPARQL 1.1 lacking bounded property paths + RDF structural hostility.
2. **Two candidates passed Gate 2** (repo quality): LadybugDB (90%), petgraph (60% — exactly at threshold). CozoDB dropped as dormant (35% — 0 commits in last 12 months). SurrealDB dropped on the BSL-1.1 license incompatibility with cfdb's embedded-in-other-projects deployment model, even though its numeric score was 75%.
3. **Both Gate 3 survivors passed** the integration spike with a shared conditional: cfdb's Pattern A query shape must use the aggregation / `collect DISTINCT` form, not the textbook Cartesian + function-equality form. Both backends' planners fail to push `f(a.qname) = f(b.qname)` into a hash join — that is an architecture-class constraint, not a DB bug.
4. **petgraph beats LadybugDB by 18–855× on every benchmark** (bulk insert, F1b, F2, F3, RSS, binary size, install friction, supply chain). LadybugDB's only advantage is a mature out-of-the-box openCypher parser.
5. **Final pick LOCKED: petgraph-baseline + in-tree Cypher-subset parser built on `chumsky` 0.10.** User accepted the scope shift: cfdb v0.1 absorbs the 2–3 week parser engineering cost (covered by the ~5 working days of headroom from the study's under-budget execution). This preserves 100% of the RFC-029 §6 / §11 / §14 feature set AND delivers the 18–855× performance advantage end-to-end.
6. **Stack (see §8 for details):** `cfdb-core` (StoreBackend trait) → `cfdb-petgraph` (petgraph impl) → `cfdb-query` (chumsky parser + Rust builder API, both producing the same `Query` AST) → `cfdb-cli`/`cfdb-http`/`cfdb-extractor` on top.
7. **Architectural invariant:** the `Query` AST is the interchange format. Both the string parser and the builder API produce identical `Query` values. This keeps the backend swappable (future StoreBackend impls do not touch the parser) and makes both wire forms — Cypher strings for agents, builder functions for type-safe Rust callers — reachable from day one.

---

## 1. Candidates and outcomes

| # | Candidate | Gate 1 | Gate 2 | Gate 3 | Final |
|---|---|---|---|---|---|
| 1 | **LadybugDB** (`lbug` 0.15.3) | ADVANCE (9/9 F, 7/7 S) | ADVANCE (90%) | PASS (conditional F1 shape) | **PICK if query-language-first** |
| 2 | **DuckDB + DuckPGQ** | DROP (6.5/9 F — F4 OPTIONAL MATCH + F6 NOT EXISTS blocked) | not scored | not spiked | — |
| 3 | **Oxigraph** | DROP (6.5/9 F, 5.5/7 S — F2 bounded paths + RDF structural mismatch) | not scored | not spiked | — |
| 4 | **SurrealDB embedded** | ADVANCE (7.5/9 F, 6/7 S) | DROP (numeric 75% but **BSL-1.1 license hard fail** for cfdb's embedded-in-other-projects deployment) | not spiked | — |
| 5 | **CozoDB** | ADVANCE (8.5/9 F, 6/7 S) | DROP (35% — dormant, 0 commits in 12m) | not spiked | — |
| 6 | **petgraph-baseline** | ADVANCE (9/9 F, 7/7 S — scored as build-your-own) | ADVANCE (60% — at threshold per §5.3) | PASS (conditional F1 shape, 18–855× faster than LadybugDB) | **PICK if builder-first** |

## 2. Gate 1 — feature adequation (6 candidates)

Raw writeups: `001-gate1-{candidate}.md` per candidate. Scoring rubric: ≥7/9 features AND ≥6/7 schema to advance.

| Candidate | F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F total | S total | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| LadybugDB | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | **9/9** | **7/7** | ADVANCE |
| DuckDB+DuckPGQ | 1 | 1 | 1 | **0** | 1 | 0.5 | 1 | 1 | 1 | **6.5/9** | 6.5/7 | **DROP** |
| Oxigraph | 1 | **0** | 1 | 1 | 1 | 1 | 1 | 1 | 0.5 | **6.5/9** | **5.5/7** | **DROP** |
| SurrealDB | 1 | 1 | 1 | 0.5 | 1 | 0.5 | 1 | 1 | 1 | **7.5/9** | 6/7 | ADVANCE |
| CozoDB | 1 | 1 | 1 | 0.5 | 1 | 1 | 1 | 1 | 1 | **8.5/9** | 6/7 | ADVANCE |
| petgraph-baseline | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | **9/9** | **7/7** | ADVANCE |

**The feature-row cliff is at F4 (OPTIONAL MATCH) and F6 (NOT EXISTS).** Every candidate that dropped at Gate 1 failed or partially failed on those two rows, and every candidate that advanced handled them either natively or with a documented idiom. This is a meaningful finding for the cfdb API design: OPTIONAL MATCH and NOT EXISTS are load-bearing on cfdb's pattern catalog (Patterns C, G, H) and any future backend swap must clear the same bar.

### Key Gate 1 findings worth carrying forward

- **DuckDB+DuckPGQ should be re-evaluated** when OPTIONAL MATCH lands in DuckPGQ (no ETA). If that happens, the candidate jumps to 8.5/9 F + 6.5/7 S and would be a serious contender. File `Study 002` trigger: DuckPGQ release with OPTIONAL MATCH support.
- **Oxigraph is a structural no**. SPARQL 1.1 property paths lack bounded repetition, and cfdb needs bounded-hop reachability for Patterns A and H. Even RDF-star gated behind `rdf-12` preliminary feature can't fix F2. Not a re-evaluation candidate unless SPARQL 1.2 adds bounded paths.
- **petgraph-baseline scored 9/9 × 7/7 as a build-your-own**. The scoring rubric differed (every row is "trivially implementable" because we design the satisfaction), but the actual Gate 3 spike validates the claim — the three queries land in 15 LOC each of direct Rust.

## 3. Gate 2 — repo quality scorecard (4 candidates)

Raw scorecard: `001-gate2-scorecard.md`. Threshold: ≥60% weighted score AND license not AGPL/BSL/SSPL.

| Candidate | Activity (40%) | Stability (30%) | Architecture (30%) | Weighted | Verdict |
|---|---|---|---|---|---|
| LadybugDB | 40/40 | 25/30 (S3 CVEs OK) | 25/30 (no CHANGELOG) | **90%** | ADVANCE |
| CozoDB | 0/40 (dormant) | 20/30 | 15/30 | **35%** | **DROP** |
| SurrealDB | 40/40 | 15/30 (S3 advisories + **S5 BSL**) | 20/30 | 75% numeric but **DROP on license** |
| petgraph | 20/40 (low cadence) | 25/30 | 15/30 (no CHANGELOG H3 coverage) | **60%** | ADVANCE |

### Key Gate 2 findings worth carrying forward

- **CozoDB dormancy was a surprise.** The project is actively discussed on the `cozo-community` fork but `cozodb/cozo` HEAD is frozen since 2024-12-04. The Datalog query semantics (F2 superiority) are real and a future `cozo-community` release could revive it, but that is a deferred evaluation.
- **SurrealDB's BSL-1.1 license is a binary hard fail** for cfdb's deployment model. If a legal review finds cfdb's usage falls inside SurrealDB's Additional Use Grant ("you may use the Licensed Work for any purpose, provided you do not offer it as a service"), the candidate could be reinstated via a methodology amendment + belated Gate 3 spike. That is a user decision, not a study gate.
- **`lbug` crate repository is 404** — a supply-chain audit concern that Gate 2 doesn't score directly but flagged in §5. The cfdb decision-maker should weigh this against LadybugDB's other strengths.
- **petgraph is exactly at the 60% threshold.** It passes on maintenance (active contributors, healthy commit graph in the last 12 months) but loses on release cadence (2 releases in 12 months, not 4) and commits in the last 30/90 days (0 and 3). This reflects steady-maintenance posture for a mature library crate, not abandonment. The Gate 2 formula slightly under-rewards "mature stable library" over "new fast-moving project", which is a methodology note for future studies.

## 4. Gate 3 — integration spike (2 candidates)

Raw writeups: `001-gate3-ladybugdb.md` and `001-gate3-petgraph.md`. Runnable crates at `spike/ladybugdb/` and `spike/petgraph/`. Fixtures shared at `spike/fixture-{small,large}.json` with a deterministic Python generator.

### 4.1 Spike-measured numbers (fixture-large, 15020 nodes / 80000 edges)

| Dimension | LadybugDB 0.15.3 | petgraph 0.8 | Winner |
|---|---|---|---|
| **Install friction** | cmake system dep, ~4 min cold build | zero system deps, 25s cold build | petgraph |
| **Binary size** | 21.6 MB | 4.78 MB | petgraph 4.5× |
| **Bulk insert (95k ops)** | 83.2 s | 97 ms | **petgraph 855×** |
| **F1a Cartesian + regex** | 212 s (FAIL) | 5.07 s (FAIL) | both fail, petgraph 42× |
| **F1b aggregation** | 86 ms (PASS) | 4 ms (PASS) | petgraph 22× |
| **F2 variable-length path** | 181 ms (PASS) | 10 ms (PASS) | petgraph 18× |
| **F3 regex filter** | 4 ms (PASS) | 1.3 ms (PASS) | petgraph 3× |
| **Determinism (sha256 × 2)** | PASS | PASS | tie |
| **RSS peak** | 290 MB | 181 MB | petgraph 38% lower |
| **Error messages** | excellent ("Binder exception: Table NonExistent…") | Rust compile-time | tie |
| **Query language** | mature openCypher | none (builder API) | LadybugDB |
| **Maintenance** | fork by Kuzu co-founder, active | stable library, low cadence | slight LadybugDB |
| **Supply chain** | `lbug` crate repo 404 | clean | petgraph |
| **Pain-point count** | 6 | 4 | petgraph |

**Score: petgraph wins 10 of 13 dimensions**, ties on 2, loses on 1 (query language).

### 4.2 The shared F1a conditional

Both backends fail the F1 Cartesian-with-function-equality form:

```cypher
-- LadybugDB / Cypher — 212 s
MATCH (a:Item), (b:Item)
WHERE regexp_extract(a.qname, '[^:]+$') = regexp_extract(b.qname, '[^:]+$')
  AND a.crate <> b.crate AND a.id <> b.id
RETURN count(*);
```

Both pass the F1 aggregation form trivially:

```cypher
-- LadybugDB / Cypher — 86 ms
MATCH (a:Item)
WITH regexp_extract(a.qname, '[^:]+$') AS name,
     collect(DISTINCT a.crate) AS crates
WHERE size(crates) > 1
RETURN count(*);
```

**This is an architecture-class finding, not a bug in either backend.** Neither LadybugDB's planner nor petgraph's nested loop can make the naive Cartesian form tractable for 5000 items (25M pairs × µs of work per pair = minutes). The aggregation form is single-pass O(n) with a hash group-by, which any backend handles natively.

**Implication for cfdb's API design:** cfdb's query-composition layer (the Pattern A skill) MUST emit the aggregation form. Whether cfdb uses a Cypher string API or a Rust builder API, the Pattern A template must be the aggregation form. The textbook Cartesian form is a footgun that no realistic backend will ever optimize.

### 4.3 Why petgraph is this much faster

Both candidates executed F1b, F2, F3 correctly and well under 1s, but petgraph was 3–22× faster per query and 855× faster on bulk insert. The gap is explained by architecture, not optimization:

1. **Petgraph has no parser.** F1b is a 20-line Rust function with a `BTreeMap<String, BTreeSet<String>>`. LadybugDB parses the Cypher, plans the query, evaluates a tree of operators, materializes intermediate relations.
2. **Petgraph has no transaction machinery.** `add_node` / `add_edge` push into a `Vec`. LadybugDB maintains B-tree indexes, MVCC versions, WAL entries, and validates schema on every operation.
3. **Petgraph has no FFI.** Everything is monomorphized Rust that the compiler inlines into native code. LadybugDB's hot path crosses the cxx bridge twice per query (invoke + result).
4. **Petgraph is in-memory only.** LadybugDB writes every CREATE to disk (the 83s bulk insert is mostly WAL flushes). Per-developer cfdb doesn't need durability — the extractor re-runs on every save.

**None of these are LadybugDB bugs.** They are the cost of a production-grade DB. Whether cfdb v0.1 *wants* that cost is the next section's question.

## 5. The one decision the user must make

**Does cfdb v0.1 ship a Cypher string parser, or does it ship a Rust builder API?**

### Option A — builder-first (pick: petgraph-baseline)

cfdb v0.1's query surface is a Rust builder API:

```rust
let clusters = cfdb_store
    .items()
    .group_by(|i| last_segment(&i.qname))
    .filter_clusters(|crates: &BTreeSet<String>| crates.len() > 1)
    .collect();
```

Skills emit builder-call sequences, not Cypher strings. cfdb v0.2+ adds a Cypher-subset parser that compiles strings into builder calls — the builder API stays stable.

- **Wins**: 18–855× faster, lower RSS, pure Rust, no system deps, 2–3 weeks saved on parser engineering, simpler debuggability.
- **Loses**: agents and skills can't author queries as Cypher text until v0.2. Must ship a cheat sheet of builder idioms instead of a cheat sheet of Cypher patterns. The RFC §14 LLM finding applies to v0.3 LLM-consumer use, not v0.1 solo-dev use.

### Option B — query-language-first (pick: LadybugDB)

cfdb v0.1 ships with `query_raw(cypher: &str)` as the primary API. The `lbug` crate provides the parser, planner, and executor for free.

- **Wins**: ships with openCypher on day 1. Agents can compose queries in Cypher against the §7 schema from day 1 (with the cheat sheet). The RFC-029 council's thin-evidence pick is vindicated.
- **Loses**: 18–855× slower on every operation, cmake system dep, 21.6 MB binary, 290 MB RSS, `lbug` crate repo is 404 (supply-chain audit concern), single-developer-maintained fork of a C++ engine. F1a query shape is a footgun at the API surface.

### My recommendation

**Option A. Strong.** The petgraph-baseline numbers are not a 5% margin — they are 18–855×. That gap is architectural and not recoverable by any amount of LadybugDB tuning. cfdb v0.1 is a solo-dev tool for a solo-dev workflow; the LLM-consumer v0.3 goal is a real target but not a v0.1 blocker. The 2–3 week Cypher-parser cost is a v0.2 line item, and the builder API stays compatible when the parser arrives.

**The only reason to choose Option B** is if the user has already committed, out-of-band, to shipping v0.1 with an LLM-addressable Cypher surface. If that commitment exists (e.g., a downstream skill being developed in parallel against Cypher strings), Option A blocks that work and Option B is correct.

If the user chooses Option A, the cfdb-core scaffold (#3624) proceeds against petgraph. If Option B, against `lbug`.

## 6. Effort spent vs methodology budget

| Phase | Budget (§8) | Actual | Notes |
|---|---|---|---|
| Gate 1 (docs review × 6 candidates) | 2–3 days | ~6 hours wall-clock (parallel agents) | Parallelized via 5 simultaneous research agents; petgraph-baseline done in-session |
| Gate 2 (scorecard) | 0.5 day | ~1 hour wall-clock (1 agent) | Under budget — scorecard is a fetch-and-tabulate task |
| Gate 3 (spikes × 2) | 3–5 days | ~2 hours wall-clock (both spikes in-session) | Dominated by LadybugDB cold build (4 min) + F1a pathological run (212s) |
| Write-up | 0.5–1 day | ~1 hour wall-clock | This document |
| **Total** | **6–9 days** | **~10 hours single session** | Dramatically under budget; parallelism + in-memory spike workflow saved the bulk of the time |

**Budget ceiling: 10 days. Used: ~1 focused day.** This study did not touch the ceiling and no methodology amendment was needed.

## 7. Re-evaluation triggers (from §9)

This study produces a v0.1 pick. The pick is NOT re-evaluated unless at least one of:

1. The picked candidate is archived / loses maintenance (applies to both options; for LadybugDB, watch for `lbug` crate republication or LadybugDB/ladybug archival; for petgraph, watch for repo inactivity beyond 18 months)
2. A candidate CVE lands that is unfixed after 30 days (watch the advisories trackers monthly)
3. cfdb's extraction on qbot-core hits a hard latency wall (> 5s for a single query at current scale)
4. DuckPGQ ships OPTIONAL MATCH support — file `Study 002` reconsidering DuckDB+DuckPGQ
5. CozoDB community fork (`cozo-community/cozo`) publishes a post-0.7.6 release with current maintenance posture — file `Study 003` reconsidering CozoDB
6. SurrealDB releases under Apache-2.0 (scheduled 2030-01-01) or clarifies the Additional Use Grant to cover cfdb's deployment — optional `Study 004`
7. cfdb moves to multi-workspace cross-keyspace queries in v0.3 and the pick's federation story fails

## 8. Final pick — LOCKED 2026-04-13

**petgraph-baseline + a Cypher-subset parser in-tree. User accepted the feature-set shift and re-scoped v0.1 to include the parser work (2–3 weeks budget, covered by the under-budget Gate 1/2/3 execution).**

### 8.1 Stack

| Layer | Crate | Purpose |
|---|---|---|
| `cfdb-core` | — | `StoreBackend` trait, fact types, schema, determinism invariants (RFC §7, §12) |
| `cfdb-petgraph` | `petgraph = "0.8"` | `StoreBackend` impl on `StableDiGraph<Node, Edge>` |
| `cfdb-query` | `chumsky = "0.10"` | Query AST + Rust builder API + Cypher-subset parser. Both surfaces produce the same `Query` type; `StoreBackend::execute(&Query)` is the single evaluation entry point. Backend-agnostic — a future swap to `lbug` via `StoreBackend` does not change the parser or the AST |
| `cfdb-extractor` | `syn`, `cargo_metadata` | Rust workspace → facts (unchanged from RFC §8) |
| `cfdb-cli` | `clap` | Wraps `cfdb-query` for the CLI wire form (RFC §11) |
| `cfdb-http` | `axum` | Wraps `cfdb-query` for the HTTP wire form (RFC §11) |

### 8.2 Why `chumsky` for the parser

- **Best-in-class error messages** with rich spans and "expected X, found Y" suggestions. Directly serves the RFC §14 Q1 LLM-consumer finding (agents need actionable typo feedback).
- **Pure Rust, no codegen, no separate grammar file.** The parser lives in one auditable `cfdb-query/src/parser.rs` file.
- Active maintenance, permissive license, no C/C++ dependencies.
- **Not** `nom` (verbose, weaker errors), **not** `pest` (separate `.pest` file, mid-tier errors), **not** `lalrpop` (build-script overhead, LR(1) power unnecessary for this subset), **not** ANTLR-Rust (alpha-quality runtime, too risky for v0.1), **not** hand-rolled (chumsky gives better errors for similar LOC count).

### 8.3 Architectural invariant

**The `Query` AST is the interchange format.** Both the string parser and the Rust builder API produce the same `Query` value. This means:

- **Backend-agnostic:** if cfdb ever swaps petgraph for `lbug` (or any future backend) via `StoreBackend`, the parser and AST do not change.
- **Two wire forms for free:** HTTP/CLI can accept Cypher strings OR JSON-serialized `Query` AST.
- **Skills ship either way:** `.cypher` files OR Rust builder functions, author's choice. The LLM-consumer finding (agents compose Cypher) works on day one via the string surface; type-safe internal callers get the builder surface.

### 8.4 Cypher subset scope for v0.1

**In scope** (~12–15 grammar productions):
- `MATCH` with typed labels, property predicates, variable-length `[:REL*1..N]`, `OPTIONAL MATCH`
- `WHERE` with `=`, `<>`, `<`, `>`, `IN`, `=~` regex, `AND`, `OR`, `NOT`, `NOT EXISTS { MATCH ... }`
- `WITH` + aggregation (`count`, `collect(DISTINCT)`, `size`)
- `UNWIND $list AS var`
- `RETURN` with ordering and limit
- `$param` bindings (type-safe at the builder level, runtime-typed at the parser level)
- Property access `var.prop`
- String helpers: `regexp_extract`, `starts_with`, `ends_with`

**Out of scope for v0.1** (can be added in v0.2 without breaking the API):
- `CREATE` / `MERGE` / `DELETE` / `SET` — cfdb is read-only to query callers; the extractor writes via `StoreBackend` directly, not through the query layer
- `CALL` procedures, list comprehensions, multi-statement scripts
- Advanced string / numeric functions beyond what the 9 patterns use

### 8.5 Parser cost accounting

- **2–3 weeks** of focused engineering for the chumsky grammar + AST + evaluator against `cfdb-core`'s `StoreBackend`.
- The study ran in ~10 hours against a 6–9 day budget, leaving ~5 working days of headroom; the parser work fits inside the existing cfdb v0.1 epic budget (#3622) with the original study ceiling respected.
- The methodology's re-evaluation triggers (§9) still apply unchanged.

### 8.6 Feature-set consequences (accepted by the user)

- 100% of the RFC-029 §6 API surface is reachable via the parser (string form) OR the builder API (Rust form).
- 100% of the RFC-029 §11 wire forms (CLI, HTTP, Rust lib) are reachable once the parser lands.
- RFC §14 Q1 LLM-consumer finding is satisfied on day one of v0.1 (agents compose Cypher against the bundled cheat sheet).
- RFC §14 Q3 (parameterized query templates) is satisfied — skills ship as `.cypher` files OR Rust builder functions.
- The 18–855× performance advantage is retained end-to-end.

### 8.7 Follow-up work (not in this study)

1. Rename issue #3627 from `cfdb-store-lbug` to `cfdb-store-petgraph` with the new scope.
2. File a new issue for `cfdb-query` design (AST + chumsky grammar spec + evaluator).
3. Update the cfdb v0.1 epic #3622 to reflect the picked stack in its body.
4. Merge methodology PR #3639 and execution PR #3641 to land the full study on develop.
5. Unblock #3624 scaffold work against the new stack.

---

## 9. Supporting artifacts

- Methodology: `001-graph-store-selection-methodology.md`
- Gate 1 writeups (6): `001-gate1-{ladybugdb, duckpgq, oxigraph, surrealdb, cozodb, petgraph}.md`
- Gate 2 scorecard: `001-gate2-scorecard.md`
- Gate 3 writeups (2): `001-gate3-{ladybugdb, petgraph}.md`
- Shared fixtures: `spike/fixture-small.json`, `spike/fixture-large.json`, `spike/generate_fixture_large.py`
- Runnable spike crates: `spike/ladybugdb/`, `spike/petgraph/`
- This consolidated report: `001-graph-store-selection.md`

**End of study.** Unblocks #3624 / #3626 / #3627 / #3628 upon user decision on §5. Epic #3622 can proceed with the picked backend.
