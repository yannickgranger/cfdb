# Study 001 — Graph store selection methodology

**Status:** Methodology locked. Study execution pending.
**Date:** 2026-04-13
**Blocks:** cfdb v0.1 scaffold (#3624) and downstream issues (#3626, #3627, #3628).
**Related RFC:** `docs/rfc/029-cfdb.md` → `.concept-graph/RFC-cfdb.md` §10.1 (graph store decision).
**Author context:** agent-team council (PR #3621) picked LadybugDB on thin evidence (Rust guru web research only). This methodology exists to replace that pick with a defensible, evidence-backed selection **before any cfdb code is written** — because a store swap mid-build is 2–3 weeks of rework.

---

## 1. Scope

This study selects **exactly one** graph store backend for cfdb v0.1.

**Not in scope:**
- Revisiting the JSONL-canonical-dump architecture (§12.1). The store is a *cache*, not a fixture — determinism is asserted on the JSONL, not the store's file format. This relaxes the store requirement: byte-stable file format is **not** a gating criterion.
- Picking a fallback store for v0.2+. The `StoreBackend` trait (#3628) makes a future swap cheap; this study picks the v0.1 primary only.
- Evaluating non-graph stores (DuckDB without DuckPGQ, plain SQLite with adjacency tables, etc.) — expressivity gap is too large, already rejected in RFC §10.1.

---

## 2. Method — three gates

```
6 candidates
    │
    │ Gate 1 — feature adequation (docs review, ~2–3 days)
    │         ≥7/9 patterns PASS or candidate is dropped
    ▼
3–4 candidates survive
    │
    │ Gate 2 — repo quality scorecard (scripted, ~0.5 day)
    │         ≥60% weighted score or candidate is dropped
    ▼
2–3 candidates survive
    │
    │ Gate 3 — integration spike (runnable code, ~1 day per candidate)
    │         all 5 spike tasks complete in ≤4h, all 3 queries correct,
    │         latency ≤1s on 15k/80k fixture, determinism check passes
    ▼
1 pick — documented, with evidence
```

If **zero** candidates survive Gate 3, the study recommends the petgraph-baseline fallback (documented cost, known scope — ~2–3 weeks of work for a minimal openCypher subset interpreter over `petgraph`). This is the explicit "none fit" outcome, not a default.

---

## 3. Candidate shortlist

Six candidates enter Gate 1. The list is fixed at the start of the study; adding candidates mid-study requires a methodology amendment.

| # | Candidate | Query language | Embedded? | Rust binding | Prior RFC status |
|---|---|---|---|---|---|
| 1 | **LadybugDB** (`lbug` crate) | openCypher | ✅ | `lbug`, cxx FFI, 0.15.x | RFC council pick (thin evidence) |
| 2 | **DuckDB + DuckPGQ** | SQL/PGQ (Cypher-inspired `MATCH`) | ✅ | `duckdb-rs` (mature) | RFC plan B |
| 3 | **Oxigraph** | SPARQL (different query shape) | ✅ | pure Rust | Not previously considered |
| 4 | **SurrealDB** (embedded mode) | SurrealQL (graph + document) | ✅ | `surrealdb` | Not previously considered |
| 5 | **CozoDB** | Datalog + Cypher-subset | ✅ | pure Rust, `cozo` crate | Not previously considered |
| 6 | **petgraph-baseline** | hand-built openCypher subset | ✅ (in-process) | pure Rust | Reject-but-documented-fallback |

**Excluded from the shortlist (with reason):**
- **Kuzu** — archived 2025-10-13 after Apple acquired Kùzu Inc.; crate frozen at v0.11.3, no maintainer. Verified by Rust guru during RFC council review.
- **FalkorDB** — requires Redis daemon; breaks the per-developer-local deployment model (RFC §14 Q5).
- **Neo4j** — requires JVM + server; same reason as FalkorDB.
- **Memgraph** — requires server; not embeddable.
- **Apache AGE (PostgreSQL extension)** — requires PostgreSQL server; not embeddable.
- **TypeDB** — requires server; not embeddable.
- **TerminusDB** — server-based; embedded mode exists but immature.

---

## 4. Gate 1 — feature adequation

**Axis:** can the query language express the 9 §3 patterns?

**Method:** for each candidate, read the query language docs and answer 9 yes/no questions with a test query written on paper (not executed). Record workarounds where the capability exists but requires a non-obvious idiom.

### 4.1 Required query capabilities

Derived from RFC §3 (9 patterns) + §6 (API verbs) + §7 (schema).

| # | Capability | Which pattern needs it | Canonical test query |
|---|---|---|---|
| **F1** | Fixed-hop label + property match | A (HSB), C (duplicate canonicals) | `MATCH (a:Item),(b:Item) WHERE a.name=b.name AND a.crate<>b.crate` |
| **F2** | Variable-length path `[:X*1..N]` | B (VSB), I (raid hidden callers) | `MATCH (ep:EntryPoint)-[:CALLS*1..10]->(fn:Item)` |
| **F3** | Property regex in `WHERE` | D (forbidden fn in scoped crates) | `WHERE callee.qname =~ 'chrono::Utc::now'` |
| **F4** | `OPTIONAL MATCH` / left join | C (canonical-missing), G (concept absence) | `OPTIONAL MATCH (canonical:Item)-[:CANONICAL_FOR]->(c)` |
| **F5** | External parameter sets / input bucket joins | I (raid plan validation) | `WITH $plan_drop AS drops MATCH (i:Item) WHERE i.qname IN drops` |
| **F6** | `NOT EXISTS` / anti-join | C (dead impl), G (co-occurrence), H (fallback-path missing) | `WHERE NOT EXISTS { (i)-[:CALLS]->(fallback) }` |
| **F7** | Aggregation + grouping | A (cluster sizes), F (money-path checklist) | `COUNT(*) GROUP BY` |
| **F8** | Parameterized queries (no string-building) | all patterns (G2 safety) | `$concept`, `$qname`, `$rule_path` bound safely — zero string interpolation |
| **F9** | Multi-valued repeated edges between same pair | B (same fn called at multiple sites) | Multiple `:CALLS` edges with distinct `:CallSite` properties |

### 4.2 Schema requirements

| # | Capability | Required because |
|---|---|---|
| S1 | Typed node labels (`:Item`, `:CallSite`, ...) as first-class | Every pattern dispatches by label |
| S2 | Typed edge labels (`:CALLS`, `:TYPE_OF`, ...) as first-class | Patterns B, I traverse specific edge types |
| S3 | Node properties with string + numeric + boolean types | qname, file, line, signature_hash, unwrap_count |
| S4 | Edge properties | `:INVOKES_AT` carries file+line; `:RECEIVES_ARG` carries param index |
| S5 | Multi-valued repeated edges (bag semantics, not set) | Same fn may CALL another fn at N distinct call sites |
| S6 | Bulk insert (millions of facts in seconds, not inserts-per-statement) | Full workspace extraction: ~15k items + ~80k edges |
| S7 | Read-only mode / snapshot isolation | G2: queries can't mutate; multiple readers during re-extract |

### 4.3 Gate 1 scoring

For each candidate, fill out the 9-row feature table and 7-row schema table. Each row scores 1 (PASS), 0.5 (workaround exists), 0 (FAIL).

**Threshold to advance to Gate 2:** ≥ 7 / 9 on features **AND** ≥ 6 / 7 on schema. Either miss below threshold drops the candidate.

**Output format:** a table per candidate with a one-line note per capability, documenting the idiom or the workaround or the failure mode. Committed as `.concept-graph/studies/001-gate1-{candidate}.md`.

---

## 5. Gate 2 — repo quality scorecard

**Axis:** is the project actively maintained and architecturally healthy enough to depend on?

**Method:** scripted fetch from GitHub / GitLab / Gitea / codeberg API. No gut-feel reads — every row is a number.

### 5.1 Metrics and weights

| Dimension | Metric | Threshold | Weight |
|---|---|---|---|
| **Activity** | Commits in last 30 days | ≥ 10 | 10% |
| | Commits in last 90 days | ≥ 30 | 10% |
| | Distinct contributors in last 12 months | ≥ 3 | 10% |
| | Release cadence | ≥ 1 tagged release per quarter | 10% |
| **Stability** | Stars (proxy for real-world use) | ≥ 500 | 5% |
| | Tagged releases with semver discipline | yes | 5% |
| | Open-CVE count | 0 (or all mitigated) | 10% |
| | Open/closed issue ratio (last 12 months) | ≤ 0.5 | 5% |
| | License (Apache-2.0 / MIT / BSD compatible — **no AGPL**) | compatible | 5% |
| **Architecture** | CHANGELOG.md / RELEASES.md present | yes | 5% |
| | CI configured + passing on `main` | yes | 5% |
| | Test coverage visible in CI or README | yes | 5% |
| | `docs.rs` / rustdoc coverage for the Rust binding | ≥ 50% | 5% |
| | LOC order of magnitude (`cloc`) | < 500k (tractable for a solo dev to diagnose) | 5% |
| | Bus factor — top contributor's share of last-year commits | < 80% (not pure single-maintainer) | 5% |

**Total weight: 100%.**

### 5.2 Gate 2 scoring

Each row: 1 (meets threshold), 0 (misses). Weighted sum across all rows. **Threshold to advance to Gate 3: ≥ 60%.**

Candidates missing Gate 2 are documented with the specific metrics that failed. No re-scoring without methodology amendment.

**Output format:** a single table `001-gate2-scorecard.md` with one row per candidate, numeric per metric. No prose per candidate at Gate 2 — just the numbers.

### 5.3 Special case — the petgraph baseline

The petgraph-baseline candidate doesn't go through Gate 2 in the same way: `petgraph` itself is a well-maintained Rust crate (activity + stability pass trivially), but the "build a Cypher-subset interpreter on top" work is new code authored during cfdb development. Gate 2 for this candidate scores `petgraph` itself, and the "interpreter on top" cost is evaluated at Gate 3 as engineering effort rather than as a repo score.

---

## 6. Gate 3 — integration spike

**Axis:** does the candidate actually work in practice, with acceptable developer ergonomics?

**Method:** for each candidate that cleared Gates 1 and 2, build a minimal runnable Rust spike. Time-boxed to 4 hours. If the spike takes more than 4 hours, the candidate fails Gate 3 (developer ergonomics signal).

### 6.1 Spike tasks (timed)

| # | Task | Time budget | Fail condition |
|---|---|---|---|
| T1 | `cargo new` a spike crate at `.concept-graph/studies/spike/{name}/`, add the candidate's crate as a dep, get to `cargo build` green | ≤ 30 min | Build fails, requires system deps not declared in docs, or compilation takes > 2 min cold |
| T2 | Declare a minimal schema (4 node types: `Crate`, `Item`, `Field`, `CallSite`; 3 edge types: `IN_CRATE`, `HAS_FIELD`, `CALLS`) | ≤ 30 min | Schema declaration is unclear from docs, or the type system cannot express the §7 requirements |
| T3 | Bulk-insert 100 nodes + 300 edges from a hand-authored JSON fixture committed at `.concept-graph/studies/spike/fixture-small.json` | ≤ 1 h | Insert API is per-row only and slow, or transaction boundaries are unclear |
| T4 | Run the 3 highest-leverage Gate 1 queries (F1 fixed-hop HSB, F2 variable-length reachability, F3 regex WHERE) | ≤ 1 h | Any query fails to parse, returns wrong results, or requires client-side filtering |
| T5 | Latency measurement on a synthetic 15k-node / 80k-edge fixture (generated by a small script, committed at `.concept-graph/studies/spike/fixture-large.json`) | ≤ 1 h | Latency > 1s on any of the 3 queries; OOM at 15k/80k |

**Total time budget: 4 hours.**

### 6.2 Spike scorecard

Recorded per candidate in `.concept-graph/studies/001-gate3-{candidate}.md`:

| Dimension | Measurement |
|---|---|
| **Install friction** | Time to first `cargo build`. Any system deps (`apt`, `cmake`, `llvm`, Python). Binary size of the candidate's shared library. |
| **Binding ergonomics** | Do queries return `Result<Row, Error>` or raw FFI pointers? Is the main handle `Send + Sync`? Does it own its own runtime or plug into tokio? |
| **Query latency** | p50 / p99 latency on the 3 Gate 3 queries, measured with `criterion` or plain `std::time::Instant`, averaged over 10 runs on the 15k/80k fixture. |
| **Determinism check** | Run the full extraction twice, export to JSONL via the candidate's dump API (or a manual walker if no dump API), `sha256sum` both. Must be identical. Documents whether any configuration knob (single-threaded iteration, stable sort) was needed. |
| **Error messages** | Trigger 3 error conditions: (a) malformed query, (b) unknown label, (c) type mismatch. Record whether the error names the mistake or just says "query error". |
| **Memory footprint** | RSS at idle after load. RSS under query load (max observed during the 3 Gate 3 queries). |
| **Pain points** | Free-form list of every friction encountered during the spike — every "oh wait I had to…" moment. This is the signal for developer ergonomics more than any other metric. |

### 6.3 Gate 3 threshold

All of the following must hold for PASS:

- All 5 spike tasks completed within 4 total hours
- All 3 Gate 3 queries returned the correct result set on `fixture-small.json`
- p99 latency ≤ 1s on all 3 queries on `fixture-large.json`
- Determinism check passes (sha256 identical across two runs)
- RSS under query load ≤ 1 GB (per-developer-local deployment constraint)

Any miss → FAIL. Multiple candidates may pass; the tiebreaker is the pain-point list (shorter wins).

---

## 7. Deliverable

`.concept-graph/studies/001-graph-store-selection.md` (the final study report, committed after all three gates complete) with:

1. **Gate 1 results** — 9 patterns × 6 candidates, checkmark / cross / workaround, one paragraph per capability per candidate
2. **Gate 2 scorecard** — metrics × candidates, numeric, weighted score, survive/drop
3. **Gate 3 spike reports** — per surviving candidate, the full spike writeup (install, queries, latency, determinism, ergonomics, pain points)
4. **Final pick** with evidence and reasoning
5. **Rejected candidates** with specific reasons (which gate failed, which metric, which pain point)
6. **Re-evaluation triggers** — conditions under which the pick should be revisited (e.g., candidate's maintainer burnout, incompatible major version, license change, performance regression in qbot-core extraction at N items)

**Supporting artifacts committed alongside:**
- `.concept-graph/studies/001-gate1-{candidate}.md` × 6
- `.concept-graph/studies/001-gate2-scorecard.md`
- `.concept-graph/studies/001-gate3-{candidate}.md` × 2–3 (only survivors)
- `.concept-graph/studies/spike/{candidate}/` × 2–3 (runnable Rust spike crates for the Gate 3 survivors)
- `.concept-graph/studies/spike/fixture-small.json` (shared 100-node fixture)
- `.concept-graph/studies/spike/fixture-large.json` (shared 15k-node fixture)

---

## 8. Effort estimate

| Gate | Effort | Notes |
|---|---|---|
| Gate 1 (docs review) | 2–3 days | 6 candidates × ~30 min per capability × 9 capabilities = ~1 day of reading, + 1–2 days of writing up in standard format |
| Gate 2 (scorecard) | 0.5 day | Scriptable. GitHub API + a few Python lines. Most cost is finding non-GitHub repos (codeberg, GitLab mirrors). |
| Gate 3 (spikes) | 3–5 days | 1 day per surviving candidate (3–5 survive); one shared day for fixture generation and harness code |
| Write-up | 0.5–1 day | Consolidating into the final `001-graph-store-selection.md` report |
| **Total** | **6–9 days focused** | 1.5–2 weeks calendar with normal interruptions |

**Budget ceiling:** if the study exceeds 10 working days, escalate — either a candidate is hiding a show-stopper, or the methodology itself needs revising.

---

## 9. Re-evaluation triggers

This study produces a v0.1 pick. The pick is **not** re-evaluated unless at least one of the following holds:

1. The picked candidate is archived / loses maintenance (like Kuzu did)
2. A candidate CVE lands that is unfixed after 30 days
3. cfdb's extraction on qbot-core hits a hard latency wall (> 5s for a single `query_raw` call that was < 1s at pick time)
4. A new candidate enters the graph-store ecosystem and a non-binding technical case is made (blog post / benchmark / production use) — then `Study 002` is filed, not this one revisited
5. cfdb moves to multi-workspace cross-keyspace queries in v0.3 and the pick's federation story fails

If none of the triggers hold, the pick stands.

---

## 10. Explicit non-goals

- **Not picking a "best-in-class" store.** The study picks "adequate for cfdb v0.1". Perfect is enemy of shipped.
- **Not benchmarking at production scale.** qbot-core is ~15k items / ~80k edges. That's the scale the spike tests. Larger codebases are a future study.
- **Not evaluating cloud / hosted options.** Embedded only (RFC §14 Q5 — per-developer-local).
- **Not a replacement for `StoreBackend` trait design.** The trait (part of #3628) is defined by cfdb-core requirements, not by the pick. A future swap goes through the trait, not through a fresh store-selection study.

---

**End of methodology.** Study execution is issue #3639 (to be filed). Blocks cfdb v0.1 scaffold #3624.
