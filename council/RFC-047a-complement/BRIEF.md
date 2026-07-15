# Council BRIEF — RFC-047a (impact query mechanics complement)

**Convened:** session 2026-06-25. **Base:** `origin/develop` @ `018e766` (worktree `work/488-seed-list-binding`).
**Mechanism:** agent-team council (4 lens teammates, mailbox + shared task list) per `CLAUDE.md §2.3` + global `§2b`. You challenge each other directly by `SendMessage`; this is **not** isolated fan-out — Phase B (§3) is mandatory cross-challenge.
**Under review:** [`docs/RFC-047a-impact-query-mechanics.md`](../../docs/RFC-047a-impact-query-mechanics.md), a complement to the ratified [`docs/RFC-047-impact-blast-radius.md`](../../docs/RFC-047-impact-blast-radius.md).

---

## 1. What this council decides

A **single** complement that (a) retracts a false foundational finding in RFC-047 and (b) resolves three mechanical blockers (B1 open-range parse, B2 silent depth cap, B3 dogfood needs HIR CALLS) and re-cuts slices 47-0/47-A.

| Verdict vocabulary | Meaning |
|---|---|
| **RATIFY** | The correction + the B1/B2/B3 design + the re-cut slices are sound; 47-0/47-A may be re-filed. |
| **REQUEST CHANGES** | Name the **exact** amendment that flips you to RATIFY (RFC-046 council standard: a concrete, checkable change). |
| **REJECT** | The complement's framing is wrong; say what the real design is. |

Ratified only when **all four lenses RATIFY** (or a single author-documented override in `RATIFIED.md`, `CLAUDE.md §2.3`). The central live question is **Q1** (§3) — expect to converge there before voting.

## 2. House rules (binding)

1. **Verify every claim at `file:line`.** The complement cites cfdb internals; open the files. The verified-facts set (§4) is your starting point — extend it, don't trust it. A verdict on an unverified claim is invalid (memory: *council foundation claims need verification* — which is itself the reason this complement exists).
2. **No self-certification / no implementation.** You analyse and vote (`CLAUDE.md global §5`).
3. **You prescribe tests.** Each re-cut slice (complement §7) carries a 4-row `Tests:` block. If a row is wrong or should be `none — rationale:`, say so.
4. **No metric ratchets** (`global §6 rule 8`). REJECT on sight if any appears.
5. **Schema discipline.** Flag any keyspace schema surface. *Note:* the author argues B1 is a **Cypher-subset grammar** change (query language), NOT a keyspace schema change → no `SchemaVersion` bump, no `graph-specs-rust` lockstep. **Verify this classification** — if you think the grammar addition drags a companion lockstep, that is a REQUEST CHANGES.
6. **YAGNI / reuse** (memory *rfc yagni reuse*). The complement claims it reuses the `(u32,u32)` AST tuple (no new variant) and adds no config knob. Verify nothing speculative crept in.

## 3. Contested questions — Phase B mailbox cross-challenge (MANDATORY)

Phase A = independent per-lens verdict file. Phase B = challenge each other on these. Each names the lenses who must engage.

- **Q1 (CRUX) — open-form `*N..` semantics: cap at `DEFAULT_VAR_LENGTH_MAX` (5) or visited-set-unbounded?** The BFS dedupes by visited-set (`eval/pattern/path.rs:211,230`) ⇒ O(V+E) either way, so the cap buys no asymptotic win — but "unbounded" becomes the global semantic for every future open-form query. RFC-047 §3.2 promised "unbounded by default." Is unbounded-via-visited-set right, or is a safety cap on the open form prudent (forcing `impact` to pass an explicit bound)? *Engage:* **rust-systems (lead) ⇄ clean-arch ⇄ solid-architect.**
- **Q2 — is the explicit-bound clamp (`*1..10` silently → 5, `path.rs:208`) a latent bug to fix independently of `impact`?** It contradicts `DEFAULT_VAR_LENGTH_MAX`'s own doc (`eval/mod.rs:62` "when a pattern OMITS its upper bound"). Does the fix land in re-framed 47-0, or a standalone `fix:` issue (cf. the council-found latent G6 bug #486)? Are any **shipped** queries currently truncated by it? *Engage:* **rust-systems ⇄ clean-arch.**
- **Q3 — 47-0 / 47-A boundary + HIR-dogfood cost.** Does query-mechanics (B1+B2) belong in a re-framed 47-0 and the canonical query + dogfood in 47-A? Is `cfdb extract --hir` inside a test (pulls `ra_ap_*`, 90–150 s cold compile per memory) acceptable, or must a lighter in-process CALLS-resolution path be exposed first? *Engage:* **solid-architect ⇄ rust-systems ⇄ ddd-specialist.**
- **Q4 — correction of record: amend RFC-047 §3.2/§5 in place (retract the false "no list-binding path exists" finding) or let this complement supersede?** *Engage:* **all.**

If a challenge converges, record the agreed position. If not, record the disagreement explicitly — the lead synthesises and may run R2.

## 4. Verified facts (lead-checked against `develop` @ `018e766` — extend, don't trust)

### 4.1 List-binding path EXISTS — the retracted finding (complement §1)
- `Param::List(Vec<PropValue>)`: `crates/cfdb-core/src/query/ast.rs:54-57`.
- Evaluator resolves it for `IN`: `eval_expr_list` `Param::List ⇒ Some(items.clone())` `crates/cfdb-petgraph/src/eval/predicate.rs:115-117`; consumed by `Predicate::In` at `predicate.rs:26-33`.
- CLI binds it: `param_resolver.rs:8,90` `list:`/`context:` → `Param::List` (#145); shipped `check-predicate` verb (#147); fixture-exercised in `crates/cfdb-petgraph/tests/raid_plan_queries.rs` (#205, binds `list_param()` into `IN $portage`/`$drop`).
- RFC-047's cited counter-evidence is the **out-of-scope** raw path: `commands/query.rs:39` (`--input` stub), `:104` (`bind_single_param` rejects arrays).

### 4.2 B1 — open-range parse gap CONFIRMED
`crates/cfdb-query/src/parser/match_clause.rs:82-86`: `range` = `just('*') → digits() → ".." → digits()`. Both bounds required; `*1..` does not parse. (`*N..M` and `[:LABEL]` unaffected.) AST target: `EdgePattern.var_length: Option<(u32,u32)>` `ast.rs:108`.

### 4.3 B2 — depth cap CONFIRMED, contradicts its own doc
`DEFAULT_VAR_LENGTH_MAX: u32 = 5` `crates/cfdb-petgraph/src/eval/mod.rs:64`, doc at `:62` = "Maximum BFS depth when a variable-length pattern OMITS its upper bound." Applied to **all** var-length in `traverse_bfs`: `let max_depth = max_depth.max(min_depth).min(DEFAULT_VAR_LENGTH_MAX.max(min_depth));` `eval/pattern/path.rs:205-208`. Visited-set dedup at `path.rs:211,230` ⇒ O(V+E). So `*1..10` → 5 (truncated); no perf reason.

### 4.4 B3 — `extract_workspace` has no resolved CALLS CONFIRMED
`crates/cfdb-extractor/src/lib.rs:18` "Out of scope for v0.1: resolved cross-crate `CALLS` (Item → Item)"; emits `INVOKES_AT` + `synthesize_referenced_items` stubs (`lib.rs:254`). Resolved `Item→Item CALLS` is HIR-only: `crates/cfdb-hir-extractor/src/emit.rs`, `call_site_emitter/`. The existing dogfood `crates/cfdb-cli/tests/predicate_library_dogfood.rs` uses `extract_workspace` and asserts only on non-CALLS facts.

### 4.5 Proofs on disk
`.proofs/488-impact-seed-binding.txt` (RFC-047 §3.2 query → parse error on `*1..`), `.proofs/488-fixture-anchor.txt` (green once bounded to `*1..5`). Salvage test: `crates/cfdb-cli/tests/impact_seed_binding.rs`.

## 5. Output contract

Write your verdict to `council/RFC-047a-complement/<lens>.md`, `<lens> ∈ {clean-arch, ddd-specialist, solid-architect, rust-systems}`:

```
# <Lens> verdict — RFC-047a

## Verdict
RATIFY / REQUEST CHANGES / REJECT — one-line reason.
(If REQUEST CHANGES: the exact, checkable amendment that flips you to RATIFY.)

## Per-blocker analysis
B1 / B2 / B3 + the correction-of-record: your position, evidence at file:line, the genuine open question.

## Contested-question positions (the Qs you engaged)
Your position + who you challenged + outcome (converged / disagreed).

## Test-surface prescription notes
Any correction to the 47-0 / 47-A 4-row Tests: blocks (complement §7).
```

**Be concrete.** A REQUEST CHANGES names the exact amendment. Cite `file:line` for every claim about cfdb internals. When done: `SendMessage` to `main` with your one-line verdict + a short summary, and mark your task complete.
