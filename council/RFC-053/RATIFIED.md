# RATIFIED — RFC-053 (`:MatchSite` + `MATCHES_ON` enum-dispatch facts)

**Council:** 2026-07-15, agent-team (4 lens teammates, mailbox + shared task list), `CLAUDE.md §2.3`. Base `origin/develop`, worktree branch `docs/rfc-053-matches-on`.
**Outcome:** **RATIFIED ×4** (R1: 4× REQUEST CHANGES → amendments applied in R2/R2.1 → R2 unanimous RATIFY).

## Verdicts

| Lens | R1 | R2 (after amendments) |
|---|---|---|
| ddd-specialist | REQUEST CHANGES | RATIFY |
| clean-arch | REQUEST CHANGES | RATIFY |
| solid-architect | RATIFY architecture / REQUEST CHANGES disposition | RATIFY |
| rust-systems | REQUEST CHANGES | RATIFY |

No lens dissented on the core design at any point: site node (`:MatchSite`) with name-level
`matched_path` + optional resolved `MATCHES_ON` edge, per the `:CallSite` precedent, was found
sound by all four lenses independently in R1.

> Note on evidence trails: the shared task list (which held per-lens verdict metadata) was
> reset mid-council by the platform; the durable record is the deliberation mailbox thread,
> summarized here, plus the R1 findings captured inline in the RFC's §5.

## What the council established

1. **Correction of record (all four lenses, independently source-verified).** The R1 draft's
   flagship evidence — "Visibility AST→wire→enum at 3 sites (#478)" — was wrong three ways:
   #478 is an unrelated issue (`build_item_props` prop-key drift); the real historical
   Visibility split-brain was already fixed by boy-scout #107 (commit `2aedd013`, 2026-04-20)
   — five days BEFORE the audit EPIC #279 (2026-04-25) that documented it as live; and the
   anatomy was never "3 sites matching `syn::Visibility`" (one external-type site; `as_wire_str`
   matches the workspace enum; `FromStr` matches `&str` and emits nothing under §3.1). §1 and
   the Appendix were rebuilt on the verified history; the staleness itself became evidence for
   the RFC's thesis (prose debt records rot; tree-derived fences don't). Positive side effect:
   53-C's ordering dependency dissolved — the zero-violation Visibility baseline has existed
   since April, so the first fence is unblocked.
2. **Vocabulary (ddd).** `matched_type` → `matched_path` (it is an arm-pattern path prefix
   under the `:CallSite.callee_path` doctrine; `matched_type` is reserved for a future HIR
   scrutinee-resolution tier). `DISPATCHES_AT` → `MATCHES_AT` ("dispatch" is established cfdb
   vocabulary for call-target resolution; one verb root across the family). `wildcard` kept —
   RFC-044 §3.7's own term; the draft's "catch-all" gloss was unsourced.
3. **Id scheme (clean-arch).** No `cfdb_core::qname` call-site id helper exists — `:CallSite`
   ids are a deliberate extractor-local inline format (RFC-032 §3 resolver-discriminator).
   `:MatchSite` follows: `matchsite:{fn_qname}:{prefix}:{local_idx}`, prefix mandatory in the
   id, dedup-per-match-expression before the occurrence counter increments.
4. **Module discipline (solid).** `match_visitor/{mod.rs, prefix.rs}` directory from day one
   (`type_render.rs` 496/500, `emit/mod.rs` 452/500); emission not routed through `emit/mod.rs`;
   in-slice boy-scout: factor the byte-duplicated `walk_macro_tokens` out of
   call_visitor/literal_visitor into one shared helper with first-ever unit tests.
5. **Resolver reuse settled at primitive level (rust-systems ↔ clean-arch ↔ solid, converged).**
   Standalone short `resolve_deferred_match_targets` calling `resolve_type_string` +
   `build_last_segment_index` (gaining first direct unit tests in 53-B; visibility stays
   private if the tests land as an inline `#[cfg(test)]` child module — R2 solid refinement).
   No generic combinator (genuine divergence: tuple arity, tier count, `kind="enum"` filter);
   no copied orchestration (the debt class this RFC fences).
6. **`matches!()` disposition (rust-systems + solid, doubly confirmed after a mid-council
   reversal-and-retraction).** Named §6 non-goal / recall limit #3 — its `expr, pat [if guard]`
   grammar fits none of the three macro re-parse tiers. Forward guidance recorded in §6: the
   future tier-4 is known-low-cost (`syn::Pat::parse_multi_with_leading_vert`, syn 2.0.117
   `pat.rs:383`), placed as a `match_visitor`-local wrapper (ISP), triggered only by a real
   fence consumer. `crates/cfdb-cli/tests/signatures.rs` is NOT a motivating instance —
   `tests/` targets are never extracted (`lib.rs:346` lib-or-bin filter); the in-scope evidence
   is `matches!(` in 26 production `src/` files.
7. **Macro claim precision (rust-systems).** Macro *invocation* bodies ARE extracted (shared
   `walk_macro_tokens`, consistent with call/literal facts); only `macro_rules!` *definitions*
   are opaque. §3.3/§3.6/§6 state the three evasion paths precisely.
8. **Three named recall limits**, all fixture-measured, never silent: single-segment patterns
   (glob imports), the lowercase-ident wildcard heuristic, `matches!()` invocations.
9. **Verticality + schema mechanics (solid).** 53-A/53-B are genuine vertical slices (RFC-045
   45-D precedent); ONE SchemaVersion bump (V0_6_0→V0_7_0) landing in 53-A with the graph-specs
   lockstep; 53-B adds no schema surface. §3.6 fence guardrail: one fence file per fenced type,
   max one canonical-site NOT-clause.

## Issue decomposition (to file per §2.4)

Per RFC-053 §7, with `Tests:` blocks carried verbatim into the issue bodies:

- **53-A** — `:MatchSite` + `MATCHES_AT` end-to-end: directory module, `walk_macro_tokens`
  factoring boy-scout, V0_7_0 bump + graph-specs lockstep PR.
- **53-B** — `MATCHES_ON` resolution pass: standalone resolver fn on shared primitives,
  resolution-rate target-dogfood metric (the HIR-tier go/no-go number).
- **53-C** — fence templates (both forms) + first live fence: `syn::Visibility` regression
  guard. Unblocked today; `--format` instance requires liveness re-verification before use.
