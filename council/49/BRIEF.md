# Council Brief — RFC-034 Query DSL (Issue #49)

**Convened:** 2026-04-21
**Team:** `council-49-query-dsl`
**RFC under review:** `docs/RFC-034-query-dsl.md`
**Members:** clean-arch, ddd-specialist, solid-architect, rust-systems (one per lens per project CLAUDE.md §2.3)
**Tracking issue:** #49

---

## 🚨 READ BEFORE YOU THINK

- **Draft RFC:** `docs/RFC-034-query-dsl.md` — read in full before writing your verdict.
- **Issue body:** #49 — fetch via `mcp__forge__forge_get_issue repo=agency:yg/cfdb number=49`.
- **Project rules:** `CLAUDE.md` §§1-7 (RFC pipeline, architect review via agent teams, dogfood enforcement).
- **Your lens section in the RFC:** §5.1 (clean-arch), §5.2 (ddd-specialist), §5.3 (solid-architect), §5.4 (rust-systems). The RFC already frames specific questions for you — answer those first.
- **Open questions §9:** direct council asks, one per lens.

Your verdict file is at `council/49/verdicts/<your-lens>.md`. Use the existing `council/verdicts/<lens>.md` files (from `council-cfdb-wiring`) as format templates.

---

## Forcing question

Issue #49 says the deliverables include "DSL grammar (BNF or chumsky-style)" and "new crate `crates/cfdb-query-dsl/`". The RFC draft REJECTS both. It proposes instead:

1. **No new grammar.** Re-use the existing `cfdb-query` Cypher subset.
2. **No new crate.** Extend `cfdb-query` with a `param_resolver` module + extend `cfdb-cli` with a `check-predicate` verb.
3. **New `.cfdb/predicates/` directory** — named Cypher templates, sibling of `.cfdb/queries/` (which is for ban rules).

**Your job:** ratify, reject, or request changes on this reframe. Specifically:

### Per-lens asks

- **clean-arch** — Q-CA-1: Is `cfdb-cli → cfdb-query → cfdb-concepts` acceptable for placing the TOML-backed param resolver in `cfdb-query`? Q-CA-2: Should `cfdb check-predicate` be a new verb, or should it collapse into the existing `cfdb violations --rule <path>` verb?

- **ddd-specialist** — Q-DDD-1: Is the `Predicate` (AST node in `cfdb_core::query`) vs `predicate` (file in `.cfdb/predicates/`) homonym acceptable within the `cfdb-query` bounded context, or does it require a rename (e.g. `.cfdb/rules/`, `.cfdb/checks/`)? Q-DDD-2: Does `cfdb check-predicate` overlap bounded-context-wise with `cfdb check --trigger T1` (from #101) sufficiently to require merging?

- **solid-architect** — Q-SOLID-1: `cfdb-query` currently has 6 responsibilities (parser, builder, DebtClass inventory, shape_lint, SkillRoutingTable loader, list_items_matching composer). Adding `param_resolver` makes 7. Does this cross the SRP threshold? If yes, should this RFC carve out a `cfdb-query-support` sub-crate first, or should the extension land here and a future refactor RFC split later?

- **rust-systems** — Q-RS-1: The RFC pledges "no new parser, no new AST variant, no new evaluator primitive". Is this achievable in practice for every predicate form enumerated in #49, or is a future UDF mechanism inevitable? If inevitable, should this RFC leave a documented extension point now instead of deferring?

### Cross-lens binding (all four must address)

- **Reframe acceptance.** The RFC rejects "new DSL grammar / new crate" as scope bloat. If you disagree, REJECT with evidence. If you agree, RATIFY the reframe explicitly (don't just ratify the downstream decisions).
- **Issue decomposition (§7).** Are the 5 vertical slices well-sized, are their `Tests:` blocks correctly filled, is the sequencing sound?
- **Non-goals (§6).** Every non-goal is a deliberate scope exclusion. Call out any you think should be IN scope or any missing exclusion that would otherwise creep in.

---

## Verdict protocol

1. Read the full RFC + your lens section + the open questions §9.
2. Write `council/49/verdicts/<your-lens>.md` with this shape:
   ```markdown
   # Verdict — <lens>

   ## Read log
   - [x] docs/RFC-034-query-dsl.md
   - [x] council/49/BRIEF.md
   - [x] <any cited source files you actually read>

   ## Reframe
   VERDICT: RATIFY | REJECT | REQUEST CHANGES

   <one paragraph justification of the reframe decision — cite file:line>

   ## Per-lens questions
   - **Q-<tag>-N:** <answer with evidence>

   ## Cross-lens binding
   - **Issue decomposition:** <comments on §7>
   - **Non-goals:** <comments on §6>
   - **Invariants:** <comments on §4>

   ## Overall
   VERDICT: RATIFY | REJECT | REQUEST CHANGES

   <summary — if REQUEST CHANGES, list specific change requests as a numbered list so the author can address them mechanically>
   ```
3. Mark your task `completed` via `TaskUpdate` when your verdict is written.
4. If you have PEER CONCERNS for another lens, add a `## Peer concerns` section naming the lens and the concern — the author will run a cross-lens synthesis pass after all four verdicts land.

## What the author will do after you're all done

1. Read all 4 verdicts.
2. If all RATIFY → write `council/49/RATIFIED.md` summarising + sealing the RFC for decomposition into forge issues. STOP.
3. If any REQUEST CHANGES → author a `council/49/SYNTHESIS-R1.md` consolidating changes, revise the RFC, commit, optionally re-spawn the affected lens for R2. Repeat until all RATIFY.
4. If any REJECT with a conflict another lens did NOT also raise → open a cross-lens peer-challenge round in SYNTHESIS-R1 before revising.

## Hard rules

- **Your verdict is non-negotiable once committed.** If you change your mind, open a new verdict file `council/49/verdicts/<lens>-r2.md`; never edit a committed verdict.
- **No new code in this branch.** This branch lands the RFC + council artifacts. Implementation lands in separate per-slice branches after ratification.
- **Evidence or it didn't happen.** Every claim in your verdict cites file:line from either the RFC, the existing codebase, or a prior RATIFIED council decision.
