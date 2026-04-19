# Council Brief — cfdb ↔ /discover & /prescribe Wiring

**Convened:** 2026-04-14
**Team:** `council-cfdb-wiring`
**Forcing question:** RFC-029 v0.2 addendum §11 "Integration points" names `/prescribe` as a cfdb consumer but **never mentions `/discover`**. This is a real architectural gap — not a drafting oversight. The council failed to see it during the v0.2 deliberation. Your job is to repair it.

---

## 🚨 READ BEFORE YOU THINK

The data directory next to this brief contains **everything** you need. Read it in full before drafting your verdict. Do **not** try to answer from prior context or from your training distribution — some of the content below contradicts what a general-purpose LLM would guess (e.g. no Prometheus, no testcontainers, no baselines).

**Mandatory reads (in this order):**

1. `data/rfc-v0.1-ratified.md` — 895 lines. RFC-029 v0.1 as ratified 2026-04-13 (open-newtype schema blessed). §3 = 9 problem patterns. §4 = non-goals and invariants ("not opinionated about workflows", "not a prompt builder"). §7 = schema. §11 = integration points. §14 = council Q1–Q7 verdicts.
2. `data/rfc-v0.2-addendum.md` — 606 lines. The v0.2 addendum with the vertical split-brain, debt taxonomy (6 classes), rescue mission protocol (§A3), Q9–Q14 verdicts. Particularly §A2 (classifier), §A3 (operate-module outline), §A4 (doctrine bootstrap), §A5 (recall protocol). **Note the gap you are being convened to fix: `/discover` appears ZERO times in this file.**
3. `data/skill-discover.md` — 794 lines. The `/discover` skill as it exists today. Grep/read-based archaeology, producing `.discovery/<issue>.md`. **This is the skill the RFC v0.2 forgot to integrate.** Note the Global Resolution Census (Steps 1d–1e), the Data Flow Trace (Step 2b), the Decision Archaeology (Step 6a), and the Doctrinal Context (Step 0b).
4. `data/skill-prescribe.md` — 531 lines. The `/prescribe` skill. Consumes the discovery artifact. Produces `.prescriptions/<issue>.md` with REUSE / EXTEND / CREATE / DEFER decisions + WIRING ASSERTIONS + Canonical Ownership + Forbidden Creations + MCP/CLI EXTEND Scan. The `DEFER` decision type and the `Forbidden Creations` negative enumeration are both load-bearing anti-CREATE-bias mechanisms you must not break.
5. `data/skill-boy-scout.md` — 284 lines. The `/boy-scout` skill as it exists: file-proximity scoped via `boy-scout-scope` binary, fixes pre-existing mechanical violations in a frozen doughnut ring. **This skill you may propose extending with a `--from-inventory` mode that reads a cfdb-filtered finding set.**
6. `data/skill-sweep-epic.md` — 364 lines. `/sweep-epic` fans out mechanical refactors in parallel against an integration worktree. Per v0.2 Q14, `/port-epic` is rejected as a new skill; sweep-epic grows a `--mode=port --raid-plan=<path>` variant.
7. `data/skill-port-epic.md` — 460 lines. The existing port-epic skill (archaeology + move-not-copy discipline). Read to understand what v0.2's `--mode=port` is replacing or refining.
8. `data/skill-freshness.md` — 340 lines. Produces `.context/<issue>.md` BEFORE discover/prescribe run. Establishes the verdict (`current` / `contested` / `obsolete` / `abort`), scope overrides, forbidden moves. Feeds the prescription.
9. `data/skill-work-issue.md` — 681 lines. The full pipeline orchestrator that calls freshness → discover → prescribe → gates → audit → quality → verify → ship. **Your wiring proposals must fit inside this orchestration.**

**Sources-of-truth outside the data dir (read if your verdict depends on them):**

- `~/.claude/CLAUDE.md` §4 (Task Framing Protocol — discover/prescribe mandatory steps) and §7 (Quality & Shipping).
- `/var/mnt/workspaces/qbot-core/CLAUDE.md` (project rules: no Prometheus, Decimal-everywhere, RFC-018 Postgres test pattern, BDD lint rules, Param Registration Checklist).
- Current on-disk cfdb source under `.concept-graph/cfdb/` — **NOTE: this worktree is `docs/rfc-022-mcp-runtime-extraction` branched from an older develop. cfdb is on develop (PR #3690 merged fb288258d). If you want to see the actual CLI shape, run `git show fb288258d -- '.concept-graph/cfdb/crates/cfdb-cli/src/main.rs'`.**

---

## Background: what you are deliberating about

### 1. The gap (confirmed by grep)

```
$ git show <RFC v0.1>:.concept-graph/RFC-cfdb.md | grep -c "/prescribe"
8
$ git show <RFC v0.1>:.concept-graph/RFC-cfdb.md | grep -c "/discover"
0
$ git show <v0.2 addendum>:.concept-graph/RFC-cfdb-v0.2-addendum-draft.md | grep -c "/discover"
0
```

v0.1 §11 (Integration points) names `/prescribe`, `/prepare-issue`, `/quality-architecture`, `/port-epic`, `/boy-scout`, ad-hoc agents, in-repo architecture tests, `/gate-raid-plan` (v0.2 new), `/operate-module` (v0.2 new), weekly audit cron, drift-at-PR gate. It does **not** name `/discover`.

### 2. Why it's load-bearing (and not a cosmetic omission)

`/discover`'s job per CLAUDE.md §4 Step 0 is *literally* what cfdb's §7 schema models: entry points, CALLS* traversal, decorator composition, port ownership, Global Resolution Census, Data Flow Trace, Decision Archaeology. Today `/discover` hand-greps for all of this. With cfdb in place it could query — faster, typed, complete.

More critically: **`/discover` is the fact-selection layer that turns workspace-wide cfdb data into a per-issue frozen artifact** (`.discovery/<issue>.md`). Gate agents read that artifact. If cfdb feeds `/prescribe` directly (as v0.2's implicit shape suggests), you lose:

- The `.discovery/<issue>.md` artifact that gates verify against (`/discover` skill body: "gate agents verify implementation against it")
- The fact-only discipline enforced by a separate agent (§4: "Discovery maps facts only. No prescriptions, no 'should', no 'needs to change.'") — a `/prescribe` that pulls cfdb directly blurs the facts/decisions boundary
- The scope classification derived from the trace (§4: "Scope is NOT classified by intuition — it is DERIVED from the call chain trace")

The correct pipeline is:

```
source.rs → cfdb extract → .cfdb/inventory.json (workspace-wide)
                              ↓
                          /discover  ← issue-scoped cfdb query + format to .discovery/<issue>.md
                              ↓
                          /prescribe ← REUSE/CREATE/EXTEND/DEFER decisions, reads .context/ + .discovery/
                              ↓
                          gates read both artifacts to verify implementation
```

`/discover` becomes the **session-scoped, issue-frozen read model** over cfdb. `/prescribe` is unchanged in role — it still reads `.discovery/<issue>.md` + `.context/<issue>.md` + applies the decision criterion.

### 3. The user's three questions (your deliverable)

**Q1 — Smart ways to use cfdb (and extend it if necessary) in `/discover` AND `/prescribe` so that prescription stops producing new split-brains.**

Not just "which verbs do they call". Concretely:

- Which of the 15 shipped verbs does `/discover` need to call to produce the current census / trace / archaeology sections? Map each discover section to a query.
- Which of the 15 shipped verbs does `/prescribe` need to verify CREATE decisions aren't split-brains (Step 5b Resolution Census, Step 5c Test-Parser Divergence, Step 5d Cross-Crate Name Collision, Step 5e Market-Phenomenon Heuristic, Step 5f MCP/CLI EXTEND Scan)?
- Are those verbs sufficient? Is there a missing verb — e.g., `list-concept-owners`, `list-definitions-of`, `query-by-name` — that the skill needs?
- Does the schema (§7, v0.1 subset: Crate/Module/File/Item/Field/CallSite) have the node/edge types required to answer prescription's questions? What's missing? Can it be added without breaking the frozen v0.1 determinism invariants (§12.1)?
- The v0.2 addendum already proposes `cfdb-hir-extractor` as a parallel crate for variant/param/entry-point emission. Does it subsume everything `/prescribe` needs, or is there a residual gap?

**Q2 — Define the integrations as a permanent watchdog mechanism.**

Not one-shot cleanup; continuous enforcement. Concretely:

- What runs **per session** (main agent invokes via skill)?
- What runs **per PR** (CI gate before merge)?
- What runs **nightly / weekly** (cron)?
- What runs **on every file save** (IDE / watchman / lefthook)?
- How does each tier produce a signal the next tier acts on? (e.g., nightly cfdb extract → JSONL delta → drift-at-PR query → block or comment on PR)
- Where does the watchdog's state live? (`.cfdb/inventory.json` in the repo? out-of-tree in `~/.cfdb/<project>/`? a branch?)
- What is the **invalidation story**: when cfdb's inventory is stale (develop moved), does `/discover` block, refresh, or proceed with a warning? This is the same class of problem `/freshness` already solves for `.context/<issue>.md`.
- How does the watchdog compose with `/work-issue` so that a session cannot reach `/gate-intent` if the inventory is >N commits stale?
- What is the **CI failure mode** when `cfdb query` finds a new split-brain introduced by the PR? (Block? Comment? Auto-file issue?)

**Q3 — Define missing details for the new skills.**

The v0.2 addendum mentions four skill-level artifacts that don't yet exist on disk. You must specify each well enough that a next-session can write them:

- **`/operate-module`** — v0.2 §A3.4 sketch exists. Your job: turn it into a skill file with inputs, outputs, invariants, failure modes, invocation protocol, relationship to `/discover` + `/prescribe` + `/gate-raid-plan` + `/sweep-epic --mode=port`. 2 responsibilities (threshold check + raid plan emission), not 4.
- **`/gate-raid-plan`** — v0.1 §11 mentions it as "Pattern I plan validation" (bounded-context raid plan validation via `query_with_input`). Spec it: inputs, what it asserts, what it returns, where in the pipeline it runs.
- **`/cfdb-scope`** — open question whether this even needs to be a skill vs. a cfdb-cli flag. Resolve it. If a skill, spec it; if a cli flag, specify the flag.
- **`/boy-scout` extension `--from-inventory`** — how does inventory-driven triage coexist with file-proximity triage? Single skill two modes, or sibling skill? How does the class filter map to the 6 v0.2 debt classes? Which classes does boy-scout own?

---

## Deliverable format

Each member writes **one** markdown file at `.context/council-cfdb-wiring/verdicts/<role>.md`. Use the skeleton below. Members do not edit each other's files — convergence happens in a second round orchestrated by the team lead after all individual verdicts land.

```markdown
# Verdict — <role>

## Read log
- [x] data/rfc-v0.1-ratified.md
- [x] data/rfc-v0.2-addendum.md
- [x] data/skill-discover.md
- ...
(tick every file you actually read; un-ticked items mean "answered without reading")

## Q1 — cfdb in /discover and /prescribe

### /discover wiring
- Section → cfdb verb mapping (table)
- Gaps: verbs or schema nodes/edges missing (justify with a prescription Step it unblocks)
- Extension proposals: phrased as "add verb X returning Y" or "add node type Z to §7 schema"
  - Each proposal includes: determinism impact (does it violate §12.1 invariants?), syn-vs-hir cost, schema encoding (open-newtype per #3670)

### /prescribe wiring
- Same mapping for Steps 5b, 5c, 5d, 5e, 5f + the Decision Tree at Step 3
- Verify each prescription anti-split-brain check has a cfdb query backing it
- Gaps + extensions, same format as /discover section

### Verdict on Q1
`GREEN` | `YELLOW` | `RED` with one sentence explaining.

## Q2 — Permanent watchdog

### Tier table
| Tier | Trigger | Runs | Writes | Blocks |
| --- | --- | --- | --- | --- |

(fill in per-save, per-session, per-PR, nightly, weekly)

### Inventory lifecycle
- Where it lives
- Who refreshes it
- Staleness detection
- Invalidation contract with /freshness

### CI failure modes
- What makes a PR fail
- What makes a PR warn
- How auto-remediation routes (`/boy-scout`, `/operate-module`, `/sweep-epic --mode=port`)

### Verdict on Q2
`GREEN` | `YELLOW` | `RED`

## Q3 — Missing skills

### /operate-module
- Description (one line, skill frontmatter style)
- Arguments, inputs, outputs
- Invariants (what it must never do)
- Invocation protocol (how /work-issue decides to invoke)
- Relationship to /discover, /prescribe, /gate-raid-plan, /sweep-epic --mode=port
- Failure modes and escape hatches

### /gate-raid-plan
- Same structure

### /cfdb-scope
- Resolve: skill or cli flag? Justify.
- If skill: full spec.
- If flag: argument shape + cli semantics.

### /boy-scout --from-inventory
- Resolve: single skill two modes, or sibling? Justify.
- Class filter to 6-class taxonomy (state which classes boy-scout owns; justify why the others are out of scope)
- Input contract (what does the inventory file look like when handed to boy-scout?)

### Verdict on Q3
`GREEN` | `YELLOW` | `RED`

## Blocking concerns
(if any — one per line, terse)

## Convergent follow-ups
(things you'd happily fold in from another member's verdict without a second deliberation)
```

---

## Rules of engagement

1. **No self-quoting from this BRIEF.** Cite the source files in `data/`. If the BRIEF summarizes something incorrectly, correct it using the data and flag the delta.
2. **No hallucinated skills.** If a skill doesn't exist on disk, say so. The list of existing skills is in `/var/home/yg/.claude/commands/` — enumerable via `ls`.
3. **No hallucinated cfdb verbs.** The 15 verbs are enumerated in RFC §6.2. If you propose a 16th, flag it as a new verb and specify cost.
4. **You are not the coder.** No implementation code in verdicts. Specifications, decisions, shapes, contracts.
5. **Disagree with the BRIEF if warranted.** If you think `/discover` shouldn't integrate with cfdb (retire it instead, option 2 from the main-agent analysis), argue it. The council's job is not to rubber-stamp the main agent's framing.
6. **Two-pass convergence.** Round 1 = individual verdicts written independently; round 2 = team lead collects, spots divergences, sends focused follow-ups to specific members. Do not peek at siblings' verdicts in round 1.
7. **Minimum table stakes:** every verdict answers all 3 questions and gives a per-question GREEN/YELLOW/RED. No skipping a question because another specialist will handle it.

---

## Output locations

- Individual verdicts: `.context/council-cfdb-wiring/verdicts/<role>.md`
- Team lead's round-1 synthesis: `.context/council-cfdb-wiring/SYNTHESIS-R1.md`
- Round-2 follow-ups (lead → members): in-thread messages
- Final ratified output: `.context/council-cfdb-wiring/RATIFIED.md` with one table of convergent decisions + a short list of surgical follow-ups for the user

---

## Roster

| Role | Agent type | Primary lens |
| --- | --- | --- |
| clean-arch | `clean-arch` | Hex arch, port purity, composition roots, dependency direction, screaming architecture. cfdb must not leak infra types into consumer skills. |
| ddd | `ddd-specialist` | Bounded contexts, homonym detection, context mapping, aggregate boundaries. Prevent new homonyms introduced by the wiring. |
| solid | `solid-architect` | SRP, OCP, ISP, LSP, DIP. Can `/discover` and `/prescribe` maintain their single responsibilities when backed by cfdb? Is the query surface an ISP-compliant slice per consumer? |
| rust-systems | `rust-systems` | Crate boundaries, Cargo dependency graph, compile-time cost, syn vs ra-ap-hir budget, orphan rules, determinism invariants. New verbs cost what? |

No quant-trader — this is methodology, not trading.
