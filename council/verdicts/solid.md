# Verdict — solid

## Read log
- [x] BRIEF.md
- [x] skill-discover.md (Steps 0a–6a)
- [x] skill-prescribe.md (full, Steps 5b–5f)
- [x] skill-boy-scout.md (full)
- [~] rfc-v0.1-ratified.md (§4, §6, §11, §14)
- [~] rfc-v0.2-addendum.md (§A2, §A2.3, §A3.2, §A3.3, §A3.4)

---

## Q1 — cfdb in /discover and /prescribe

### SRP — /discover: 7 → 3 reasons-to-change

Today /discover owns distinct change vectors per step: 1a–c (grep conventions), 1d–e (split-brain census), 1f (audit-split-brain CLI), 1g (MCP bypass grep), 1h (param registry), 2–2c (call-chain heuristics), 3 (decorator patterns), 4 (ownership rules), 6a (git-log conventions). With cfdb, 1a–1g + 2–2c + 3 + 4 collapse to **one** responsibility: "format a per-issue read-model over a cfdb query set."

Remaining reasons:
1. cfdb schema evolves (upstream, uniform)
2. `.discovery/<issue>.md` formatting contract
3. Doctrinal reconciliation via `.context/<issue>.md` (steps 0a/0b/6a — stays in /discover; cfdb cannot own this per §4 "not opinionated about workflows")

**7 → 3.** BRIEF's framing as "session-scoped, issue-frozen read model over cfdb + freshness package" is SOLID-correct.

### SRP — /prescribe Steps 5b–5f: 5 → 1 upstream vector

Steps 5b/5c/5d/5e/5f are 5 different verification passes but share **one** reason-to-change: "the invariant set that CREATE must pass." Today they're 5 grep scripts with 5 independent failure modes; after cfdb they're 5 queries against 1 schema — one upstream vector (schema bump). The Step 3 decision tree stays /prescribe's unique responsibility.

### ISP — per-consumer query slicing

| Consumer | Verbs | Shape |
|---|---|---|
| /discover | `list_callers`, `query_raw`, `schema_describe` | Broad traversal |
| /prescribe | `find_canonical`, `list_bypasses`, `list_violations`, `query_raw` | Narrow canonical lookups |
| /operate-module | `query_raw` aggregates, `list_violations` | GROUP BY bounded_context |
| /boy-scout --from-inventory | `list_violations` filtered | Finding scan |

Each consumer imports 2–4 of 15 verbs; INGEST/SNAPSHOT/enrich_* groups are orthogonal. Per-consumer `.cypher` files in `queries/` directories (RFC §13) keep missing-typed-verb cases localized. **ISP ✅.**

**Risk:** `query_raw` is a universal escape hatch. Rule: typed verb if one exists; `.cypher` file in consumer/queries/ otherwise; propose a new typed verb only when ≥2 consumers need the same shape.

### DIP — what skills depend on

Three wire forms (CLI, HTTP, Rust lib) honor one contract: *"query → {rows, warnings} per the `.cypher` RETURN, deterministic per G1."*

- Sub-agent skills (/discover, /prescribe, /operate-module): CLI — no Rust compile context, file-based handoff natural
- /boy-scout --from-inventory: CLI + JSON input (already file-based via boy-scout-scope)
- In-repo Rust arch tests: Rust lib

**DIP ✅** — skills depend on the verb contract, not on LadybugDB-vs-DuckDB-vs-JSONL. Risk: embedded Cypher dialect in skills; mitigated by the per-skill `.cypher`-file discipline.

### LSP — backend swappability

- **LadybugDB ↔ DuckDB:** both satisfy the query verb contract. LSP ✅.
- **JSONL:** does NOT satisfy the query contract — serialization format, not query engine. **RFC should explicitly say JSONL is determinism/diff/snapshot, NOT a query-backend plan C.** Matters for the watchdog's offline-mode story.
- **G1 determinism:** both backends emit byte-identical sorted-JSONL. LSP ✅.

### Main Sequence distance

| | Ca | Ce | A | I | d |
|---|---|---|---|---|---|
| **cfdb-core** | high (4+ consumers) | low (syn + `StoreBackend` trait) | ≈0.6 | ≈0.2 | ≈0.2 — abstract+stable ✅ |
| **Skills** | 1 (/work-issue) | moderate (cfdb + freshness + FS) | ≈0.1 | ≈0.9 | ≈0.0 — concrete+unstable ✅ |

**Boundary verdict: clean crossing.** Skills → cfdb-core (unstable depends on stable, correct per SDP). `StoreBackend` trait protects cfdb-core from backend churn.

### Gaps (none need a 16th verb)

1. **Step 6a Decision Archaeology has no v0.1 path.** Commit messages aren't ingested. Options: (a) add `enrich_commit_messages` in v0.2 flagged non-deterministic, excluded from G1 dump (§6a is session-scoped not audit-scoped); (b) /discover keeps grep fallback for §6a. **Recommendation: (a) for v0.2, (b) is v0.1 reality.**
2. **Step 1h Param Census needs `:CallSite` + literal-argument capture.** v0.1 explicitly excludes call-graph (Q1 voted Pattern D). **Recommendation: hybrid — /discover retains grep for Step 1h in v0.1.**
3. **Freshness package is NOT replaced by cfdb.** `.context/<issue>.md` is human-curated doctrinal reconciliation, not structural fact. Confirmed SRP boundary, not a gap.

### Verdict on Q1
**GREEN** — SRP wins real (/discover 7→3, /prescribe 5→1), ISP natural, DIP clean via CLI + `.cypher`, LSP holds (with the JSONL clarification), Main Sequence crossing correct. Two narrow gaps, addressable without new verbs.

---

## Q2 — Permanent watchdog

### OCP — new-rule extensibility

| Extension | Author edits | cfdb-core edit? |
|---|---|---|
| New ban rule (Pattern D) | One `.cypher` in `qbot-core/.cypher/rules/` | ❌ |
| New debt class | `.cfdb/skill-routing.toml` + classifier `case` arm | ❌ |
| New consumer skill | New skill file + its `queries/` dir | ❌ |
| New schema node type | cfdb-extractor + minor schema bump (additive, G4) | ✅ additive |
| New verb | cfdb-core | ✅ — ISP red flag |

**OCP ✅.** Classifier `case` (§A2.2) localizes class additions to one file.

### Tier SRP

| Tier | Trigger | Runs | Writes | Blocks | Reason-to-change |
|---|---|---|---|---|---|
| **per-save** | IDE/lefthook | Incremental re-extract | `.cfdb/keyspace-local/` | — | Local responsiveness |
| **per-session** | /work-issue start | `cfdb extract` if `cfdb_sha ≠ HEAD` on scope | Session keyspace | /discover + /prescribe until refresh | Session freshness |
| **per-PR** | PR open/update | Extract HEAD → diff vs develop → rules + classifier | PR comment + CI status | New destructive findings | PR-drift detection |
| **nightly/weekly** | Cron | Full develop extract → rules → RESCUE-STATUS | Audit artifacts | — | Trend tracking |

4 distinct reasons-to-change. **SRP ✅.** Per-save vs per-session are deliberately separate (latency vs correctness budgets).

### Inventory lifecycle

- **Lives:** out-of-tree at `~/.cfdb/keyspaces/<project>/`. lbug file is a cache; committing creates byte-noise. Canonical commit artifact (if any) is sorted-JSONL per §12.1.
- **Refreshes:** /freshness owns staleness already. Extend its verdict shape with `cfdb_sha`. When `HEAD ≠ cfdb_sha` on scope files → verdict `contested`, /discover refuses. **Attribute addition, not a new staleness owner — SRP-correct.**
- **Staleness detection:** `git diff HEAD cfdb_sha --stat` filtered to session scope.
- **Invalidation contract:** /freshness reads `.cfdb/current.sha`. Missing → block ("cfdb never extracted"). Stale on scope → annotate package `cfdb_stale`, /discover refuses.

### CI failure modes

| Verdict | Trigger | Action |
|---|---|---|
| **BLOCK** | New findings (via `cfdb diff`) in `duplicated_feature`, `context_homonym`, `canonical_bypass` touching PR file | Fails CI |
| **WARN** | New findings in `random_scattering`, `unwired`, `unfinished_refactor` touching PR file | PR comment |
| **PASS** | No new findings touching PR files | Silent |

BLOCK is narrow (3 destructive classes). CI never runs remediation itself (preserves §4) — emits class + suggested skill + command in PR comment.

### Verdict on Q2
**GREEN** — OCP preserved, 4 tiers have 4 distinct reasons-to-change, inventory binds to existing /freshness owner, BLOCK narrow enough to not dominate PR friction.

---

## Q3 — Missing skills

### /operate-module

- **Description:** "Evaluate bounded-context infection thresholds and emit a raid plan for council review."
- **Responsibilities: 2** — (1) threshold check per §A3.2; (2) raid-plan markdown emission per §A3.3 template. Does NOT run cfdb, does NOT run boy-scout fallback, does NOT execute portage.
- **Arguments:** `<context-name> <inventory-json-path>`
- **ISP:** `query_raw` aggregates (COUNT / GROUP BY bounded_context) + LoC-per-crate for §A3.2. Does NOT use `list_callers`/`find_canonical` (those are /discover + /prescribe vocabulary).
- **Outputs:** `raid-plan-<context>.md` if threshold crossed, else `{below_threshold, route_to: /boy-scout}`.
- **Invariants:** (1) does not re-run cfdb; (2) does not decide remediation (council's job); (3) does not touch source files; (4) does not exceed §A3.3 template.
- **Relationship:** parallel to /discover+/prescribe (operates on contexts, not issues); upstream of /gate-raid-plan; far upstream of `/sweep-epic --mode=port`.
- **Failure modes:** schema mismatch → refuse; missing `.cfdb/concepts/<context>.toml` → refuse; invalid threshold → quote §A3.2 rule.

### /gate-raid-plan

- **Description:** "Validate a raid plan against Pattern I queries before council review."
- **Responsibility: 1** — run 5 Pattern I queries from §3.9 against plan inputs, report dangling/hidden/missing/completeness/dirty failures.
- **Arguments:** `<raid-plan-path>`
- **ISP:** `query_raw` with `sets` parameter (the `query_with_input` collapse). One verb, 5 `.cypher` files. No typed verbs.
- **Outputs:** `{completeness, dangling_drops[], hidden_callers[], missing_canonicals[], dirty_overlap[]}`.
- **Invariants:** read-only; validation only (no fixes); cite the failing Pattern I query on failure.
- **Pipeline position:** after /operate-module drafts a plan, before council reads it.
- **🚨 Sequencing constraint:** Pattern I + `:CallSite` extraction are **NOT in cfdb v0.1** (Q1 voted Pattern D). **/gate-raid-plan cannot ship until cfdb v0.2.**

### /cfdb-scope — CLI flag, NOT a skill

**Reasoning:** scope extraction is pure verb composition (`extract` + filtered `query_raw` + formatter). No reasoning, no judgment. Skills host reasoning; elevating a pure function to a skill pays sub-agent overhead for nothing.

**Spec:**
```
cfdb scope <context-name> [--workspace <path>] [--format json] [--output <path>]
```
- Reads `.cfdb/concepts/<context>.toml` → runs Pattern A/B/C filtered to those crates → classifier → emits §A3.3 JSON.
- Exit codes: 0 ok, 1 unknown context, 2 extract failure, 3 query failure.
- Determinism inherited from underlying verbs.

**Skill-vs-flag test:** if scope ever needs to reason about threshold trips, that reasoning belongs in /operate-module. Keeping /cfdb-scope as a flag preserves "cfdb emits facts, skills decide policy" (§4 invariant).

### /boy-scout --from-inventory — extension mode, NOT a sibling

**SRP test:** does `--from-inventory` change boy-scout's reason-to-change? **No.** Responsibility stays "fix ~50% of pre-existing mechanical debt in a scoped doughnut." Only the doughnut *computation* differs:

- File-proximity (today): changed files + 2-hop Cargo graph
- Inventory (new): cfdb findings filtered by class

Same fix set, same 50%/5-file budget, same output. Mode flag, not a sibling skill.

**Classes boy-scout OWNS:**
- ✅ `random_scattering` — extract helper inline
- ✅ `unwired (no tracker)` — delete dead code
- ⚠️ `unwired (with tracker)` — only when wire-up is a single mechanical call site; multi-point wire-ups route to the issue owner

**Classes boy-scout does NOT own** (exceed mechanical-only / 5-file / no-logic constraints):
- `duplicated_feature` → `/sweep-epic` (multi-file surgery)
- `context_homonym` → `/operate-module` (needs Context Mapping decision, council)
- `unfinished_refactor` → `/sweep-epic --mode=port` (needs approved raid plan + RFC)
- `canonical_bypass` → `/sweep-epic` (per §A2.3, special case of consolidation)

**Input contract:**
```
boy-scout --from-inventory <inventory.json> [--class random_scattering,unwired] --workspace <path>
```
- Inventory is §A3.3 JSON (or filtered subset).
- `--class` defaults to `random_scattering,unwired`; other values rejected with "class X routes to <skill>" citing SkillRoutingTable.
- 50% budget applies to filtered finding set.
- Commit distinguishable: `chore(boy-scout #<issue>): fix <N> inventory-driven findings near <context>`.

### Verdict on Q3
**GREEN** — /operate-module decomposed to 2 (not 4); /gate-raid-plan well-scoped but v0.2-blocked (flagged); /cfdb-scope correctly a CLI flag; /boy-scout --from-inventory is a mode flag aligning constraints to the 2 classes it actually owns.

---

## Blocking concerns

- **/gate-raid-plan cannot ship in cfdb v0.1.** Requires Pattern I + `:CallSite`; Q1 voted Pattern D which excludes both. Wiring Q3 must treat this as a v0.2 dependency.
- **Step 6a has no v0.1 cfdb path.** /discover retains grep for §6a in v0.1; v0.2 should add `enrich_commit_messages` flagged non-deterministic, excluded from G1 dump.
- **JSONL-as-query-backend confusion.** RFC should say explicitly: JSONL is serialization/determinism/diff, NOT a query backend plan C.

## Convergent follow-ups

- **From ddd (expected):** making `.cfdb/concepts/<context>.toml` mandatory for cross-cutting crates (messenger, sizer, allocators where prefix-strip heuristic fails) — I'd fold in.
- **From rust-systems (expected):** splitting cfdb-core into `cfdb-core` (pure, no I/O) + `cfdb-enrich` (I/O, git, FS) — I'd fold in; sharpens Main Sequence crossing and DIP story.
- **On /cfdb-scope:** if another specialist argues for promoting it to a skill, the test is "name one piece of reasoning it would host that isn't pure verb composition." I don't see one.
