# Verdict — clean-arch

## Read log
- [x] BRIEF.md
- [x] data/skill-discover.md
- [x] data/skill-prescribe.md
- [~] data/rfc-v0.1-ratified.md (grepped §4, §7, §11, §14 only)
- [~] data/rfc-v0.2-addendum.md (grepped §A1, §A3, §A4 only)
- [ ] data/skill-boy-scout.md (not required for the hex-layer lens)
- [ ] data/skill-sweep-epic.md, skill-port-epic.md, skill-freshness.md, skill-work-issue.md

---

## Q1 — cfdb in /discover and /prescribe

### Hex-layer framing

cfdb is **infrastructure** (`syn`/`ra-ap-hir` parse, filesystem walk, LadybugDB, git2). `/discover` and `/prescribe` are **application-layer orchestrators** — they compose skills, read/write artifact files, and produce decisions. The clean-arch contract is: cfdb exposes a **port-like query surface** (15 verbs), and the skills compose it without importing `syn`, `ra_ap_*`, file globs over `crates/*/src/`, or `lbug::*`.

v0.1 §4 already states the invariant: *"Not opinionated about workflows. Knows nothing about 'raids', '/prescribe', 'RFCs'. Those are consumer-side compositions."* Every proposal below must preserve that invariant.

### /discover wiring — hex-layer view

Every step of `/discover` today hand-greps source files. With cfdb in place, the hand-grep path becomes infrastructure I/O done inside cfdb (once per workspace SHA, cached), and `/discover` becomes a **pure fact-selection projection** over the graph into per-issue frozen markdown.

| Discover section | Current grep | cfdb verb(s) | Port-purity note |
|---|---|---|---|
| Step 1 Concept Inventory (1a–1c) | `rg "struct Name\|trait Name\|enum Name"` | `query_raw` over `:Item` by qname/name regex | Clean. Results are qnames + file:line — no AST leak. |
| Step 1d FromStr census | `rg "impl FromStr for <T>"` | `query_raw` over `:Item` where `kind='impl'` AND `trait='FromStr'` AND `for_type=<T>` | Clean — filter is a Cypher WHERE, not a source read. |
| Step 1e const table ancestry | `rg "const NAME:"` + literal co-occurrence | `query_raw` over `:Item` where `kind='const'` | Adequate for count; literal-overlap test still needs the skill to `Read` the file contents because cfdb `:Item` does NOT carry const initializer bodies (correctly — that would bloat the graph). **This is fine:** the skill does the I/O once the candidate qnames are known. |
| Step 1f audit-split-brain prescan | `audit-split-brain --full-workspace` | `list_bypasses(keyspace, concept)` — typed verb specifically for Pattern C. For per-concept FromStr split also `query_raw` filtering `IMPLEMENTS` edges. | Clean. `list_bypasses` is the typed composition for Pattern C. |
| Step 1g MCP/CLI bypass grep | `rg "<Type>" crates/qbot-mcp/` etc. | `list_callers(keyspace, qname)` filtered by `crate IN {qbot-mcp, *-cli}` | Clean. Callers typed verb exists. |
| Step 1h Param Census | `rg '\.get\("(\w+)"\)'` | **residual gap.** `.get("key")` is a runtime dictionary read — not a structural fact in §7. Cypher cannot see string args without the `:CallSite` + `RECEIVES_ARG` edges from v0.2 hir extractor. Before v0.2 hir lands, this step remains hand-grep. | Acceptable. Param Census is currently conditional; the skill can degrade to grep for this one step without breaking port purity. |
| Step 2 Call Chain Trace | manual reading from entry → adapter | `query_raw` over `CALLS*` edges from `:EntryPoint` — pure v0.2 hir territory | Clean when hir lands. Pre-v0.2 degrades to the current manual trace; mark as deferred. |
| Step 2b Data Flow Trace | manual param trace | hir extractor emits `RECEIVES_ARG` per v0.2 §A1.1. | Clean, v0.2. |
| Step 2c Caller Census | `grep -rn "fn_name("` | `list_callers(keyspace, qname)` | Clean. Typed verb is canonical. |
| Step 3 Decorator chains | manual `impl Trait for Wrap<T>` walk | `query_raw` over `IMPLEMENTS` edges joined on struct generic param type = same trait (Cypher pattern is expressible against §7) | Clean. |
| Step 4 Ownership Map | callers of port methods | `list_callers(keyspace, port_method_qname)` aggregated by caller crate | Clean. |
| Step 5 Scope Classification | from traces | composition of Step 2 + Step 4 results; no new verb | Clean — derivation lives entirely in the skill body. |
| Step 6a Decision Archaeology | `git log --follow -L` + commit message read | v0.2 `enrich_history` fills `:Item.git_age_days`, `git_commit_count`, `git_last_author`; `query_raw` returns these attributes. The **commit-message rationale quote** is not in the graph — the skill must still `Read` git log for that. | Clean. cfdb gives the pointer (which commit); the skill quotes it. |

**Residual cfdb-consumer I/O that is CORRECT:** `/discover` continues to `Read` source files and `Bash(git log)` for (a) verbatim comment/scope-carve-out extraction and (b) commit-message rationales. This is not a hex violation — cfdb provides structured locators (`file:line`, `git_commit_sha`), the skill projects them to markdown quotes. The graph does not need to store prose.

**Gap in v0.1 schema:** no verb for "list all definitions whose name matches pattern X" across `:Item.name` (not `qname`). The closest is `query_raw`, but the skill uses this often enough that a **typed convenience verb `list_items_matching(keyspace, name_pattern, kinds?)`** would be justified — reduces Cypher dialect leakage into skill bodies (RFC §6.2 Clean finding [CLEAN-1]). **Proposal:** add as 16th verb in v0.2. Determinism impact: none (`query_raw` already supports it; this is purely a composition helper). Schema impact: none. syn-vs-hir cost: none. Naming: **verb MUST scream structure, not use case** — `list_items_matching` is fine, `list_prescribe_creates_needing_verification` is forbidden (leaks consumer vocabulary).

### /prescribe wiring — hex-layer view

Prescription runs AFTER discovery emits `.discovery/<issue>.md`. The clean-arch question is: **does /prescribe call cfdb directly, or does it read only from the discovery artifact?**

**Verdict: /prescribe SHOULD call cfdb, but only for verification, not for primary discovery.** Discovery is the fact-selection layer (§4: "Discovery maps facts only. No prescriptions."). If /prescribe bypasses the artifact and queries cfdb fresh, the two agents can see different fact bases — exactly the split-brain the council is trying to prevent. BUT /prescribe's Step 5b mandates an **independent Resolution Census verification** (Council Directive #2209: *"DO NOT trust the discovery artifact blindly"*). That verification is load-bearing and cannot be removed. cfdb is the right substrate for it.

| Prescribe step | Current grep | cfdb verb(s) | Port-purity note |
|---|---|---|---|
| Step 5b Independent Resolution Census | `rg "impl FromStr for <T>"` + `rg "fn parse_<x>"` | `query_raw` (same shape as discover 1d) OR new `list_items_matching`  | Clean. Called a second time against the same keyspace as discovery — deterministic per G1/G5. |
| Step 5c Test-parser divergence | `rg "fn <name>" crates/*/tests/` | `query_raw` filtered on `file LIKE '%/tests/%'` — test items are already in the graph | Clean. |
| Step 5d Cross-crate name collision | `rg "pub (struct\|enum\|type) <Name>"` | `query_raw` over `:Item` where `name=<Name>` AND `visibility='pub'` grouped by crate | Clean. |
| Step 5e Market-phenomenon heuristic | `rg "(Imbalance\|Swing\|...)" crates/domain-market-structure/` | `query_raw` filtered by `name ~ <pattern>` AND `crate='domain-market-structure'` | Clean. |
| Step 5f MCP/CLI EXTEND scan | `rg "<Type>" crates/qbot-mcp/` etc. | `list_callers(qname)` filtered by consumer crate | Clean. |
| Step 3 decision tree branches | reading the discovery artifact | **no new cfdb call** — reads `.discovery/<issue>.md` | Clean. Prescription's primary input is the frozen artifact, not live cfdb. |
| Param Census REGISTER decisions | reading discovery | reads `.discovery/<issue>.md` — no cfdb | Clean. |

**Port-purity violation risk:** if /prescribe's Step 5b quietly replaces the discovery artifact's Global Resolution Census with a fresh cfdb query and the two disagree, the prescription is operating on a different fact base than the audit agents that verify gates. **Mitigation:** Step 5b must query the **same keyspace SHA** the discovery artifact recorded in its header. Discovery must emit `cfdb_keyspace_sha: <sha>` in the artifact frontmatter; prescription reads that, passes it to `query_raw`, and if the keyspace no longer exists (e.g., develop moved and the old snapshot was dropped), prescription REFUSES to run with verdict `cfdb keyspace stale — refresh discovery`.

**Dependency direction:** `/discover` → cfdb, `/prescribe` → cfdb + `/discover`'s artifact. cfdb NEVER imports any skill SDK, NEVER reads `.discovery/` or `.prescriptions/`, NEVER parses frontmatter. The arrow points one way. ✅

**Composition root for Q1:** the skill body is the composition root. There is no "cfdb adapter crate" for skills — the skills invoke the `cfdb` CLI (`Bash(cfdb query ...)`) or the HTTP endpoint. No `use cfdb::*;` in skill Rust code because skills are markdown + shell, not Rust.

### Verdict on Q1
**GREEN** — cfdb maps cleanly to discovery/prescription with existing typed verbs + `query_raw`; one small 16th verb (`list_items_matching`) would reduce Cypher dialect in skill bodies but is not blocking. Port purity is preserved as long as (a) Step 5b uses the discovery-pinned keyspace SHA, and (b) no verb leaks consumer vocabulary in its name.

---

## Q2 — Permanent watchdog

### Composition roots per tier

Each tier has a distinct composition root OUTSIDE cfdb core.

| Tier | Trigger | Composition root | Runs | Writes | Blocks |
|---|---|---|---|---|---|
| **per-save** | file watcher / git pre-commit hook | `.githooks/pre-commit` (or `lefthook.yml`) — calls `cfdb extract --incremental` on touched crate only | incremental extract + `query_raw` on 1–2 local ban rules (e.g., no new FromStr for existing domain type) | stderr to terminal | YES — commit blocked on rule hit |
| **per-session** | `/freshness` → `/discover` → `/prescribe` invocation chain | skill body (`/freshness` caller) — runs `cfdb extract` if keyspace older than N commits | `cfdb query_raw` for discover/prescribe | `.context/<issue>.md` with `cfdb_keyspace_sha` frontmatter | YES — discover refuses if keyspace stale and refresh fails |
| **per-PR** | CI workflow (`.gitea/workflows/cfdb-drift.yml` or equivalent) | CI YAML — no cfdb code knowledge | `cfdb extract base_sha + head_sha` → `diff base head kinds=*` | PR comment + JSON artifact | YES — PR blocked on new Pattern A/B/C findings not whitelisted (whitelist is per-rule `expected_findings.json`, NOT a metric baseline per §1.1 project rule) |
| **nightly** | systemd timer or cron on builder host | `systemd-timer` / `cron.d` entry | `cfdb extract develop` → full ruleset → populate `.concept-graph/RESCUE-STATUS.md` | commit to develop (bot) | NO — report only |
| **weekly** | existing "weekly audit cron" (RFC §11) | cron wrapper script | full audit + `/audit-weekly` invocation (already exists as a skill) | `.audits/_weekly/YYYY-WW.md` | NO |

**None of these roots lives inside cfdb core.** cfdb is called from outside. cfdb core does not know there are tiers. ✅

**Clean-arch concern at the per-PR tier:** the drift-gate script MUST NOT hand-write a whitelist file that tells the tool "ignore these findings." This is the v0.1 §1.1 project rule against metric ratchets and the §6 rule 7 against hand-forged tool artifacts. The PR gate must either **pass** (zero new findings) or **fail** (any new findings, hand-fix required). An "allowlist" is a ratchet. The correct escape hatch is: if a finding is a false positive, fix the cfdb extractor or the Cypher rule in a reviewed PR that argues the change against the whole fact base. Do NOT add per-finding waivers.

### Inventory lifecycle — hex-layer view

**Where it lives:** `.cfdb/keyspaces/<sha>/` (backend cache) + `.cfdb/snapshots/<sha>.jsonl.gz` (canonical artifact, §12.1). Per-workspace, NOT in the repo (too big, would pollute diffs). Per-developer local (Q5 v0.1 vote). CI keyspaces are ephemeral per-job.

**Does cfdb know about skills or PR state?** No. cfdb stores keyspaces by `(workspace_sha, schema_version)`. It does not know "this keyspace was used by /prescribe session X" or "this snapshot was taken at PR #123 head." That metadata lives in the consumer tiers (`.context/<issue>.md` frontmatter carries `cfdb_keyspace_sha`; CI carries `base_sha`, `head_sha`). ✅ port purity preserved.

**Staleness detection:** the discovery agent compares `cfdb_keyspace_sha` in the context package against `git rev-parse develop`. If develop has moved N commits since the keyspace was extracted, `/freshness` re-extracts before `/discover` runs. Concretely: `/freshness` is the composition root for staleness — it reads git state, it reads cfdb state, it decides. Neither side knows about the other except via explicit SHAs.

**Invalidation contract with /freshness:** /freshness already carries a verdict field (`current` / `contested` / `obsolete` / `abort`). Extend its precondition checks with **"cfdb keyspace for <sha> exists and matches schema_version"**. If not, trigger re-extract before setting `verdict=current`. This keeps the invalidation logic inside the application layer (the skill), not inside cfdb.

### CI failure modes

- **Fail the PR:** new Pattern A/B/C finding on files touched by the PR, or new `:EntryPoint` registered that doesn't appear in the MCP/CLI canonical parser grep (SB-10 rule from CLAUDE.md §7).
- **Warn (comment only):** existing findings in touched files that the PR doesn't fix (boy-scout-adjacent, routed to follow-up issue).
- **Auto-route:** a single finding → comment with `/boy-scout --from-inventory <finding-id>` hint. A cluster exceeding §A3.2 thresholds → comment with `/operate-module <context>` hint. The CI does NOT auto-invoke either skill — it points the human reviewer at the right tool.

### Verdict on Q2
**YELLOW** — architecture is coherent and port-pure, but **the invalidation contract between /freshness and cfdb is underspecified in the v0.2 addendum.** §A4.2 says `RESCUE-STATUS.md` is refreshed "on every merge to develop plus weekly cron," but says nothing about how a *feature-branch* session knows its cfdb snapshot is usable. Without a skill-level `cfdb_keyspace_sha` handshake, sessions will either (a) re-extract every single time (expensive) or (b) silently use stale data (wrong). Needs one addendum bullet resolving this before v0.2 ships.

---

## Q3 — Missing skills

### /operate-module

- **Composition root:** `~/.claude/commands/operate-module.md` (skill file) — invoked by `/work-issue` or manually by the user. Not inside cfdb.
- **Hex-layer concerns:** clean as long as the skill does exactly two things (per v0.2 §A3.4): evaluate threshold + emit raid plan. It does NOT run cfdb extract (separate skill call precedes), does NOT execute boy-scout, does NOT execute portage. One-way data flow: `inventory.json → raid-plan.md`.
- **One-paragraph spec essence:** inputs = `<context-name>` + path to structured cfdb inventory JSON (§A3.3 shape); output = `raid-plan-<context>.md` in `.concept-graph/raid-plans/` OR a `below-threshold` verdict routing to `/boy-scout`. Invariant: the skill reads **only** the JSON inventory — it must not grep sources, must not call cfdb directly, must not read git log. All signals come from the precomputed inventory. This is the hex-pure form; any cfdb call inside `/operate-module` is a layering violation because `/cfdb-scope` (below) owns that step.

### /gate-raid-plan

- **Composition root:** `~/.claude/commands/gate-raid-plan.md` (skill file). Invoked between raid plan emission and council approval — validates the plan against the live fact base before a human reviews it.
- **Hex-layer concerns:** the skill runs the 5 Pattern I Cypher queries from parent RFC §3.9 (completeness, dangling-drop, hidden-callers, missing-canonical, clean/dirty-mismatch) via `query_raw` against a current keyspace. Output is a pass/fail verdict with specific dangling references. Clean — queries are consumer-side compositions, `query_raw` is the right verb.
- **One-paragraph spec:** inputs = path to `raid-plan-<context>.md` + workspace path; output = verdict `valid` / `invalid` + list of dangling references. Fails if any drop bucket contains items still referenced from outside the portage scope, any portage bucket contains items that depend on drop bucket items, or any canonical candidate has callers not in the rewire bucket. Runs BEFORE council review. The 5 queries are compiled into the skill body as Cypher string constants — no code generation, no new verb.

### /cfdb-scope

- **Resolution: CLI flag, NOT a skill.** The operation is "aggregate cfdb facts for a bounded-context into a single JSON inventory matching the §A3.3 shape." That is data aggregation — one `query_raw` composing 5–6 context-scoped queries into a merged JSON envelope. It has zero decision logic, zero orchestration, zero artifact management. Per clean-arch dependency-direction rule: **if the operation is pure data aggregation with no branching, it belongs inside the tool, not in a skill wrapper.** Skills are for composing tools and writing artifacts; cli flags are for typed data projections.
- **Flag shape:** `cfdb scope --context <name> [--workspace <path>] [--output json]` — emits the §A3.3 JSON envelope on stdout. Reuses the existing `enrich_bounded_context` attribute; filters all per-class queries by `bounded_context=<name>`. This is effectively a 16th composition verb (like `find_canonical` or `list_callers`) — it is typed, it is a convenience composer on `query_raw`, it fits RFC §6.2's composition-verb pattern. Determinism impact: none (pure read). Schema impact: none.
- **Why not a skill:** a skill would wrap one CLI call in markdown, add no value, create indirection, and risk drifting from the cli flag over time. Same-decision split-brain per prescribe's criterion — reject CREATE.

### /boy-scout --from-inventory

- **Resolution: single skill, two modes.** `/boy-scout` is already scoped around the "frozen doughnut ring of fixes on or near touched files" discipline. The inventory-driven mode is a **different input source** for the same routine — instead of `boy-scout-scope` binary deriving a ring from git diff, an inventory JSON specifies the items directly. Everything downstream (the 15-min fix rule, the commit format, the quality-metrics checkpoint) is unchanged.
- **Clean-arch justification for one skill:** the change vector is the input protocol, not the behavior. SRP says "one reason to change" — boy-scout's reason to change is "new mechanical debt class worth fixing inline," which is the same regardless of how the debt was found. A sibling skill would duplicate the 15-min rule, the doughnut discipline, the quality-metrics hook. Same-decision = reuse.
- **Class filter to the 6-class taxonomy:** `/boy-scout` owns classes **4 (Random scattering)** and **6 (Unwired logic, delete variant)** — the two classes whose fix strategy is "mechanical, no council required, no architectural judgment." It does NOT own classes 1 (Duplicated feature — requires canonical-head choice), 2 (Context Homonym — requires context mapping decision), 3 (Unfinished refactoring — requires knowing the migration target), or 5 (Canonical bypass — requires port-chain knowledge). Those route to `/operate-module` or to full `/work-issue` with `/prescribe`. Class 6's "wire" variant (not delete) also routes out — wiring requires knowing the entry point, which is a prescribe concern.
- **Input contract when handed to boy-scout:** a JSON file with `{findings: [{id, class, file, line, qname, fix_action}], scope_ring_sha: <sha>}` where `class ∈ {random_scattering, unwired}` and `fix_action ∈ {extract_helper, delete_dead_item}`. Boy-scout REFUSES to run if any finding has a class outside its ownership — that is a routing bug in the caller, not a boy-scout responsibility.

### Verdict on Q3
**GREEN** — all four artifacts resolve cleanly under hex-arch rules: `/operate-module` is two-responsibility (threshold + emit), `/gate-raid-plan` is pure query composition, `/cfdb-scope` is a cli flag not a skill (data aggregation ≠ orchestration), `/boy-scout --from-inventory` is one skill with two input modes (same fix discipline).

---

## Blocking concerns

- **Keyspace staleness handshake is missing from v0.2 §A4.** /freshness must know how to detect and refresh a stale cfdb keyspace before /discover runs. Without this, either every session re-extracts (cost) or sessions silently use stale facts (correctness).
- **No verb currently returns `:Item` filtered by name pattern as a typed convenience.** Skills will inline Cypher fragments for the most common operation. Add `list_items_matching(keyspace, name_pattern, kinds?)` as a 16th verb — low cost, high clarity, preserves "no Cypher in skill bodies" invariant from RFC §6.2 [CLEAN-1].

## Convergent follow-ups

- Any verdict that proposes cfdb emit pre-formatted markdown fragments (raid plans, prescriptions, discovery sections) is wrong by §4 — flag and correct. cfdb returns JSON; consumers format.
- Any verdict that proposes `/discover` skip the `.discovery/<issue>.md` artifact and let `/prescribe` query cfdb directly breaks the gate-verification chain. Gates read the artifact; live-query would make gate verdicts non-reproducible across a keyspace refresh. Reject.
- If the rust-systems verdict argues for a direct `use cfdb_core;` Rust dependency from any skill, that is a dependency-direction violation — skills are markdown, not crates. cfdb is reached via CLI or HTTP. Flag.
