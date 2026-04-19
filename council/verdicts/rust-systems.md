# Verdict — rust-systems

## Read log
- [x] data/rfc-v0.1-ratified.md
- [x] data/rfc-v0.2-addendum.md
- [x] data/skill-discover.md
- [x] data/skill-prescribe.md
- [x] data/skill-boy-scout.md
- [x] data/skill-sweep-epic.md
- [x] data/skill-port-epic.md (read partially — BRIEF states v0.2 explicitly kills this as a new skill; variant flag confirmed)
- [x] data/skill-freshness.md
- [x] data/skill-work-issue.md
- [x] BRIEF.md

---

## Q1 — cfdb in /discover and /prescribe

### Cargo coupling model (framing constraint)

Before mapping verbs, the coupling model must be resolved because it affects every answer below.

**Three options:**

| Option | Mechanism | Cargo coupling to qbot-core | Acceptable? |
|---|---|---|---|
| A | `cfdb query …` as subprocess via `Bash` tool | Zero — subprocess, no crate dep | YES — skill runs cfdb like any CLI tool |
| B | Rust lib dependency (`use cfdb_core::query`) | Pulls cfdb-core (and transitively cfdb-store-lbug, lbug FFI cxx, lbug itself) into qbot-core build graph | NO — cfdb is a sub-workspace at `.concept-graph/cfdb/`; it is NOT a member of the qbot-core workspace Cargo.toml. Making it a dependency would require either extracting cfdb to a published crate or a `path = "…"` dep pointing into `.concept-graph/`. Either breaks the §8.1 "sub-workspace" isolation decision. |
| C | HTTP (`POST /v1/query`) against a warm cfdb-server | Zero compile coupling; requires cfdb-server running | Viable for latency-sensitive paths; requires process management |

**Verdict on coupling model: Option A (subprocess CLI) is the correct form for skill integration.** The skills are Claude agent skills, not Rust binaries — they invoke tools via shell commands. Option B is an architectural violation (the sub-workspace isolation decision in RFC §8.1 Q3 was explicitly "in-tree now, extract later" with separate Cargo.toml). Option C is viable for `/gate-raid-plan` which needs per-PR warm-process latency; for `/discover` and `/prescribe` which run at session start (cold), Option A is simpler and sufficient.

**No new Cargo.toml changes are needed in qbot-core workspace to integrate cfdb with /discover and /prescribe.** The only coupling is a PATH reference to the `cfdb` binary in the skill's `allowed-tools` list.

### /discover wiring

**Current /discover workflow (grep/file-based, what cfdb replaces):**

| /discover Step | Current implementation | cfdb verb that replaces/augments |
|---|---|---|
| Step 1a — concept search | `rg "struct <Name>"` etc. across workspace | `cfdb query` — `MATCH (i:Item) WHERE i.name =~ '(?i)<name>'` returns file:line with zero-grep overhead |
| Step 1b — classify match (PORT/DOMAIN/ADAPTER) | Read file, infer from path | `cfdb query` — `i.crate` attribute encodes which crate; layer is derivable from crate prefix convention (§A3.2) |
| Step 1c — concept inventory | Manual grep compilation | `cfdb query` — structured rows, no manual assembly |
| Step 1d — FromStr/Display audit | `rg "impl\s+FromStr\s+for\s+<Type>"` | `cfdb query` — `MATCH (i:Item {kind:'ImplBlock'}) WHERE i.qname CONTAINS 'FromStr'` — faster, typed, cross-crate |
| Step 1e — Const table ancestry | `rg "const\s+<NAME>"` + co-occurrence | `cfdb query` — `MATCH (i:Item {kind:'Const'}) WHERE i.name = '<NAME>'` |
| Step 1f — split-brain pre-scan | `audit-split-brain --full-workspace` (existing binary) | `cfdb query` with HSB `.cypher` rule (Pattern A) — MORE PRECISE than the audit-split-brain binary because it uses structural similarity signals, not just name matching |
| Step 1g — MCP/CLI adapter scope grep | `rg "<TypeName>" crates/qbot-mcp/…` | `cfdb query` — `MATCH (caller:Item)-[:CALLS]->(target:Item) WHERE target.name = '<TypeName>' AND caller.crate =~ 'qbot-mcp.*'` — but ONLY with HIR (CALLS edges need cfdb-hir-extractor) |
| Step 2a — entry point identification | `rg` in qbot-mcp/cli dirs | `cfdb query` — `MATCH (ep:EntryPoint)` — BUT requires cfdb-hir-extractor (v0.2), NOT available in Phase A |
| Step 2b — data flow trace | Manual file walking | `cfdb query` BFS over CALLS* — requires cfdb-hir-extractor |
| Step 2c — caller census | `grep -rn "<fn>(" crates/*/src/` | `cfdb list-callers --qname <fn>` (TYPED verb) — Phase A stub currently, Phase B implementation |
| Step 3 — decorator chains | `rg "impl <Trait> for"` | `cfdb query` IMPLEMENTS edges — syn-available (Phase A) |
| Step 4 — ownership map | Manual analysis of call chains | `cfdb query` `CANONICAL_FOR` edges — requires concept enrichment (Phase B) |
| Step 6a — decision archaeology | `git log --follow -L <line>,<line>:<file>` | cfdb `enrich_history` — `:Item.git_age_days`, `:Item.git_last_author` available after enrichment; but git blame commands stay out-of-band (cfdb doesn't wrap git log in readable form for archaeology) |

**Phase A vs Phase B coverage gap:**

Critical finding: **Steps 2a (entry points), 2b (data flow trace), and 1g (MCP/CLI scope grep via CALLS) all require cfdb-hir-extractor (v0.2 parallel crate), which is NOT YET SHIPPED.** These are the highest-value /discover sections for split-brain prevention. The syn-based Phase A cfdb covers Steps 1a-1f (static item census) well but misses the dynamic call-graph sections entirely.

This means /discover integration with cfdb has two distinct phases:
- **Phase A integration (available now):** replace grep-based concept census (Steps 1a-1e), const table ancestry, global name search. These are syn-level facts that cfdb-extractor already populates.
- **Phase B integration (after cfdb-hir-extractor ships):** replace call chain tracing (Steps 2a-2c), decorator chain following, entry point cataloging.

**Verb gaps for /discover:**

The 15 shipped verbs lack one verb that /discover needs: `list-definitions-of <name>` — return all `:Item` nodes whose `name` attribute exactly or case-insensitively matches a given string, across all crates. The existing `query` verb CAN express this with a Cypher string, but the skill author must know the Cypher syntax. A `list-definitions-of` typed verb (16th verb, Phase B) would be the ISP-correct surface for Step 1a.

**Determinism impact of adding `list-definitions-of`:** zero — read-only query over existing syn-extracted nodes, no new extraction pass, no new schema elements. The verb is a typed convenience wrapper over `query_raw`, identical in structure to `find-canonical`. It does NOT violate §12.1 invariants (BTreeMap, single-thread, stable sort, sorted-jsonl canonical dump). syn source walks remain unchanged.

**Cost (proposed 16th verb):** adds one `cfdb query` composition to cfdb-cli. wire_form test must be updated (per the BRIEF: "budget each new verb as a Phase B implementation + wire_form update"). Low cost. Syn-level — no hir dependency.

### /prescribe wiring

**Current /prescribe anti-split-brain checks and cfdb verb mapping:**

| /prescribe Step | Current check | cfdb verb |
|---|---|---|
| Step 5b — Resolution Census Verification | `rg "impl\s+FromStr\s+for\s+<Type>"` across workspace | `cfdb query` — same as Step 1d above; FAST because already indexed |
| Step 5b — parse/resolve function search | `rg "fn\s+(parse|resolve)_\w*<concept>"` | `cfdb query` — `MATCH (i:Item {kind:'Fn'}) WHERE i.name =~ '(parse|resolve)_<concept>.*'` |
| Step 5b — const table overlap check | `rg "const\s+\w*<CONCEPT>"` + 3-value sample | `cfdb query` — `MATCH (i:Item {kind:'Const'}) WHERE i.name CONTAINS '<concept>'` |
| Step 5c — test-parser divergence | `rg "fn\s+<fn_name>" crates/*/tests/` | cfdb query on `:Item {kind:'Fn'}` filtered by file path containing `test` — requires syn sees test files |
| Step 5d — cross-crate name collision | `rg "pub\s+(struct|enum|type)\s+<Name>" crates/*/src/` | `cfdb query` — MATCH (i:Item) WHERE i.name = '<Name>' AND i.kind IN ['Struct','Enum','TypeAlias'] — most precise, no grep noise |
| Step 5e — market-phenomenon heuristic | `rg "pub\s+(struct|enum)\s+\w*(Imbalance|Swing|Gap|…)" crates/domain-market-structure/src/` | `cfdb query` — filter by crate + name pattern; equivalent but typed |
| Step 5f — MCP/CLI EXTEND scan | `rg "<TypeName>" crates/qbot-mcp/src/ crates/screener-cli/src/ …` | `cfdb list-callers --qname <TypeName>` (TYPED verb) — Phase A stub; `cfdb query` with CALLS edges (HIR, Phase B) |
| Step 3 — decision tree: does concept exist? | Discovery artifact scan | `cfdb find-canonical --concept <name>` (TYPED verb, Phase A stub) — Phase B returns real data |

**Critical observation on Step 5b (the most load-bearing check):**

/prescribe Step 5b is the "before I CREATE, prove no existing resolution point exists" gate. Currently this relies on the prescriber running grep. The same information is in cfdb's `:Item` nodes after a syn extraction. A `cfdb query 'MATCH (i:Item) WHERE i.qname CONTAINS "FromStr" AND i.name CONTAINS "<Type>"'` returns all FromStr impls across the workspace in one indexed lookup vs. multiple grep invocations. This is an **immediate win that requires zero new verbs** — just a prescriber that knows to run `cfdb query` before finalizing a CREATE decision.

**Schema adequacy for prescribe checks:**

Steps 5b, 5c, 5d, 5e are fully coverable with the Phase A syn schema (`:Item`, `name`, `kind`, `crate`, `file`, `line`). Step 5f requires CALLS edges (Phase B). The schema §7 has the required node/edge types; the gap is purely Phase A vs Phase B availability.

One missing schema element for /prescribe: **`:Item.bounded_context` attribute**. This is introduced by `enrich_bounded_context` (§A2.2 enrichment pass) but is NOT populated by syn-only Phase A extraction. The Step 5d cross-crate name collision check needs bounded_context to correctly distinguish "same concept different context" (keep both) from "same concept same context" (REUSE). Without it, /prescribe must fall back to manual reasoning from crate names. The enrichment is deterministic (crate-prefix heuristic + `.cfdb/concepts/*.toml` overrides) — it COULD be incorporated into the Phase A extractor rather than being a separate enrichment pass. This is a design decision: making `bounded_context` a syn-level attribute (derive from crate name at extraction time) vs. an enriched attribute (separate pass). The former eliminates the dependency on `enrich_bounded_context` running before /prescribe.

**Recommendation:** make `bounded_context` a syn-level attribute derived from the crate-prefix convention at extraction time (cfdb-extractor produces it, not cfdb-enrich-concepts). The `.cfdb/concepts/*.toml` override mechanism remains as an enrichment pass for the exceptional cases. This means /prescribe can use `i.bounded_context` immediately after a `cfdb extract` without requiring `cfdb enrich-concepts` to run first.

**Determinism impact:** zero — crate-prefix derivation is a deterministic pure function. No new I/O. Does not violate §12.1 invariants.

### Verdict on Q1

YELLOW. The syn-based Phase A cfdb (shipped as of fb288258d) covers /discover Steps 1a-1f and /prescribe Steps 5b-5e adequately via `cfdb query` and `cfdb violations`. Steps 2a-2c (call-graph, entry-point, data-flow trace) require cfdb-hir-extractor (Phase B) and cannot be replaced until that ships. Two concrete gaps need addressing before full integration: (1) `bounded_context` should be a syn-level attribute, not enrichment-only; (2) a 16th `list-definitions-of` typed verb reduces friction for skill authors calling cfdb from /discover.

---

## Q2 — Permanent watchdog

### Tier table

| Tier | Trigger | Runs | Writes | Blocks |
|---|---|---|---|---|
| Per-save (IDE) | `lefthook` pre-commit on `.rs` file save | `cfdb violations --rule arch-ban-*.cypher` (scoped to changed crates only) | Console warning to developer | No — advisory only; too slow for every keystroke; can gate `git commit` instead |
| Per-session | Main agent at `/work-issue` PHASE 0 (between /freshness and /discover) | `cfdb query` (Step 1a-1f of /discover, syn-level facts only) against current workspace SHA | Nothing new — /discover writes `.discovery/<issue>.md` using cfdb query results | YES — /discover HARD-BLOCKS if inventory is stale AND staleness exceeds threshold (see below) |
| Per-PR (CI) | Any push to a feature branch | `cfdb violations --rule arch-ban-*.cypher` (all rules) + `cfdb diff <develop-ks> <branch-ks>` to list NEW violations introduced by the PR | PR comment: list of new violations by class; fail CI if any pattern D/E violation is new | YES for new pattern D/E violations; WARN for existing violations (debt not introduced by this PR) |
| Nightly | Cron (weekly audit cron per RFC §11; nightly is achievable with syn-only) | `cfdb extract --workspace .` (full re-extract against HEAD), then full classifier pipeline, refresh `.concept-graph/RESCUE-STATUS.md` | `.concept-graph/RESCUE-STATUS.md`, JSONL snapshot in `.cfdb/snapshots/<sha>/` | No — advisory; emails/pings if infection thresholds crossed |
| Weekly | Same as nightly but additionally runs `cfdb diff <prev-week-ks> <current-ks>` | drift delta report; feeds operate-module threshold check | `.concept-graph/RESCUE-STATUS.md` delta section | No — produces input for next /operate-module invocation |

### Inventory lifecycle

**Where it lives:** `.cfdb/inventory.json` (or `.cfdb/<project>.ldb` for the LadybugDB backend) lives INSIDE the repo at `.concept-graph/cfdb/` (the sub-workspace already owns this directory). The canonical JSONL dump lives at `.cfdb/snapshots/<sha12>.jsonl`. The LadybugDB `.ldb` file is a REBUILDABLE CACHE — it is gitignored. The JSONL snapshots ARE committed (per §12.1 "JSONL canonical fact format" — snapshots are committed as test fixtures and drift baselines).

**Why the JSONL snapshot is committed and the .ldb file is not:** the `.ldb` file has no format stability guarantee (§10.1: "treat the `.ldb` file as a rebuildable cache, not a portable fixture"). The JSONL dump IS stable and diffable. Committing `.ldb` would create binary noise in every commit that follows a re-extract.

**Who refreshes it:** CI runs `cfdb extract` on every merge to develop. The output JSONL snapshot is committed back by the CI job. This is the same pattern as `.concept-graph/RESCUE-STATUS.md` (§A4.2 — committed to repo, refreshed by CI).

**Staleness detection:** the inventory key is `(workspace_sha, schema_major, schema_minor)`. /discover checks whether a keyspace exists for the current `git rev-parse HEAD`. If not, the inventory is stale. The staleness threshold for /work-issue is: if `git rev-list <last-extract-sha>..HEAD --count` exceeds N commits (recommend N=10 as a starting value — calibrate from v0.2 telemetry), /discover BLOCKS and refuses to proceed until inventory is refreshed.

**Invalidation contract with /freshness:** /freshness already checks the workspace for RFC/commit freshness. cfdb inventory staleness is a separate concern (a cfdb-specific check). The cleanest integration is a new step in /freshness — Step 2f (cfdb pre-scan, per skill-freshness.md line 106) — which runs `cfdb extract` if the current HEAD has no matching keyspace. /freshness is the right place for this because: (1) /freshness already has the `allowed-tools: Bash(*)` permission set needed to run cfdb; (2) /freshness runs before /discover; (3) it converts a blocking condition into a non-blocking prep step (freshness refreshes the inventory rather than blocking the pipeline).

**Specifically:** add to /freshness Step 2f (currently the split-brain pre-scan step):

```
2f-cfdb: Check cfdb inventory freshness.
  Run: cfdb list-keyspaces --db .cfdb/
  If no keyspace matches current git HEAD SHA (truncated to 12 chars):
    Run: cfdb extract --workspace <ws> --db .cfdb/ --keyspace qbot-core-<sha12>
    This takes 20-60s for syn-only extraction on a 23-crate workspace.
  Write result to context package: "cfdb_inventory_sha: <sha12>" 
  Downstream /discover reads this and uses the correct keyspace.
```

This is compatible with the /freshness "allowed-tools" surface (`Bash(*)`) and produces a deterministic artifact reusable by /discover.

**CI failure modes:**

| Condition | CI outcome | Routing |
|---|---|---|
| New pattern D/E violation introduced by the PR branch (in diff result) | FAIL — block merge | Annotate PR with `cfdb diff` output listing new violations; route to /boy-scout if class = `random_scattering` or `canonical_bypass`; route to /operate-module if context infection threshold crossed |
| Existing violations detected (not new — pre-existing debt) | WARN — no block | Comment on PR listing existing violations in touched scope; does NOT block merge |
| cfdb extract fails (toolchain issue, parse error) | WARN — skip cfdb gate | Do not block CI on cfdb infrastructure issues; log and alert; cfdb is additive, not a compilation dependency |
| Infection threshold crossed in any context touched by PR | COMMENT — no block in CI; notify in PR | Generate operate-module candidate list; add to `.concept-graph/RESCUE-STATUS.md` next nightly run |

**Auto-remediation routing:**

The CI gate does NOT auto-apply /boy-scout, /operate-module, or /sweep-epic — that would violate "cfdb never modifies Rust files" and "operate-module produces plans, never edits" (RFC §A7). Instead, CI annotates the PR with a routing recommendation. The session agent in the next /work-issue run reads the annotation and routes accordingly.

### Verdict on Q2

YELLOW. The watchdog architecture is sound: per-PR cfdb diff gate, nightly inventory refresh, staleness detection via keyspace matching, /freshness integration as the session-layer inventory check. Two things remain unresolved until cfdb-hir-extractor ships: (1) the per-PR diff gate is syn-level only (pattern D/E arch bans work; pattern B VSB does not until HIR lands); (2) the CI memory budget for `cfdb extract` with HirDatabase is unverified (v0.2-5b). RED if HIR extract exceeds CI runner memory budget — that would force a streaming/per-context extraction mode.

---

## Q3 — Missing skills

### /operate-module

**Description (skill frontmatter style):** Evaluate cfdb context infection inventory against §A3.2 thresholds and emit a bounded-context raid plan if thresholds are crossed.

**Arguments:** `<context-name> <inventory-json-path> [--workspace <path>]`

**Inputs:**
- `<context-name>` — the bounded context to evaluate (e.g., `trading`, `portfolio`)
- `<inventory-json-path>` — path to the structured JSON emitted by `cfdb query` or `/cfdb-scope`; matches the §A3.3 shape exactly
- `--workspace <path>` — required for writing the output raid plan to the correct location

**Outputs:**
- If threshold NOT crossed: text verdict "Below threshold for context <name>. Route findings to /boy-scout for classes: random_scattering, unwired."
- If threshold IS crossed: `raid-plan-<context-name>.md` written to `.concept-graph/raid-plans/` in the workspace. The file follows the §A3.3 template verbatim.

**Invariants (what it must never do):**
- NEVER run `cfdb extract` or `cfdb query` itself — it consumes a pre-built inventory; it does NOT query the graph
- NEVER edit Rust source files
- NEVER invoke /sweep-epic or /boy-scout directly — it produces a plan that humans/council approve, then those skills are invoked separately
- NEVER claim council approval that hasn't happened — the raid plan is always "draft — council required"

**Invocation protocol (how /work-issue decides to invoke):**

/work-issue does NOT invoke /operate-module automatically. /operate-module is invoked BY the human or main agent when: (a) RESCUE-STATUS.md shows a context crossing the §A3.2 thresholds; OR (b) the per-PR CI gate emits an "operate-module candidate" annotation.

The invocation flow is:
```
cfdb query → structured inventory → /operate-module → raid-plan.md → COUNCIL → RFC → /sweep-epic --mode=port
```

/operate-module is NOT part of the standard /work-issue gate sequence. It is a separate lifecycle protocol for surgical interventions.

**Relationship to sibling skills:**
- To `/discover`: /operate-module reads cfdb output, not /discover artifacts. /discover is per-issue; /operate-module is per-context.
- To `/prescribe`: /operate-module does NOT prescribe. It documents infection inventory and declares a surgical plan. /prescribe runs later, inside the /work-issue that executes the portage.
- To `/gate-raid-plan`: /gate-raid-plan validates an APPROVED raid plan (post-council) against the current fact base to catch dangling callers before code moves. /operate-module produces the DRAFT plan. Sequence: /operate-module → council → /gate-raid-plan (validation) → /sweep-epic --mode=port (execution).
- To `/sweep-epic --mode=port`: /operate-module produces the raid plan file that `/sweep-epic --mode=port --raid-plan=<path>` consumes.

**Failure modes and escape hatches:**
- Missing inventory file: HARD STOP, emit error, do not guess context state
- Context not recognized (not in crate-prefix convention and not in `.cfdb/concepts/*.toml`): HARD STOP, emit list of known contexts
- Raid plan file already exists for this context: append a new `## Revision <N>` section, do NOT overwrite
- Infection data is stale (inventory SHA is >N commits behind HEAD): WARN in the raid plan header; do not refuse

### /gate-raid-plan

**Description:** Validate an approved bounded-context raid plan against the current cfdb fact base via Pattern I queries. Catches dangling callers, missing canonicals, and clean/dirty mismatches before any file moves.

**Arguments:** `<raid-plan-path> --workspace <path> --db <cfdb-db-path>`

**Inputs:**
- `<raid-plan-path>` — path to an approved `raid-plan-<context>.md` (the §A3.3 format document)
- `--workspace` — workspace path
- `--db` — path to the cfdb database directory

**What it asserts (5 Pattern I queries from RFC §3.9):**

1. **Completeness** — every item in the "Portage list" section of the raid plan exists in cfdb as a reachable `:Item` with correct `crate` attribute. No phantom items.
2. **Dangling-drop** — for every item in the "Dead list" (no reachable entry points), verify `reachable_from_entry = false` in the current cfdb snapshot. If anything was wired since the raid plan was drafted, flag it — the disposition changes from DELETE to potential WIRE.
3. **Hidden-callers** — for every item in the "Portage list" or "Misplaced list", enumerate ALL callers via `cfdb list-callers`. Any caller NOT in the raid plan's scope is a hidden dependency. Emit the caller list; fail if any hidden caller is in a different bounded context (cross-context dependency that the raid plan missed).
4. **Missing-canonical** — for every concept in "Canonical candidates", verify a `CANONICAL_FOR` edge exists or that cfdb `find-canonical` returns a definitive result. If the candidate is ambiguous (two implementations with equal fan-in), flag for council re-vote.
5. **Clean/dirty-mismatch** — for every item marked "Portage list" (viable — move to new home), verify the item's debt class is NOT `context_homonym` or `unfinished_refactor` (those belong in Misplaced/Dead, not Portage). Mixed disposition in the same item = mismatch.

**What it returns:**
- Exit 0 with "RAID PLAN VALIDATED" if all 5 checks pass
- Exit 1 with a structured JSON report listing which checks failed and why; each failure lists the cfdb query that produced it

**Where in the pipeline it runs:**
Between council approval of the raid plan and the first `/sweep-epic --mode=port` invocation. It is a one-shot pre-flight check, not a continuous gate. /gate-raid-plan MUST pass before any portage code moves.

**Wire form:** CLI (`cfdb query_with_input` calls with the portage/drop/glue/rewrite buckets from the raid plan as external parameter sets — the `--input` flag of the `cfdb query` verb, per §6.2).

**Failure modes:**
- cfdb inventory stale: FAIL with staleness warning; re-run `cfdb extract` first
- raid plan missing required sections (Portage list, Dead list, Canonical candidates): FAIL with format error
- Pattern I queries require CALLS edges (HIR): if cfdb-hir-extractor is not available, checks 3/4/5 degrade to syn-level only (incomplete recall); emit warning, do not hard-fail

### /cfdb-scope

**Resolution: NOT a skill — implement as a shell-level composition invoked by /operate-module and /work-issue.**

**Rationale:** `/cfdb-scope` as described in the v0.2 BRIEF is the step that extracts a per-context structured inventory from cfdb and writes it to a JSON file for /operate-module to consume. This is exactly one `cfdb query` call with a context filter plus a JSON redirect. There is no judgment, no artifact that needs to be a committed `.prescriptions/` file, no audit trail requirement. Making it a skill adds orchestration overhead (sub-agent spawn, wait, read result) for what is equivalent to:

```bash
cfdb query --db .cfdb/ --keyspace qbot-core-<sha> \
  'MATCH (i:Item) WHERE i.bounded_context = "<context>" RETURN i' \
  > .cfdb/inventory-<context>.json
```

**If a CLI flag is preferred:** add `--context <name>` to `cfdb query` (or to a new `cfdb inventory` verb). The flag filters extracted items to those belonging to the specified bounded context. This is an optional 16th verb (`cfdb inventory --context <name>`) with:
- Input: context name, keyspace, db path
- Output: §A3.3-shaped JSON (structured inventory with findings_by_class, canonical_candidates, etc.) as opposed to raw query rows

This 16th verb is higher-value than the `list-definitions-of` verb proposed in Q1, because it encapsulates the entire A3.3 output shape (aggregation of multiple queries), making /operate-module a simple consumer of a single CLI call rather than an orchestrator of multiple cfdb queries.

**Determinism impact of `cfdb inventory` verb:** reads only from the current keyspace (no new extraction); output is deterministic given the same keyspace. No §12.1 violations.

**Syn vs HIR:** class assignment (the findings_by_class field) requires the classifier pipeline (§A2.2 enrichment passes + Cypher). The enrichment passes that require git I/O (`enrich_git_history`) must have been run. This verb is a Phase B verb — it depends on enrichments that are Phase B work.

### /boy-scout --from-inventory

**Resolution: single skill, two modes.** Justify:

The /boy-scout skill has one responsibility: fix pre-existing mechanical violations near touched files. The `--from-inventory` mode changes the SOURCE of violations from the file-proximity doughnut (boy-scout-scope binary) to a cfdb Finding inventory. The fix actions are identical (unwrap → expect, ignore → reason, etc.). Adding a second mode is an input substitution, not a new responsibility — it does not violate SRP.

A sibling skill would duplicate the entire fix/verify/commit pipeline. That is worse.

**`--from-inventory` mode specification:**

Arguments: `--from-inventory <path-to-inventory.json> [--class <class-filter>]`

Behavior: instead of running `boy-scout-scope` to compute the doughnut, parse the inventory JSON and extract violations with `class ∈ <class-filter>`. Apply the same budget cap (50%, max 5 files). Apply the same mechanically-fixable-only constraint.

**Class filter to 6-class taxonomy — which classes boy-scout owns:**

| Class | Boy-scout owns? | Rationale |
|---|---|---|
| `random_scattering` | YES | Copy-paste drift with no refactor intent; short functions; mechanical — exactly boy-scout's mandate |
| `unwired` (no tracker) | YES | Delete-only action; no logic change required; mechanical |
| `unwired` (with `TODO(#issue)`) | PARTIAL — wire only if the wiring is mechanical | If the wiring requires architecture decisions, skip silently |
| `canonical_bypass` | NO | Rewiring requires understanding which is canonical; architecture decision, not mechanical |
| `duplicated_feature` | NO | Consolidation requires picking one head (architecture judgment) + pub use migration |
| `unfinished_refactor` | NO | Completing a migration requires reading the RFC intent; not mechanical |
| `context_homonym` | NO | Context Mapping decision; architectural, requires council |

**Why the split:** boy-scout's mandate is "mechanically fixable only" and "correctness must not be risked." The classes boy-scout does NOT own all require knowing which of two parallel implementations is the canonical one — that is prescriber-level judgment. Boy-scout with `--from-inventory` is a targeted cleanup of the 2 lowest-risk classes, not a general debt-elimination sweep.

**Input contract:** the inventory JSON consumed by `--from-inventory` must be the §A3.3-shaped output — specifically the `findings_by_class.random_scattering` and `findings_by_class.unwired` arrays, each element containing `file`, `line`, and `evidence`. The `file:line` pairs become the boy-scout batch input instead of the doughnut-scoped violations.

**Budget cap behavior in `--from-inventory` mode:** same 50%/max-5-files cap applies. The inventory may contain hundreds of random_scattering findings — boy-scout still caps at 5 files per invocation. Multiple boy-scout runs consume the inventory incrementally.

### Verdict on Q3

YELLOW. /operate-module and /gate-raid-plan specs are complete and Rust-systems clean (no orphan violations, no object safety issues, no Cargo coupling problems — they operate as subprocess CLI consumers). The /cfdb-scope question resolves cleanly as a 16th `cfdb inventory` verb (Phase B) rather than a skill. The /boy-scout `--from-inventory` extension is clean SRP (input substitution, not new responsibility). The main open risk: /gate-raid-plan's Pattern I checks 3/4/5 degrade badly without HIR, which is a Phase B dependency.

---

## Blocking concerns

- `cfdb-hir-extractor` (Phase B, not yet shipped) blocks the highest-value /discover sections (call-chain trace Steps 2a-2c) and /prescribe Step 5f (MCP/CLI EXTEND scan via CALLS edges). The Phase A integration described in Q1 is real and valuable but incomplete. Sessions operating in Phase A have no choice but to fall back to `rg`-based call chain tracing for those steps.
- `bounded_context` attribute must be available from syn-level extraction (not enrichment-only) for /prescribe Step 5d to work without requiring `cfdb enrich-concepts` to run first. If this is not fixed, /prescribe must run `cfdb enrich-concepts` as a prerequisite, which adds latency and is not documented anywhere in the current skill files.
- The 60s CI budget for cfdb (from BRIEF §A3.2 "cfdb must refresh on changed files and emit a drift query result in under ~60s") is feasible for syn-only extraction on a 23-crate workspace (syn walks are fast; measured syn extract times for 20-crate workspaces are in the 10-30s range). It is NOT feasible for HIR extraction (v0.2-5b gate: "N=5 min, M=4 GB — calibrated after first measurement"). Therefore the per-PR CI gate must remain syn-only (patterns D/E arch bans) until a streaming/per-context HIR extraction mode ships. This is an architectural constraint on the watchdog design in Q2.

---

## Convergent follow-ups

- The BRIEF's pipeline diagram (cfdb extract → .cfdb/inventory.json → /discover → /prescribe) is correct and I would accept it without a second deliberation if other council members reach the same wiring shape.
- The §A4.1 /freshness integration for cfdb inventory pre-check (run `cfdb extract` if HEAD has no keyspace) is straightforward and I expect the clean-arch and solid specialists to have no objection.
- Routing the 16th verb (`cfdb inventory --context <name>`) through the v0.2 wire_form test update is the correct gate — I would fold this in from any member who proposes it independently.
- The `--from-inventory` mode for /boy-scout replacing a sibling skill is a convergent conclusion I expect from the SOLID specialist's SRP analysis.
