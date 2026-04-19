# Round 1 Synthesis — cfdb ↔ /discover, /prescribe, watchdog, missing skills

**Date:** 2026-04-14
**Team:** council-cfdb-wiring
**Participants:** ddd, rust-systems, clean-arch, solid (original clean-arch + solid were respawned as clean-arch-2 + solid-2 after stalling)
**Round 1 verdicts:**

| Member | Q1 | Q2 | Q3 |
| --- | --- | --- | --- |
| ddd | 🟡 YELLOW | 🟢 GREEN | 🟡 YELLOW |
| rust-systems | 🟡 YELLOW | 🟡 YELLOW | 🟡 YELLOW |
| clean-arch | 🟢 GREEN | 🟡 YELLOW | 🟢 GREEN |
| solid | 🟢 GREEN | 🟢 GREEN | 🟢 GREEN |

No RED. All four members converged on the same wiring shape with different thresholds for "done enough".

---

## Convergent decisions (ratified — no round 2 needed)

### Q1 — cfdb in /discover and /prescribe

1. **Pipeline shape.** `cfdb extract → (keyspace cache) → /discover → .discovery/<issue>.md → /prescribe → .prescriptions/<issue>.md → gates read the frozen artifact.` Unanimous.
2. **Discovery artifact MUST pin keyspace SHA in frontmatter.** `cfdb_keyspace_sha: <sha12>` becomes part of the `.discovery/<issue>.md` header. /prescribe's Step 5b verification queries cfdb against THAT pinned keyspace, not against a live re-extract. If the pinned keyspace no longer exists, /prescribe REFUSES to run (verdict `cfdb keyspace stale — refresh discovery`). Explicit in clean-arch; implicit in ddd, rust-systems, solid.
3. **/prescribe consumes the artifact but ALSO queries cfdb.** Step 5b is an independent verification (Council Directive #2209, "DO NOT trust the discovery artifact blindly"), not a bypass. Both agents read the same keyspace — same facts, different projections. No split fact base.
4. **/discover keeps grep fallback for two residual slices** that are not structural facts:
   - **Step 6a Decision Archaeology rationale quotes** — commit-message prose is not in the graph. cfdb gives the pointer (`:Item.git_last_commit_sha`), skill does `git log -1 <sha>` for the prose. clean-arch + solid converge.
   - **Step 1h Param Census literal arguments** — `.get("key")` string args need `:CallSite` + `RECEIVES_ARG` edges from cfdb-hir-extractor (v0.2). Pre-HIR, hand-grep. clean-arch + solid converge.
5. **Wire form: subprocess CLI, NOT Rust library dependency.** Skills are markdown + shell; cfdb is invoked via `Bash(cfdb query ...)` or (later) HTTP. No `use cfdb_core;` in any skill-adjacent Rust crate. Rust-systems explicit (Option A vs B vs C); clean-arch explicit ("no `use cfdb::*` in skill Rust code"); solid implicit via ISP/DIP.
6. **cfdb v0.1 schema has everything needed for static item census** (Steps 1a–1f of discover, Steps 5b–5e of prescribe). Syn-level, shipped.
7. **cfdb-hir-extractor (Phase B, not yet shipped) gates the high-value call-graph sections** (Steps 2a–2c of discover, Step 5f of prescribe, Pattern I of /gate-raid-plan checks 3/4/5). Rust-systems + solid converge. Integration proceeds in two phases: Phase A wiring now, Phase B wiring after cfdb-hir-extractor lands.
8. **One new typed convenience verb.** Three of four members propose a variant. Names diverge (`list_items_matching` / `list_definitions_of` / `list_context_owner`); the common intent is *"typed composition over query_raw returning items by name pattern, grouped by bounded context"*. Solid abstains ("not needed — per-skill `.cypher` files are fine"), but doesn't object. **Round-2 decision:** single 16th verb, name and signature TBD in ratification. Zero determinism impact, syn-level.
9. **`bounded_context` belongs at the syn-level** (derived from crate-prefix convention at extraction time), with `.cfdb/concepts/<name>.toml` overrides as enrichment. Rust-systems explicit. ddd compatible (wants a Context node on top of it; see D2 below). Clean-arch + solid don't contest.

### Q2 — Permanent watchdog

1. **Five-tier model.** per-save (IDE/lefthook) → per-session (`/freshness` + `/discover`) → per-PR (CI `cfdb diff` gate) → nightly cron (full extract, RESCUE-STATUS refresh) → weekly cron (delta report). All four members agree.
2. **`/freshness` owns cfdb staleness detection.** It is the existing owner of context-package freshness; cfdb keyspace staleness is an additive attribute on that verdict, not a new owner. ddd + rust-systems + clean-arch + solid converge.
3. **CI never auto-applies remediation.** The drift gate annotates PR with a routing recommendation (`/boy-scout --from-inventory`, `/operate-module <context>`); a subsequent `/work-issue` session reads the annotation and acts. Preserves §4 invariant "cfdb never modifies Rust files".
4. **BLOCK-vs-WARN CI routing.**

   | Class | Verdict | Skill route |
   | --- | --- | --- |
   | `context_homonym` | BLOCK (always — needs council) | `/operate-module` |
   | `canonical_bypass` | BLOCK (new only — existing warn) | `/sweep-epic` |
   | `duplicated_feature` | WARN | `/sweep-epic` |
   | `unfinished_refactor` | WARN | `/sweep-epic --mode=port` |
   | `random_scattering` | WARN | `/boy-scout --from-inventory` |
   | `unwired` | WARN | `/boy-scout` (delete) or issue-owner session (wire) |
   | existing Pattern D/E ban violation introduced by PR | BLOCK | inline fix |

5. **No metric baselines, no ratchet files, no per-finding allowlists.** Any "expected findings whitelist" is a ratchet by CLAUDE.md §1.1. A false positive is fixed by editing the Cypher rule in a reviewed PR against the whole fact base, not by waiving it locally. Clean-arch explicit.
6. **`.ldb` / backend files are rebuildable cache, gitignored.** §10.1 of RFC v0.1 is unanimous.
7. **Canonical JSONL dump per §12.1 is committed** when it serves as a test fixture or drift baseline. That's the published language; the backend file is the cache.

### Q3 — Missing skills

1. **`/operate-module`: 2 responsibilities, not 4.**
   - (1) Evaluate §A3.2 thresholds against a pre-built inventory. (2) Emit raid plan markdown per §A3.3 template, or a `below_threshold` JSON verdict routing to `/boy-scout`.
   - Never runs `cfdb extract`. Never edits source. Never invokes `/sweep-epic` or `/boy-scout` directly. Never declares a Context Mapping resolution (ACL / Shared Kernel / etc.) — raises the question for council, doesn't answer it.
   - Per-context, NOT per-issue. NOT part of `/work-issue`'s standard gate sequence. Triggered by CI annotation, nightly threshold alerts, or manual invocation.
   - All four members converge on this shape.
2. **`/gate-raid-plan`: 5 Pattern I queries from RFC §3.9** (completeness, dangling-drop, hidden-callers, missing-canonical, clean/dirty-mismatch). Read-only; validates an existing raid plan against live cfdb facts. Fails if the plan's portage/dead/misplaced/canonical buckets don't reconcile with current call graph.
3. **`/gate-raid-plan` cannot ship in cfdb v0.1.** Pattern I + `:CallSite` are v0.2 (solid explicit, rust-systems converges). v0.1 has a reduced gate that runs checks 1+2 only (completeness + dangling-drop names) with a documented "hidden callers unverified" warning. Solid blocking concern; rust-systems blocking concern.
4. **`/cfdb-scope`: CLI flag, NOT a skill.** All four members converge. The operation is pure data aggregation with no judgment; wrapping it in a skill violates the RFC §4 invariant and doesn't earn its overhead. Exact flag shape is in round-2 (minor surface divergence).
5. **`/boy-scout --from-inventory`: single skill, two modes** (input substitution, not new responsibility). All four members converge. Owns `random_scattering` + `unwired (no tracker)` only.

   | Class | Owns? | Routes to |
   | --- | --- | --- |
   | `random_scattering` | ✅ YES | `/boy-scout --from-inventory` |
   | `unwired (no tracker)` | ✅ YES — delete-only | `/boy-scout --from-inventory` |
   | `unwired (tracker exists)` | ⚠️ partial — wire only if single-call-site mechanical | issue owner session |
   | `duplicated_feature` | ❌ | `/sweep-epic` |
   | `canonical_bypass` | ❌ | `/sweep-epic` |
   | `unfinished_refactor` | ❌ | `/sweep-epic --mode=port` |
   | `context_homonym` | ❌ — v0.2 Q10 unanimous | `/operate-module` (always) |

6. **Context homonym is NEVER auto-routed to `/boy-scout`.** Mechanical dedup on a homonym deletes bounded-context isolation. v0.2 addendum Q10 confirmed; all four members restate.

---

## Divergences needing round 2

Three real divergences remain. Everything else is either convergent or orthogonal.

### D1 — Inventory storage location: in-repo vs out-of-tree

| Member | Position |
| --- | --- |
| ddd | `.cfdb/inventory.json` committed in-repo; `.ldb` gitignored; `.cfdb/snapshots/` JSONL committed |
| rust-systems | `.cfdb/inventory.json` committed inside `.concept-graph/cfdb/` (sub-workspace owns it); JSONL committed; `.ldb` gitignored |
| clean-arch | Out-of-tree at `~/.cfdb/keyspaces/<project>/<sha>/` per-developer (Q5 v0.1 vote); JSONL in `.cfdb/snapshots/<sha>.jsonl.gz` but **not in repo** — too big, pollutes diffs |
| solid | Out-of-tree at `~/.cfdb/keyspaces/<project>/`; canonical commit artifact (when committed) is sorted-JSONL per §12.1 |

**The split is 2-2** between "inventory.json committed in-repo" and "out-of-tree per-developer cache".

**Convergence hypothesis (team lead):** these four positions may all be reconcilable once the council distinguishes three artifact tiers:

1. **Ephemeral keyspace / .ldb backend file** — unanimous: rebuildable cache, NOT committed, per-developer workspace.
2. **Canonical JSONL snapshot** (per §12.1) — published language, committed when it serves as a test fixture or drift baseline. This is already established in RFC v0.1 and §12.1 determinism gate.
3. **Session inventory JSON** (per-issue, derived) — this is the one in dispute. Is it an ephemeral pipe artifact between `/freshness` and `/operate-module` (clean-arch + solid, out-of-tree) or a committed session record (ddd + rust-systems, in-repo)?

**Round 2 question:** Is a session inventory JSON a committed artifact or a pipe? Vote, justify briefly.

### D2 — Who triggers inventory refresh

| Member | Position |
| --- | --- |
| ddd | Nightly cron does full refresh against develop HEAD. Sessions read committed state with a staleness warning; never block on extraction |
| rust-systems | CI runs `cfdb extract` on merge to develop; commits JSONL back. `/freshness` Step 2f runs `cfdb extract` on-demand if session HEAD has no matching keyspace |
| clean-arch | `/freshness` is the composition root for staleness — reads git state, reads cfdb state, re-extracts or blocks |
| solid | `/freshness` reads `.cfdb/current.sha`. Missing → block. Stale on scope → annotate `cfdb_stale` flag, `/discover` refuses |

**These aren't contradictory — they're layered.** The synthesis:

- **Nightly cron** (or merge-to-develop CI) produces the committed canonical snapshot for develop HEAD.
- **Per-session `/freshness`** detects staleness against current session scope (not total commit count, but scope-filtered per solid's "filtered to session scope" framing). If stale, runs `cfdb extract` on-demand for the session scope only. If the extraction fails or is out of budget, blocks.

**Round 2 question:** Confirm the layered model (cron produces baseline; /freshness refreshes per-session on demand). State the scope-staleness algorithm precisely — is it (a) `git diff HEAD cfdb_sha` filtered to session-scope files non-empty, or (b) commit count N, or (c) both?

### D3 — /gate-raid-plan sequencing: before or after council review

| Member | Position |
| --- | --- |
| ddd | plan → council → `/gate-raid-plan` → `/sweep-epic --mode=port` |
| rust-systems | plan → council → `/gate-raid-plan` → `/sweep-epic --mode=port` |
| clean-arch | plan → `/gate-raid-plan` → council → `/sweep-epic --mode=port` (gate BEFORE review, as auto-lint) |
| solid | (implicit: after council, before sweep) |

**3 for post-council, 1 for pre-council.** Clean-arch's reasoning has independent merit: a pre-flight lint catches dangling refs before humans waste cycles reviewing a broken plan.

**Synthesis hypothesis:** **two-phase gate.**

1. **Phase 1 (pre-council):** `/gate-raid-plan --mode=lint` — runs checks 1 (completeness) and 2 (dangling-drop name-level) as a fast auto-lint before council sees the plan. Fails fast on malformed plans.
2. **Phase 2 (post-council, pre-sweep):** `/gate-raid-plan --mode=preflight` — runs all 5 checks against current keyspace SHA (which may have advanced during council deliberation). Ensures nothing drifted while humans reviewed.

**Round 2 question:** Ratify two-phase gate, or pick one.

---

## Ratified blocking concerns (all members must agree)

1. **`Item.is_test` attribute not in v0.1 §7 schema.** Blocks /prescribe Step 5c test-parser divergence check. (ddd) — needs v0.1 minor schema bump, additive, zero determinism impact, zero extractor cost (`#[cfg(test)]` and `#[test]` are syntactic attributes already walked by the extractor).
2. **`(:Crate)-[:BELONGS_TO]->(:Context)` edge + `:Context` node not in v0.1 §7 schema.** Blocks /discover Steps 1b/1g and /prescribe's bounded-context test. (ddd) — rust-systems converges via a *different* solution (syn-level `bounded_context` attribute on `:Item`). **The two proposals are complementary**: syn-level attribute for per-item fast lookup; Context node for explicit cross-context aggregation and overrides. Both ship. Zero determinism impact.
3. **`list_context_owner` typed verb absent.** Blocks prescription's bounded-context test on every CREATE decision. (ddd) — partial overlap with the Q1 convergent 16th verb proposal; may collapse into it.
4. **cfdb-hir-extractor (Phase B) blocks high-value integrations:** /discover Steps 2a–2c, /prescribe Step 5f, /gate-raid-plan Pattern I checks 3/4/5. (rust-systems + solid)
5. **freshness ↔ cfdb keyspace staleness handshake underspecified in v0.2 §A4.** (clean-arch + solid)
6. **`cfdb_keyspace_sha` frontmatter absent from current /discover output format.** Discovery artifact does not yet pin a keyspace SHA, so /prescribe's Step 5b verification cannot use the same facts. (clean-arch + solid)
7. **JSONL is NOT a query backend (Plan C).** RFC v0.1 §14 Q2 currently lists "JSONL canonical dump" alongside LadybugDB primary and DuckDB/DuckPGQ plan B in a way that can be read as "JSONL is a third backend". Solid flags this as a real LSP ambiguity — JSONL is serialization/determinism/diff format, not a query engine. Needs explicit clarification.
8. **/gate-raid-plan cannot ship fully in cfdb v0.1.** Pattern I + `:CallSite` are v0.2. (rust-systems + solid) — v0.1 ships a reduced 2-check form; full 5-check form ships in v0.2.

## Ratified convergent follow-ups (no objections expected)

1. Make `bounded_context` a syn-level attribute on `:Item` derived from crate-prefix convention at extraction time. (rust-systems proposes; ddd compatible; clean-arch + solid don't contest.)
2. Add `Item.is_test` as a syn-level attribute derived from `#[cfg(test)]` and `#[test]` attributes at extraction time. (ddd proposes; unanimous cost assessment: zero.)
3. Add ONE 16th typed verb for name-pattern item lookup. Consolidate the three proposals (`list_items_matching` / `list_definitions_of` / `list_context_owner`) into one verb with an optional `--group-by-context` flag. Single wire_form test update. (Convergent across 3 members.)
4. `/discover` output format grows a `cfdb_keyspace_sha: <sha12>` frontmatter field. /prescribe reads it and pins its Step 5b queries to that SHA. If the SHA is absent or the keyspace no longer exists, both skills REFUSE to run with a clear error. (Clean-arch explicit; unanimous implication.)
5. `.cfdb/concepts/<context>.toml` declares cross-cutting crates (messenger, sizer, allocators) where crate-prefix heuristic fails to assign a bounded context. (solid flags as convergent from ddd.)
6. RFC §14 Q2 gets a clarification sentence: "JSONL is the canonical determinism/diff/snapshot format, not a query-backend plan C." (solid explicit.)
7. The sorted-JSONL canonical dump per §12.1 is the published language boundary. cfdb owns the schema; skill layer conforms. Conformist relationship, not Shared Kernel. (ddd explicit; unanimous implication.)

---

## Round 2 routing

Round-2 follow-ups go to specific members with specific questions:

| Follow-up | To | Question |
| --- | --- | --- |
| D1 — inventory storage location | all four | Artifact-tier model: keyspace (cache) vs canonical JSONL (published) vs session inventory JSON (ephemeral pipe or committed record?). Vote + one-sentence justify. |
| D2 — refresh trigger | all four | Confirm layered model (cron + /freshness on-demand). State the scope-staleness algorithm: scope-diff, commit count, or both? |
| D3 — /gate-raid-plan sequencing | all four | Ratify two-phase gate (lint pre-council + preflight post-council) or pick one phase? |
| 16th verb name | ddd + rust-systems + clean-arch | Converge on ONE verb name + signature for the name-pattern lookup. Propose in round 2; lead picks the name that wins ≥2 votes. |
| /cfdb-scope CLI shape | rust-systems + ddd + clean-arch + solid | Minor surface — lead can ratify without round 2 if a majority form exists. Proposed: `cfdb scope --context <name> [--workspace <path>] [--format json] [--output <path>]`. Any objection? |

---

## What rounds-2 is NOT deliberating

All of the following are ratified and will be written into `RATIFIED.md` after round 2 resolves D1/D2/D3:

- Subprocess CLI wire form (not Rust library dep)
- /discover keeps grep fallback for Step 6a prose and Step 1h param literals
- /freshness owns staleness detection
- CI never auto-applies remediation
- BLOCK-vs-WARN CI class routing table
- No metric baselines / ratchets / allowlists
- /operate-module 2-responsibility decomposition
- /cfdb-scope is a CLI flag not a skill
- /boy-scout --from-inventory is single skill two modes, owning random_scattering + unwired (no tracker) only
- Context homonym is never auto-routed to /boy-scout
- Canonical JSONL dump is the published language, .ldb is cache
- Phase A wiring proceeds now; Phase B wiring waits for cfdb-hir-extractor

---

**Next action:** team lead sends round-2 follow-ups to all four members in parallel. Each member responds with a single short message answering the D1/D2/D3 questions + their verb name proposal. Lead collects, writes `RATIFIED.md`, returns to user.
