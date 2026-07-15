# RFC-045 — RATIFIED

**RFC:** `docs/RFC-045-polyglot-relationship-edges.md`
**Date:** 2026-06-04
**RFC SHA base:** `553627b` on `origin/develop`
**Parent:** META #266 (cfdb multi-language roadmap) — Phase 4
**Council:** `rfc-045-council` (TeamCreate per global CLAUDE.md §2b) — clean-arch, ddd-specialist, solid-architect, rust-systems
**Pipeline:** draft → Council R1 → candidate → coder dry-run → revised candidate → Council R2 → ratified

## Outcome
- **Council R1: 4/4 REQUEST CHANGES** (`SYNTHESIS-R1.md`). 5 blockers (D1 homonym; cfdb-extractor-shared syn-isolation ×2; TS implements path; CallSite id namespace) + changes; all folded.
- **Coder dry-run** (`DRYRUN.md`): two coders compiled scratch implementations and found 4 blockers review could not — `target_resolved` unqueryable (ingest drops dangling edges, `graph.rs:227`), TS has no method `:Item`s, "additive cfdb-core attr" breaks 3 frozen surfaces, CALLS≈0. Forced D2 reversal (emit-only-resolved), a new prerequisite slice 45-D0, the coordinated-edit checklist, and the callee_path-based "callers" posture.
- **Council R2: 4/4 VALIDATE.** Amends folded: clean-arch NIT (resolver="syn" backfill assertion); ddd ×2 (TS callee_path textual; static:: precision); rust-systems ×2 (implements_clause fallback shapes; 45-D0 public_field_definition arrow methods).

## Per-lens R2 verdicts
| Lens | R1 | R2 | Key R2 ruling |
|---|---|---|---|
| clean-arch | REQUEST CHANGES | **VALIDATE** | D2 reversal is referential-integrity-correct, not policy; producer→core direction clean (resolver concept already core-owned). |
| ddd-specialist | REQUEST CHANGES (1 BLOCKER) | **VALIDATE** | "callers via callee_path"=honest syn-parity; php/ts_construct homonym-free; external→no-edge vocabulary-clean. 2 prose amends. |
| solid-architect | REQUEST CHANGES (1 BLOCKER) | **VALIDATE** | inline (no common crate) CRP-correct; FROZEN_NARRATIVE_DIGEST recompute is sanctioned, NOT a ratchet; cfdb-core stability unperturbed. |
| rust-systems | REQUEST CHANGES (2 BLOCKERS) | **VALIDATE** | all grammar tables verified vs tree-sitter-php-0.23.11 / -typescript-0.23.2; Invariant 5 scoping sound; determinism sound. 2 textual amends. |

## Design decisions ratified
- **D1-a**: direct `:Item{class}-[:IMPLEMENTS]->:Item{interface}` (no synthetic impl-block); `resolver` attr disambiguates the Rust impl-block→trait homonym.
- **D2 (revised)**: emit `IMPLEMENTS` iff both endpoints in-workspace (two-pass); external→no edge (closed-world, documented); no `target_resolved`. No stub nodes.
- **D3-a**: defer `extends`; documented as a known false-negative on transitive implementors + negative-assertion test. (45-E optional, only if a consumer needs `EXTENDS` → V0_6_0 + lockstep.)
- **D4**: per-language qname endpoints; no shared normalizer; inline (no `cfdb-extractor-common`).
- **CALLS**: PHP static in-workspace only; **TS zero** (syn-parity); "callers of X" via `:CallSite.callee_path` + `INVOKES_AT`.
- **cfdb-core**: additive `resolver` on IMPLEMENTS + `:CallSite` enum extension + `php_construct`/`ts_construct` documented + INVOKES_AT descriptor `from/to` bugfix. **No SchemaVersion bump**, but coordinated edits (digest + spec md + pins) per §4.1.

## Issue decomposition → backlog (under #266)
45-A (PHP IMPLEMENTS + cfdb-core resolver attr) · 45-B (TS IMPLEMENTS + ts_construct) · 45-D0 (TS method :Item — prereq) · 45-C (PHP CallSite/CALLS) · 45-D (TS CallSite, zero CALLS). Optional 45-E (EXTENDS). Each carries the prescribed `Tests:` block.

## Ratification basis
4/4 RATIFY-equivalent (R2 VALIDATE) with all amends folded into the same artifact. No author override required.
