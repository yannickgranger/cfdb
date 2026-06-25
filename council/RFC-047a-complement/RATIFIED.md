# RATIFIED — RFC-047a (impact query mechanics complement)

**Council:** 2026-06-25, agent-team (4 lens teammates, mailbox + shared task list), `CLAUDE.md §2.3` + global `§2b`. Base `origin/develop` @ `018e766`.
**Outcome:** **RATIFIED ×4** (2 RATIFY + 2 REQUEST-CHANGES → amendments applied → RATIFY).

## Verdicts

| Lens | R1 | After amendments | Verdict file |
|---|---|---|---|
| ddd-specialist | RATIFY | RATIFY | `ddd-specialist.md` |
| clean-arch | RATIFY | RATIFY | `clean-arch.md` |
| rust-systems | REQUEST CHANGES | RATIFY | `rust-systems.md` |
| solid-architect | REQUEST CHANGES | RATIFY | `solid-architect.md` |

Each lens verified the load-bearing claims at `file:line`. No lens dissented on the design — both REQUEST-CHANGES were test-prescription / documentation amendments with explicit flip conditions, now applied to `docs/RFC-047a-impact-query-mechanics.md` §3.2 + §7.

## What the council established

1. **Correction of record (all four).** RFC-047 §3.2/§5's blocking finding — *"no list-binding path exists today"* — is **false and was false at deliberation time**. The in-process `Param::List` + `IN $param` path ships (`ast.rs:54-57`, `eval/predicate.rs:115-117`, `param_resolver.rs:90` #145, `check-predicate` #147, `raid_plan_queries.rs` #205). The original council inspected only the out-of-scope raw `--params`/`--input` surface. Original 47-0 ("land list-binding") is **closed: capability pre-exists**.

2. **Three real blockers, file:line-confirmed by ≥2 lenses each:**
   - **B1** — open-range `*N..` doesn't parse (`match_clause.rs:82-86`). Fix in `cfdb-query`; reuse `Option<(u32,u32)>` (`ast.rs:108`), no new AST variant.
   - **B2** — var-length silently caps at 5 (`eval/mod.rs:64`, `path.rs:205-208`), contradicting the const's own doc. **Live bug** (rust-systems, confirmed clean-arch): three shipped queries truncated today, incl. the CI ban rule `.cfdb/queries/vsb-multi-resolver.cypher:67` (`*1..10`→5). **Not a regression** — `DEFAULT_VAR_LENGTH_MAX` has been 5 since portage `8ed8b97`.
   - **B3** — `extract_workspace` emits no resolved CALLS (`lib.rs:18`); resolved Item→Item CALLS is HIR-only.

## Resolved contested questions

- **Q1 (crux) — open-form `*N..` semantics → visited-set-unbounded.** O(V+E) confirmed (`path.rs:216-232`: `visited.insert` at `:230` is at enqueue, not gated by `max_depth`). A cap buys nothing; unbounded matches RFC-047 §3.2 + the already-unbounded enrich BFS (`reachability.rs:246`). **Mandatory:** policy comment at the `u32::MAX` branch in `traverse_bfs`.
- **Q2 — explicit-bound clamp → unconditional fix in 47-0.** No Q1 dependency. Honour explicit finite bounds as written; named regression test (`*1..10` traverses 10). **Caveat:** re-verify cfdb-self `violations` gate stays zero after un-truncating.
- **Q3 — 47-0/47-A boundary + HIR dogfood → split.** Mechanics (B1+B2) → 47-0; canonical query + dogfood → 47-A. HIR dogfood is CCP-split into `impact_hir_dogfood.rs`, `integration-live`-gated, in-process `build_hir_database` + `extract_call_sites` (no shell-out; no `extract_workspace_hir`, `lib.rs:89-92`).
- **Q4 — correction of record → complement supersedes** (amending a ratified RFC in place corrupts the audit trail). Forward-pointer added to RFC-047 §3.2/§5.

## Re-cut slices (supersede RFC-047 §7 for 47-0/47-A)

- **47-0 (re-framed)** — var-length reverse-reachability query mechanics: B1 (open-range parse) + B2 (honour explicit bounds, unbounded open form) + the policy comment + the salvage fixture test (`impact_seed_binding.rs`, `IMPACT_QUERY` → `*1..`) + named parser/evaluator unit tests + the cfdb-self zero-violation re-check. **Supersedes the closed list-binding 47-0 (#488).**
- **47-A (amended)** — canonical reverse-reachability query (now parseable) + `impact_hir_dogfood.rs` HIR self-dogfood (`integration-live`).
- **47-B / 47-C** — unchanged (47-B additionally owns the `--max-depth` CLI flag mapping to `*1..N`).

## Follow-ups for the maintainer

- Re-file/re-scope the slice issues per the re-cut decomposition (#488 47-0 re-framed; #489 47-A amended).
- The B2 live-truncation is a real shipped-gate bug; it rides 47-0 (no separate `fix:` issue needed since 47-0 fixes it directly), but is worth calling out in the 47-0 PR body.
