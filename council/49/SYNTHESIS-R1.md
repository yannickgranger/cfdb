# Synthesis R1 — council-49-query-dsl

**Date:** 2026-04-21
**Author:** team-lead (main agent)
**Inputs:** `council/49/verdicts/{clean-arch,ddd,rust-systems,solid}.md`

---

## Verdict matrix

| Lens | Reframe | Overall | Blocking? |
|---|---|---|---|
| clean-arch | RATIFY | RATIFY | 2 editorial corrections (non-blocking) |
| ddd-specialist | RATIFY | RATIFY | 1 non-blocking request |
| solid-architect | RATIFY | **REQUEST CHANGES** | CRP violation — param_resolver placement |
| rust-systems | RATIFY | **REQUEST CHANGES** | Parser-reuse claim contradicted by §3.5 example |

**Reframe** (no new crate, no new grammar, no new evaluator primitive): **RATIFIED by all four lenses.** The load-bearing decision of the RFC stands.

**Two blocking changes to R1** — both are concrete and addressable without re-opening the reframe.

---

## R1 changes (applied to `docs/RFC-034-query-dsl.md`)

### C1 — Move `param_resolver` from `cfdb-query` to `cfdb-cli` (solid-architect Change Request)

**Finding:** `cfdb-query` currently has zero runtime consumers of a filesystem-reading module; `cfdb-cli` is the sole consumer. Placing `param_resolver` in `cfdb-query` forces every future `cfdb-query` consumer to accept a `cfdb-concepts` dep they don't need (CRP violation). solid-architect's CRP analysis wins the tie-break against clean-arch's SDP-only RATIFY.

**Resolution:**
- §3.1 — the `pub mod param_resolver` module now lives at `crates/cfdb-cli/src/param_resolver.rs`, not in `cfdb-query`.
- §3.1 API signatures unchanged. Visibility: `pub(crate)` within `cfdb-cli` (accessible from the verb handler in `check_predicate.rs` and from integration tests via `cfdb_cli::param_resolver::*` if re-exported at the crate root — handled at slice-1 discovery time).
- §5.1 updated: dependency edge is `cfdb-cli → cfdb-concepts` (new direct dep), not `cfdb-query → cfdb-concepts`. Direction remains acyclic.
- §5.3 updated: `cfdb-query`'s responsibility count stays at 6; SRP thresholding concern is retracted. CRP justification cites solid-architect's verdict.
- §7 Slice 1 scope — target directory changed to `crates/cfdb-cli/src/param_resolver.rs`; Cargo.toml edit is on `cfdb-cli`, not `cfdb-query`.

### C2 — Rewrite RFC §3.5 canonical predicate to use real schema vocabulary + supported parser constructs (rust-systems + ddd peer concern)

**Finding (two parts):**

**C2.a — Grammar defects (rust-systems Finding 1 + Finding 2):**
- Positive `EXISTS { MATCH ... }` is not in the parser or AST (`parser/predicate.rs:43-48`, `ast.rs:147`, `eval/predicate.rs:45-47`). Only `NOT EXISTS` is supported.
- Inner subquery WHERE in `NOT EXISTS` is Compare-only (`parser/predicate.rs:131-139`); `IN`, `AND`, `OR`, `NOT` are rejected in the inner WHERE.

**C2.b — Schema-vocabulary defects (ddd peer concern → verified by main agent):**
- **`RE_EXPORTS` edge does not exist** in `cfdb_core::schema::EdgeLabel` (labels.rs enumeration confirmed — see `rg "pub const [A-Z_]+" crates/cfdb-core/src/schema/labels.rs`).
- **`DECLARED_IN` edge does not exist.** The intended edge for Item→Crate is `IN_CRATE`.
- Re-export resolution is documented as Phase B / HIR in `crates/cfdb-extractor/src/type_render.rs:4` ("re-exports is RFC §8.2 Phase B (`ra-ap-hir`)"). Emitting `RE_EXPORTS` is explicitly future work, not shipped.
- Available cross-Item edges today: `CALLS` (fn→fn), `IMPLEMENTS` (impl→Trait), `IMPLEMENTS_FOR` (impl→target type). No re-export edge.

**Resolution — rewrite §3.5's seed predicate #1:**

Original (invalid): "context-member-reexport-without-adapter" using `RE_EXPORTS` + positive `EXISTS`.

Rewritten seed #1: **`context-homonym-crate-in-multiple-contexts.cypher`** — a detector for a Crate node whose `name` appears in 2+ context TOML files. Expressible today using `BELONGS_TO` (Crate→Context) edges emitted during extraction. Example:

```cypher
// Params: $context_a (list), $context_b (list) — two context-membership sets to test against
// Returns: (qname, line, reason) — a crate that sits in both contexts is a homonym candidate
MATCH (c:Crate)
WHERE c.name IN $context_a
  AND c.name IN $context_b
RETURN c.name AS qname, 0 AS line, 'crate is a member of both contexts — candidate homonym' AS reason
ORDER BY qname
```

This is a legitimate cross-repo consistency check that exercises the `IN $list` param binding against real data. It is NOT the same predicate as the issue-body example (re-export + adapter), but it addresses the SAME consumer need (detect illegitimate cross-context overlap without LLM judgment).

**The issue-body predicate (re-export + adapter) is deferred to a future RFC** once `RE_EXPORTS` edge is added to the schema (which requires HIR / Phase B). RFC §6 adds this as an explicit non-goal.

**Resolution — update §3.3 Cypher-subset additions table:**
- Strike `re-exported FROM crate IN context-map[portfolio]` row (not expressible today).
- Update the other rows to use real edge vocabulary (`IN_CRATE` not `DECLARED_IN`; `IMPLEMENTS_FOR` stays correct).
- Add note: any predicate referencing a label/edge not in `cfdb_core::schema::{Label,EdgeLabel}` is out of scope.

### C3 — Editorial corrections to §5.1 (clean-arch Change Request)

- §5.1 — remove the false claim that `cfdb-concepts` is "already present" as a dep of `cfdb-query`. With C1 applied, the dep is added to `cfdb-cli`, not `cfdb-query`. `cfdb-cli`'s Cargo.toml does NOT currently list `cfdb-concepts` (verified: `grep cfdb-concepts crates/cfdb-cli/Cargo.toml` returns nothing); the dep is new.
- §5.3 retract the "already present" phrasing (also clean-arch noted).

### C4 — Slice 1 scope description (clean-arch + solid-architect)

- Slice 1 now targets `crates/cfdb-cli/src/param_resolver.rs` (not `cfdb-query`). Cargo.toml change is `crates/cfdb-cli/Cargo.toml` gains `cfdb-concepts = { path = "../cfdb-concepts" }`.
- Slice 1 `Tests:` row for "Self dogfood" updated: the integration test lives in `cfdb-cli/tests/` (not `cfdb-query/tests/`).

### C5 — §6 Non-goal addition (rust-systems Finding 3 + C2 implication)

Add two explicit non-goals to §6:
- **"Not an extension to inner-subquery WHERE grammar."** Cypher subqueries keep the current Compare-only inner-predicate grammar; widening to `IN` / `AND` / `OR` / `NOT` in subquery WHERE is a separate RFC.
- **"Not an extension to schema vocabulary."** The seed predicates use only labels/edges already in `cfdb_core::schema::{Label,EdgeLabel}`. Re-export predicates (the original issue #49 example shape) are deferred to a future RFC that adds `RE_EXPORTS` after HIR Phase B ships.

### C6 — Slice 2 static-check (ddd-specialist non-blocking request)

Slice 2 now includes a new unit test: iterate `.cfdb/predicates/*.cypher`, parse each, walk the AST, and assert every `:Label` and `[:EdgeLabel]` literal resolves to a known variant in `cfdb_core::schema::{Label,EdgeLabel}`. This fails CI early if a future predicate file references a typo'd or out-of-schema vocabulary item — catches C2.b class of errors before they reach dogfood.

### C7 — Update §9 open-questions with verdict-resolutions

Tag each open question with the resolution from the verdicts:
- Q-CA-1 → ANSWERED by clean-arch verdict (direction clean) + solid-architect CRP tie-break (relocates the concern to `cfdb-cli`).
- Q-CA-2 → ANSWERED by clean-arch verdict (keep separate — different contract, different consumer).
- Q-DDD-1 → ANSWERED by ddd-specialist verdict (homonym acceptable; docs duty in Slice 5).
- Q-DDD-2 → ANSWERED by ddd-specialist verdict (keep separate; closed trigger registry vs open predicate library).
- Q-SOLID-1 → ANSWERED by solid-architect verdict (CRP > SRP; relocate to cfdb-cli; no split-crate needed now).
- Q-RS-1 → ANSWERED by rust-systems verdict (defer UDF design; `eval_call` dispatch table at `eval/predicate.rs:111-121` is the documented extension point).

---

## What R1 does NOT change

- The reframe (no new crate, no new grammar, no new evaluator primitive) — ratified by all four.
- The `.cfdb/predicates/` directory name — ddd-specialist ratified; alternative names were considered and rejected.
- The `cfdb check-predicate` verb name — ddd-specialist ratified; distinct from `cfdb check --trigger` (editorial-drift).
- The invariants §4 — all four lenses endorsed; C6 strengthens §4.6 mechanically via the slice-2 static check.
- The issue-decomposition structure (5 slices) — all four endorsed; only Slice 1 target and Slice 2 test surface change.

---

## R2 re-spawn plan

- **rust-systems (R2)** — confirm C2.a + C5's non-goal addition adequately resolve Finding 1/2/3.
- **solid-architect (R2)** — confirm C1's relocation to `cfdb-cli` resolves the CRP concern; confirm §5.3 retraction is mechanically correct.
- **ddd-specialist** — NO R2 spawn required. Verdict was RATIFY; non-blocking request (C6) is incorporated.
- **clean-arch** — NO R2 spawn required. Verdict was RATIFY; editorial corrections (C3) are mechanical.

R2 prompts ask ONLY for confirmation that the R1 revisions address the specific finding(s) — not a full re-review.

---

## Ratification path

When `rust-systems (R2)` and `solid-architect (R2)` both return RATIFY on the targeted changes, `council/49/RATIFIED.md` is authored, sealing the RFC for decomposition. No R3 is anticipated — the R1 changes are concrete and self-contained.
