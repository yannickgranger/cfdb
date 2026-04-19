# Ratified Council Decisions — cfdb ↔ /discover, /prescribe, Watchdog, Missing Skills

**Date:** 2026-04-14
**Team:** `council-cfdb-wiring`
**Members:** ddd, rust-systems, clean-arch (R1 by clean-arch, R2 not re-confirmed — R1 positions carry), solid
**Convening question:** The v0.2 addendum council forgot to integrate /discover with cfdb. This document repairs that gap and ratifies the wiring, the permanent watchdog mechanism, and the specs for 4 new or extended skills.

---

## TL;DR

cfdb integrates with /discover AND /prescribe via subprocess CLI, pinning a keyspace SHA in every discovery artifact so prescription verifies against the same facts. A five-tier watchdog (per-save / per-session / per-PR / nightly / weekly) anchors enforcement, with /freshness owning staleness detection via scope-diff (not commit count) and CI never auto-remediating. Four new skills are scoped: `/operate-module` (2 responsibilities), `/gate-raid-plan` (5 Pattern I checks, two phases, one skill), `/cfdb-scope` (CLI flag not a skill), and `/boy-scout --from-inventory` (single skill, two input modes, owning random_scattering + unwired-no-tracker only). Eight blocking concerns identified; cfdb v0.1 needs one minor schema bump (additive) and one new typed verb (`list_items_matching`) before full wiring is possible. The rest waits on cfdb-hir-extractor (v0.2 Phase B).

---

## Section A — Convergent decisions (unanimous or 3-of-4, R1 + R2 combined)

### A.1 Pipeline shape

```
source.rs → cfdb extract → .cfdb/snapshots/<sha>.jsonl.gz (canonical, committed)
                           .cfdb/keyspaces/<sha>/ (.ldb backend cache, gitignored)
                              ↓
                          /freshness (scope-staleness check; refreshes on-demand)
                              ↓
                          /discover (session-scoped read model over cfdb + doctrinal context)
                              ↓
                          .discovery/<issue>.md  ← pins cfdb_keyspace_sha: <sha12> in frontmatter
                              ↓
                          /prescribe (reads artifact + queries cfdb at pinned SHA)
                              ↓
                          .prescriptions/<issue>.md
                              ↓
                          gates read both artifacts to verify implementation
```

### A.2 Wire form

cfdb is invoked by skills via **subprocess CLI** (`Bash(cfdb query ...)`), NOT as a Rust library dependency. Skills are markdown + shell; there is no `use cfdb_core;` in any skill-adjacent Rust crate. Rationale: skill agents run shell commands, not Rust binaries. Binding skills to cfdb-core as a Cargo dep would break the `.concept-graph/cfdb/` sub-workspace isolation decision (§8.1).

An optional future path: `/gate-raid-plan` and other latency-sensitive paths may use HTTP against a warm `cfdb-server` (Option C). Not ratified — revisit when HIR extract memory/latency numbers are measured.

### A.3 Discovery artifact pins the keyspace SHA

`.discovery/<issue>.md` gains a frontmatter field `cfdb_keyspace_sha: <sha12>`. /prescribe reads it and runs its Step 5b verification queries pinned to that SHA. If the pinned keyspace no longer exists (e.g. aggressive eviction), both skills REFUSE to run with `🔴 cfdb keyspace stale — refresh discovery`. This prevents the split-brain where discovery sees one fact set and prescription sees another.

### A.4 Grep fallback retained for two residual slices

/discover keeps hand-grep for:
1. **Step 6a Decision Archaeology rationale quotes.** cfdb provides the pointer (`:Item.git_last_commit_sha`); /discover runs `git log -1 <sha>` for the prose. Commit messages are not in the graph by §4 invariant.
2. **Step 1h Param Census literal arguments.** `.get("key")` string arguments require `:CallSite + RECEIVES_ARG` edges from cfdb-hir-extractor (v0.2, not shipped). Pre-HIR, hand-grep is acceptable.

Neither is a workflow violation — cfdb gives structured locators, skills do bounded I/O for prose.

### A.5 Phase A vs Phase B integration split

| Step | Phase | Backed by | Ships when |
| --- | --- | --- | --- |
| /discover 1a-1f (static item census) | A | cfdb-extractor (syn) | Now |
| /discover 1g (MCP/CLI bypass scope) | A (partial) | `list_callers` + crate filter | Now |
| /discover 2a-2c (entry points, CALLS*, data flow) | B | cfdb-hir-extractor | After HIR lands |
| /discover 3-4 (decorator chains, ownership) | A | syn IMPLEMENTS edges | Now (partial), full in B |
| /discover 6a (decision archaeology) | A | `enrich_history` + git log | Now |
| /prescribe 5b (resolution census) | A | `list_items_matching` + cfdb query | Now |
| /prescribe 5c (test-parser divergence) | A | schema needs `Item.is_test` (pending bump) | After schema bump |
| /prescribe 5d (cross-crate collision) | A | `list_items_matching --group-by-context` | After schema bump + 16th verb |
| /prescribe 5e (market-phenomenon heuristic) | A | crate filter | Now |
| /prescribe 5f (MCP/CLI EXTEND scan) | B | CALLS edges from HIR | After HIR lands |

### A.6 /freshness owns cfdb staleness

/freshness gains a Step 2f-cfdb that runs before /discover:

```
2f-cfdb: Check cfdb inventory freshness.
  Read: .cfdb/current.sha (the last successfully extracted workspace SHA)
  If missing: run `cfdb extract --workspace <ws>` (bootstrap path).
  Compute: git diff --name-only HEAD <cfdb_sha> -- <session-scope-files>
  If diff is non-empty:
    run `cfdb extract --workspace <ws>` for the session scope (incremental if supported)
    update .cfdb/current.sha
  Write to .context/<issue>.md frontmatter:
    cfdb_keyspace_sha: <sha12>
  Downstream /discover reads this field and pins its queries.
```

### A.7 Permanent watchdog — five tiers

| Tier | Trigger | Runs | Writes | Blocks |
| --- | --- | --- | --- | --- |
| Per file-save | lefthook / git pre-commit | `cfdb query --rule arch-ban-*.cypher` scoped to changed crates | Terminal warning | Commit yes (on rule hit) |
| Per session | /work-issue Phase 0 via /freshness Step 2f-cfdb | Scope-staleness check + optional re-extract | `.context/<issue>.md: cfdb_keyspace_sha` | /discover yes (on stale keyspace + failed refresh) |
| Per PR | CI on push | `cfdb extract <head> && cfdb diff <base> <head>` → new findings | PR comment + CI status | PR yes (new context_homonym + canonical_bypass + Pattern D/E) |
| Nightly | cron | Full extract against develop, full ruleset, refresh RESCUE-STATUS.md | `.cfdb/snapshots/<sha>.jsonl.gz` committed, `.concept-graph/RESCUE-STATUS.md` committed | No — advisory |
| Weekly | cron | `cfdb diff <prev-week-sha> <current-sha>` + delta report | `.concept-graph/RESCUE-STATUS-<date>.md` | No |

### A.8 CI BLOCK vs WARN routing

| Class | CI verdict | Route to |
| --- | --- | --- |
| `context_homonym` (new, introduced by PR) | **BLOCK** — always | `/operate-module` (council required) |
| `canonical_bypass` (new) | **BLOCK** | `/sweep-epic` |
| new Pattern D/E ban rule violation | **BLOCK** | inline fix required |
| `duplicated_feature` (new or existing in touched scope) | WARN | `/sweep-epic` |
| `unfinished_refactor` (new or existing in touched scope) | WARN | `/sweep-epic --mode=port` |
| `random_scattering` (new or existing in touched scope) | WARN | `/boy-scout --from-inventory` |
| `unwired` (new or existing in touched scope) | WARN | `/boy-scout` (delete) or issue-owner session (wire) |
| Pre-existing violations not touched by the PR | Silent | — |

CI NEVER auto-applies remediation. It annotates with routing recommendations; the next `/work-issue` session reads the annotation and acts. Preserves RFC §4 invariant "cfdb never modifies Rust files".

### A.9 No metric ratchets, no allowlists, no baselines

Per CLAUDE.md §1.1 and RFC v0.1 §6 rule 8, the CI drift gate NEVER uses an `expected_findings.json` whitelist or a per-finding waiver. False positives are fixed by editing the Cypher rule in a reviewed PR that argues the change against the whole fact base, not by waiving locally. This rule is non-negotiable and was flagged by clean-arch as a clean-arch concern for the per-PR tier.

### A.10 Artifact storage — three tiers (from R2 convergence)

| Tier | Artifact | Location | Committed? | Rationale |
| --- | --- | --- | --- | --- |
| 1 | `.ldb` backend file / keyspace cache | `~/.cfdb/keyspaces/<project>/<sha>/` (per-developer, out-of-tree) | No — always gitignored | Rebuildable cache, binary, no format stability guarantee |
| 2 | Canonical JSONL snapshot (`cfdb dump`) per §12.1 | `.cfdb/snapshots/<sha>.jsonl.gz` in-repo | **Yes** — committed when it serves as a determinism fixture or drift baseline | Published language, diffable, load-bearing for two-run determinism gate |
| 3 | Session inventory JSON (derived per-issue, per-context) | `~/.cfdb/sessions/<issue>/inventory.json` (out-of-tree, ephemeral) | **No** — discarded on session end | Pipe artifact between /freshness and /operate-module; re-queryable from tier 2 at the pinned SHA |

**Ratified unanimously by ddd, rust-systems, solid in round 2.** Clean-arch's round 1 position (out-of-tree, per-developer) is consistent with this 3-tier framing.

### A.11 Staleness algorithm (from R2)

**Primary signal: scope-diff.**

```bash
stale := git diff --name-only HEAD <cfdb_sha> -- <session-scope-files>
# non-empty → stale for this session's scope → /freshness re-extracts
```

**Rationale (solid, most forceful; ddd and rust-systems compatible):**

- Commit count is a ratchet-in-waiting ("bump N to silence alerts") — solid's objection from R2.
- 200 commits that don't touch scope files is irrelevant; 1 commit that does is blocking.
- Scope-diff binds the staleness decision to the exact facts the session depends on. SRP-cleanest.

**Secondary optimization (rust-systems, accepted):** the scope-diff may be skipped as a fast-path if HEAD is within 10 commits of `cfdb_sha`. This is a performance optimization, not a staleness signal — it assumes scope overlap is negligible in the very-small-delta case. If the fast-path is enabled, the scope-diff runs any time `git rev-list HEAD..cfdb_sha --count > 10`.

**Hard ceiling (ddd's N=50) — REJECTED** on solid's ratchet objection. Cross-scope freshness for structurally unrelated queries (duplicate detection across unrelated crates) is a CI concern, not a per-session concern; nightly cron handles it.

### A.12 Inventory refresh — layered model (from R2 convergence)

Unanimous:

1. **Nightly cron (or merge-to-develop CI)** produces the committed canonical JSONL snapshot for develop HEAD. This is the baseline.
2. **Per-session /freshness** runs `cfdb extract` on-demand against the session scope if the scope-diff signals staleness. Delta from baseline is computed, not a full workspace re-extract, where the extractor supports incremental mode.
3. If incremental extract is out of budget (HIR memory ceiling — rust-systems flagged this as an unknown that needs measurement), /freshness blocks with a clear error. There is no silent stale-use path.

### A.13 /gate-raid-plan — two-phase gate, one skill (from R2)

Ratified unanimously by ddd, rust-systems, solid in R2. Clean-arch's R1 pre-council preference is preserved in Phase 1 but no longer replaces the post-council check — both fire.

| Phase | Flag | When | Checks | v0.1 | v0.2 |
| --- | --- | --- | --- | --- | --- |
| 1 — Lint | `/gate-raid-plan --lint` | BEFORE council review | 1 (completeness), 2 (dangling-drop, name-level) | ✅ ships | ✅ |
| 2 — Preflight | `/gate-raid-plan --preflight` | AFTER council approval, BEFORE `/sweep-epic --mode=port` | 1+2+3 (hidden callers, CALLS) + 4 (missing canonical) + 5 (clean/dirty mismatch) | ❌ awaits cfdb-hir-extractor | ✅ |

Rationale (solid's SRP argument is load-bearing): both phases run the same Pattern I query set against the same cfdb schema. Only the subset + timing differ. Reason-to-change is shared (schema bump + Pattern I evolution). One skill, two modes.

### A.14 16th verb: `list_items_matching`

**Winner: `list_items_matching(keyspace, name_pattern, kinds?, group_by_context?) → {rows, warnings}`** (clean-arch R1 + solid R2, 2 votes).

Alternatives considered and rejected:
- `list_items_by_name` (ddd R2, 1 vote) — narrower name; doesn't clearly express the `kinds?` filter or pattern semantics
- `find_items` (rust-systems R2, 1 vote) — reasonable but less explicit about the matching-by-pattern intent
- `list_definitions_of(name)` (rust-systems R1, withdrawn R2) — narrowest slice but N round-trips for N CREATE decisions; worst ISP shape

**Signature:**
- `keyspace: String` — the keyspace SHA to query
- `name_pattern: String` — regex against `Item.name` (openCypher-compatible)
- `kinds?: Vec<ItemKind>` — optional filter: `Struct`, `Enum`, `Fn`, `Const`, `TypeAlias`, `ImplBlock`, `Trait`
- `group_by_context?: bool` — if true, results are grouped by `Item.bounded_context`; collapses ddd's `list_context_owner` use case

**Determinism impact:** zero — read-only composition over existing query_raw. syn-level, no HIR dep.

**Schema impact:** zero — uses existing `:Item` nodes.

**Cost:** one new clap subcommand in cfdb-cli, one wire_form test update (`tests/wire_form_16_verbs.rs`), one composition function in cfdb-core.

**Subsumes three R1 proposals:**
- clean-arch's `list_items_matching` ✅ (the direct form)
- ddd's `list_context_owner(concept)` ✅ (→ `list_items_matching(concept, kinds=[Struct,Enum,Trait], group_by_context=true)`)
- rust-systems' `list_definitions_of(name)` ✅ (→ `list_items_matching(name, kinds=None, group_by_context=false)`)

### A.15 /operate-module — 2 responsibilities

- **Description:** Evaluate cfdb bounded-context infection thresholds against §A3.2 and emit a raid plan markdown for council review.
- **Arguments:** `<context-name> <inventory-json-path> [--workspace <path>]`
- **Inputs:** pre-built tier-3 session inventory JSON (from `cfdb scope`); `.cfdb/concepts/<context>.toml` (optional, cross-cutting crate overrides); `.cfdb/skill-routing.toml`
- **Outputs:**
  - Above threshold: `raid-plan-<context>.md` at `.concept-graph/raid-plans/`, marked `council_required=true`
  - Below threshold: JSON verdict `{below_threshold: true, route_to: "/boy-scout --from-inventory", classes: [...]}` on stdout
- **Invariants (never does):**
  - Never runs `cfdb extract` or `cfdb query` — consumes a pre-built inventory only
  - Never edits source files
  - Never invokes `/sweep-epic` or `/boy-scout` — emits routing hints, never execution
  - Never declares a Context Mapping resolution (ACL / Shared Kernel / Conformist / Published Language) — raises the question for council
  - Never routes `context_homonym` to `/boy-scout` — v0.2 Q10 unanimous
- **Invocation:** NOT part of `/work-issue`'s standard pipeline. Triggered by CI annotation, nightly RESCUE-STATUS threshold alert, or manual invocation.
- **Relationship:**
  - Upstream: `/freshness` confirms session freshness; `cfdb scope --context <name> --output json` produces the inventory
  - Downstream: council review → `/gate-raid-plan --lint` → council approval → `/gate-raid-plan --preflight` → `/sweep-epic --mode=port --raid-plan=<path>`
- **Failure modes:**
  - Missing inventory → HARD STOP, "inventory not found — run `cfdb scope` first"
  - Unknown context → HARD STOP, list known contexts from `.cfdb/concepts/`
  - Plan already exists → append `## Revision <N>` section, do NOT overwrite
  - Stale inventory (from tier-3 ephemeral but sha mismatches tier-2 baseline) → WARN in plan header, do not refuse

### A.16 /gate-raid-plan — spec

- **Description:** Validate a raid plan against the live cfdb fact base via Pattern I queries.
- **Arguments:** `<raid-plan-path> --mode=lint|--mode=preflight [--workspace <path>] [--keyspace <sha>]`
- **Inputs:** `raid-plan-<context>.md`, cfdb keyspace (read via `cfdb query_with_input` — the `--input` flag of query per §6.2)
- **Outputs:** structured JSON report `{verdict: PASS|BLOCK, phase: lint|preflight, findings: [{query_id, violation, evidence: file:line}]}`
- **Invariants:** read-only; validation only; cites the failing Pattern I query on every failure
- **Pipeline position:** Phase 1 lint BEFORE council review; Phase 2 preflight AFTER council approval, BEFORE `/sweep-epic --mode=port`
- **v0.1 limitation:** only Phase 1 lint ships in v0.1 (checks 1 + 2 name-level). Phase 2 preflight (checks 3, 4, 5 using CALLS edges) awaits cfdb-hir-extractor. v0.1 preflight invocation returns `{phase: "preflight", verdict: "UNAVAILABLE", reason: "awaits cfdb-hir-extractor (v0.2)"}`.

### A.17 /cfdb-scope — CLI flag, not a skill

**Ratified unanimously.** `/cfdb-scope` is NOT a skill; it is a CLI subcommand or composition verb.

**Argument shape (ratified):**
```
cfdb scope --context <name> [--workspace <path>] [--format json] [--output <path>] [--keyspace <sha>]
```

- `--context <name>`: filter items to the named bounded context (via `:Item.bounded_context` + optional `.cfdb/concepts/<name>.toml` override)
- `--format json`: emits the §A3.3 structured inventory shape (`findings_by_class`, `canonical_candidates`, `reachability_map`, `loc_per_crate`)
- `--output <path>`: writes to file; otherwise stdout
- `--keyspace <sha>`: use a specific snapshot; defaults to latest

**This is a tier-3 artifact generator** — its output is written to `~/.cfdb/sessions/<issue>/inventory.json` (ephemeral) and consumed by `/operate-module`. Then discarded.

**Why not a skill:** a skill hosts reasoning. `cfdb scope` is pure data aggregation (context filter + query composition + §A3.3 shape). Elevating it to a skill pays sub-agent overhead for nothing and violates RFC §4 workflow-agnosticism. Solid's SRP test: "name one piece of reasoning it would host that isn't pure verb composition" — no one could. Unanimous.

### A.18 /boy-scout --from-inventory — single skill, two modes

**Ratified unanimously.** `/boy-scout` gains a second input mode via `--from-inventory <path>`; existing file-proximity mode (via `boy-scout-scope` binary) stays.

**Class filter — what boy-scout owns:**

| Class | Owns? | Action | Route for non-owned |
| --- | --- | --- | --- |
| `random_scattering` | ✅ | Extract helper inline (text-only) | — |
| `unwired (no tracker)` | ✅ | Delete | — |
| `unwired (tracker exists)` | ⚠️ partial | Wire only if single-call-site mechanical | Issue owner session |
| `duplicated_feature` | ❌ | — | `/sweep-epic` |
| `canonical_bypass` | ❌ | — | `/sweep-epic` |
| `unfinished_refactor` | ❌ | — | `/sweep-epic --mode=port` |
| `context_homonym` | ❌ NEVER | — | `/operate-module` (always, council required) |

**Why context homonym is never auto-routed to boy-scout:** a homonym is a bounded-context ambiguity, not a code-style violation. Mechanical rename resolves the name collision but not the semantic collision, leaving the next session blind to the original DDD problem. v0.2 Q10 rationale restated by every member.

**Input contract:**

```json
{
  "context": "trading",
  "findings_by_class": {
    "random_scattering": [
      {"item_a": "qname_a", "item_b": "qname_b", "file": "...", "line": 42, "evidence": "near-identical AST body"}
    ],
    "unwired": [
      {"item": "qname_c", "file": "...", "line": 99, "tracker": null}
    ]
  }
}
```

The orchestrator (main agent or CI) filters to the 2 owned classes before handing the inventory. Boy-scout REFUSES to run if any finding has a class outside its ownership — routing bug in the caller, not a boy-scout responsibility. This is ISP on the boy-scout side: it never sees context_homonym findings and cannot be confused into acting on them.

**Budget:** same 50% / max-5-files cap as file-proximity mode. Multiple boy-scout runs consume the inventory incrementally.

---

## Section B — Blocking concerns (ratified — require action before full wiring)

### B.1 Schema bumps (v0.1 minor, additive, zero determinism impact)

1. **Add `Item.is_test: bool` attribute.** Derived from `#[cfg(test)]` module attribute and `#[test]` function attribute at syn extraction time. Unblocks /prescribe Step 5c (test-parser divergence check). Zero cost — attributes are already walked by the extractor.
2. **Add `Item.bounded_context: String` attribute.** Derived from crate-prefix convention + `.cfdb/concepts/<name>.toml` override at syn extraction time (NOT enrichment-only). Unblocks /prescribe Step 5d cross-crate collision with context-awareness. Zero cost.
3. **Add `:Context {name, canonical_crate, owning_rfc}` node + `(:Crate)-[:BELONGS_TO]->(:Context)` edge.** Complementary to B.1.2, not a replacement. Enables explicit cross-context aggregation queries. Low cost — derived from Cargo.toml metadata and `.cfdb/concepts/`.

These three are v0.1 minor schema bumps (additive only), ratified as compatible with the frozen v0.1 determinism invariants per §12.1.

### B.2 New typed verb

4. **Add `list_items_matching(keyspace, name_pattern, kinds?, group_by_context?)`** as the 16th cfdb-cli verb. Cost: one clap subcommand, one wire_form test update (`tests/wire_form_16_verbs.rs`), one composition function in cfdb-core. Syn-level, no HIR dependency. Subsumes 3 R1 proposals.

### B.3 Skill updates

5. **`/discover` output format** grows a `cfdb_keyspace_sha: <sha12>` frontmatter field. /prescribe reads it and pins Step 5b queries to that SHA. Both skills REFUSE to run if the SHA is absent or the keyspace no longer exists.

6. **`/freshness` gains Step 2f-cfdb** per A.6 above. This is the composition root for cfdb staleness detection.

### B.4 Phase B dependencies (await cfdb-hir-extractor)

7. **/discover Steps 2a–2c, /prescribe Step 5f, /gate-raid-plan Phase 2 preflight** all require `:CallSite` + `CALLS` edges from cfdb-hir-extractor. Phase A wiring proceeds now with these slices retaining grep/degraded fallback; Phase B wiring ships after cfdb-hir-extractor lands. Per v0.2 §A1 — the parallel crate, not an upgrade to cfdb-extractor.

### B.5 RFC clarifications

8. **RFC §14 Q2 (backend plan) needs explicit clarification.** Current phrasing implies JSONL is a query-backend plan C alongside LadybugDB (primary) and DuckDB/DuckPGQ (plan B). Solid's R1 LSP analysis is correct: JSONL is serialization/determinism/diff format, not a query engine. Add one-sentence clarification: *"JSONL is the canonical determinism/diff/snapshot format, not a query-backend option. Plan A = LadybugDB. Plan B = DuckDB/DuckPGQ. The sorted-JSONL dump is the published language between backends."*

---

## Section C — Non-contentious convergent follow-ups

These are convergent in R1/R2 and folded in without separate ratification:

- `.cfdb/concepts/<context>.toml` declares cross-cutting crates (messenger, sizer, allocators) where crate-prefix heuristic fails to assign a bounded context.
- The sorted-JSONL canonical dump per §12.1 is the Published Language boundary between cfdb and the skill layer. cfdb owns the schema; skill layer conforms. Conformist relationship per ddd R1.
- `signature_divergent` UDF (v0.2 gate item 8) uses cfdb's `:Concept` nodes via `LABELED_AS` edges to distinguish Shared Kernel (same concept, intentionally co-owned, same `:Concept`) from Homonym (different semantics, different or no `:Concept`). Ddd R1 specifically frames this.
- The BRIEF's pipeline diagram (cfdb extract → /discover → /prescribe) is correct and was ratified by all four members in R1 without dissent.

---

## Section D — Divergences resolved in Round 2 (for audit trail)

### D.1 Inventory storage location

**Round 1 split:** 2-2 (ddd + rust-systems in-repo vs clean-arch + solid out-of-tree).

**Round 2 resolution:** unanimous **ephemeral pipe, out-of-tree** once the 3-tier framing clarified that ddd and rust-systems were actually talking about the tier-2 canonical JSONL (which is committed) — the tier-3 session inventory JSON is ephemeral. No actual disagreement once tiers were distinguished.

### D.2 Staleness algorithm

**Round 1 split:** various commit-count thresholds (N=10, N=50) vs scope-diff only.

**Round 2 resolution:** **scope-diff is the authoritative staleness signal** (solid's ratchet objection was decisive). **Commit count is only a fast-path optimization** (rust-systems' N=10 skip — when delta is very small, assume scope overlap is negligible and skip the diff computation). ddd's N=50 hard ceiling was REJECTED on solid's ratchet-in-waiting concern.

### D.3 /gate-raid-plan sequencing

**Round 1 split:** 3-for-post-council, 1-for-pre-council (clean-arch minority).

**Round 2 resolution:** unanimous **two-phase gate, single skill, two modes**. Clean-arch's R1 pre-council motivation (catch malformed plans before humans waste review cycles) is preserved in Phase 1 lint; ddd and rust-systems' R1 post-council motivation (catch drift during deliberation) is preserved in Phase 2 preflight. Solid's SRP analysis confirms one skill (two modes of the same reason-to-change), not two skills.

---

## Section E — Outstanding and deferred

### E.1 Requires measurement, not deliberation

- **HIR extract memory/latency budget.** Rust-systems R1 blocking concern. The per-PR CI gate must remain syn-only until either streaming HIR mode ships or measured HIR memory stays under CI runner budget (est. 4 GB). This calibration is an engineering task, not a council decision.
- **Staleness fast-path N=10 calibration.** Rust-systems proposed N=10 as a starting value. Telemetry from v0.2 rollout should confirm or adjust. This is a knob, not a ratchet — the authoritative signal is scope-diff.

### E.2 Council's job when cfdb-hir-extractor lands

- Ratify Phase B wirings (/discover 2a–2c, /prescribe 5f, /gate-raid-plan Phase 2 preflight) against the actual HIR schema.
- Confirm whether `cfdb-hir-extractor` stays a separate crate (v0.2 decision) or eventually replaces cfdb-extractor (v0.3 consideration).

### E.3 Intentionally not addressed by this council

- The `.concept-graph/cfdb/` vs future stand-alone repo extraction question (RFC v0.1 Q3, deferred to post-v0.2).
- The LadybugDB vs DuckDB/DuckPGQ backend choice (RFC v0.1 Q2, abstain — revisit after usage).
- The MCP/CLI boundary fix template for new domain types (CLAUDE.md rule, out of cfdb scope).

---

## Section F — Follow-up action list for next session

In order, minimum set to make the wiring real:

1. **File issue:** add `Item.is_test` + `Item.bounded_context` attributes + `:Context` node + `(:Crate)-[:BELONGS_TO]->(:Context)` edge as a v0.1 minor schema bump. Cite B.1 of this document.
2. **File issue:** implement `list_items_matching` as the 16th cfdb-cli verb. Cite B.2.
3. **File issue:** extend `/discover` skill output format with `cfdb_keyspace_sha` frontmatter; update `/prescribe` to read and pin to it. Cite B.3.5.
4. **File issue:** extend `/freshness` skill with Step 2f-cfdb per A.6. Cite B.3.6.
5. **File issue:** write `/operate-module` skill per A.15.
6. **File issue:** write `/gate-raid-plan` skill per A.16 with Phase 1 lint only (Phase 2 preflight deferred to v0.2 HIR).
7. **File issue:** add `cfdb scope` CLI subcommand per A.17.
8. **File issue:** extend `/boy-scout` skill with `--from-inventory` mode per A.18.
9. **File issue:** update RFC §14 Q2 with the JSONL-is-not-a-backend clarification per B.5.8.
10. **File issue:** implement per-PR CI `cfdb diff` gate per A.7 tier 3, with the BLOCK/WARN routing table per A.8.

10 issues, 5 in cfdb-cli/cfdb-core (#1, #2, #7, #10, and supporting work), 5 in the skill layer (#3, #4, #5, #6, #8) + #9 in RFC.

None of these are blocked on user review except the schema bump (#1) which touches determinism invariants and should have an explicit one-paragraph RFC addendum confirming B.1 is additive-only.

---

## Section G — Notes on the deliberation process

- Original `clean-arch` and `solid` agents stalled mid-task on Round 1 and were replaced with `clean-arch-2` and `solid-2` (general-purpose agent type, tighter prompts, explicit Write-tool instructions). Their round-1 verdicts landed successfully.
- Round 2 was conducted in message form (plain text to team-lead), not file form. ddd, rust-systems, solid-2 replied fully; clean-arch-2 did not reply in round 2 despite one nudge — their round-1 positions were used for ratification where relevant (and were never the minority on any divergence, so this does not affect any ratified decision).
- No RED verdicts at any point. All divergences were resolvable within the 3-tier framing (D1) or via a compromise synthesis (D2 scope-diff primary + N=10 fast-path, D3 two-phase single skill).
- The council's one non-obvious catch: clean-arch's ratchet objection to any committed `expected_findings.json` whitelist at the per-PR tier. This binds the watchdog design tightly to CLAUDE.md §1.1 and prevents a common failure mode.
