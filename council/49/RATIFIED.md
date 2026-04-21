# Ratified — council-49-query-dsl

**Date:** 2026-04-21
**Team:** `council-49-query-dsl`
**RFC:** `docs/RFC-034-query-dsl.md` (R1 REVISED)
**Tracking issue:** #49
**Parent EPIC:** #34
**Members:** clean-arch, ddd-specialist, solid-architect, rust-systems (per project CLAUDE.md §2.3 — 4 of 4 required lenses)

---

## Verdict

**RATIFIED by all 4 lenses.** No R3 required. The RFC is sealed for decomposition into forge issues per §7.

## Verdict matrix

| Lens | R1 | R2 | Net |
|---|---|---|---|
| clean-arch | RATIFY (2 editorial corrections — applied as R1 C3) | — (not required — R1 verdict was already RATIFY) | **RATIFY** |
| ddd-specialist | RATIFY (1 non-blocking request — applied as R1 C6) | — (not required) | **RATIFY** |
| solid-architect | REQUEST CHANGES — CRP relocation (R1 C1) | RATIFY (all 3 items RESOLVED) | **RATIFY** |
| rust-systems | REQUEST CHANGES — parser-reuse contradictions (R1 C2, C5) | RATIFY (all 3 findings RESOLVED) | **RATIFY** |

## Load-bearing decisions sealed

1. **Reframe ratified.** Issue #49's "New crate `cfdb-query-dsl/` + new DSL grammar" is REJECTED. The shippable surface is: param-resolver module + named-predicate library + new CLI verb. No new crate, no new grammar, no new evaluator primitive, no `SchemaVersion` bump.
2. **Param-resolver placement: `cfdb-cli`** (not `cfdb-query`). Solid-architect's CRP tie-break wins over clean-arch's SDP-only RATIFY. `cfdb-query` has zero runtime consumers of a filesystem-reading module; `cfdb-cli` is the sole consumer. Placement preserves `cfdb-query`'s instability metric (I=0.33).
3. **New direct dep: `cfdb-cli → cfdb-concepts`.** Previously absent; added by Slice 1.
4. **Seed predicate #1 reframed.** Original "context-member-reexport-without-adapter" was unshippable (RE_EXPORTS edge does not exist; re-export resolution is Phase B / HIR). Replaced by "context-homonym-crate-in-multiple-contexts" which exercises the same param-resolver + composition pathway using real schema vocabulary and supported parser constructs.
5. **Re-export predicate DEFERRED** to a future RFC. §6 non-goals now explicitly cites this as outside scope until HIR Phase B ships `RE_EXPORTS` edge emission.
6. **Parser extension points sealed.** No positive `EXISTS { }`. No `IN`/`AND`/`OR`/`NOT` in subquery inner WHERE. Both explicitly forbidden by §6 non-goals.
7. **UDF mechanism deferred.** `eval_call` dispatch table at `crates/cfdb-petgraph/src/eval/predicate.rs:111-121` is the documented extension point. No UDF work in this RFC.
8. **Verb separation sealed.** `cfdb check-predicate` and `cfdb violations --rule <path>` are legitimately separate verbs (different contract, different consumers, different change vectors). `cfdb check --trigger T1` (from #101) is also legitimately distinct from `cfdb check-predicate`.
9. **Homonym accepted.** `Predicate` (AST node, `cfdb_core::query::Predicate`) vs `predicate` (file, `.cfdb/predicates/*.cypher`) — same bounded context, different layers. Documentation duty in Slice 5 (`docs/query-dsl.md`).

## Canonical ownership assertions

| Concept | Owning crate/dir | Canonical authority |
|---|---|---|
| Param resolver | `cfdb-cli` (R1 C1 relocation) | `crates/cfdb-cli/src/param_resolver.rs` — single module, `pub(crate)` surface |
| Context-map vocabulary | `cfdb-concepts` | `cfdb_concepts::load_concept_overrides` — ONLY loader (invariant §4.6) |
| Predicate library | `.cfdb/predicates/` | sibling of `.cfdb/queries/` + `.cfdb/concepts/`; flat layout |
| check-predicate verb | `cfdb-cli` | `crates/cfdb-cli/src/check_predicate.rs` + dispatch arm in `main_dispatch.rs` |
| Query parsing | `cfdb-query` | unchanged — no new module, no new dep, no new responsibility |
| Schema vocabulary | `cfdb-core` | unchanged — no new label, no new edge, no SchemaVersion bump |
| Evaluator | `cfdb-petgraph::eval` | unchanged — no new primitive, no new dispatch arm |

## Forbidden creations (cross-RFC)

- A second inline TOML parser for `.cfdb/concepts/*.toml` anywhere in the workspace (invariant §4.6).
- A `cfdb-query-dsl` crate (reframe §2.2).
- A positive `EXISTS { ... }` parser arm in `cfdb-query::parser::predicate_parser` (non-goal §6).
- An `IN`/`AND`/`OR`/`NOT` extension to subquery inner WHERE (non-goal §6).
- A `RE_EXPORTS` edge in `cfdb-core::schema::EdgeLabel` within this RFC's scope (deferred to future HIR-Phase-B RFC).
- A ratchet/allowlist/baseline file for predicate-violation counts (invariant §4.3).
- A SchemaVersion bump within this RFC (reframe §2.2 + invariant §4.4).

## Decomposition → forge issues

Per project CLAUDE.md §2.4, the RFC's §7 issue decomposition is filed as vertical-slice forge issues, each linking back to this RFC and carrying its prescribed `Tests:` block verbatim. Sequencing:

```
slice-1 (cfdb-cli::param_resolver) ───┐
                                       ├─→ slice-3 (check-predicate verb) ─→ slice-4 (dogfood+det)
slice-2 (.cfdb/predicates/ seed + static check) ─┘                                         │
                                                                                             ↓
                                                                                      slice-5 (docs)
```

Issue numbers will be allocated at filing time; sub-issue map updated in this file as a `## Filed sub-issues` section appended in a follow-up commit.

## Filed sub-issues

_(populated in a follow-up commit after forge_create_issue for each slice)_

## Shutdown

- Team `council-49-query-dsl` → all members idle → `TeamDelete` after RATIFIED.md commits.
- RFC status in header → "RATIFIED" (from "R1 REVISED").
- #49 → updated with `Supersedes-by: RFC-034` comment once the RFC ships; implementation issues filed from the decomposition take over the actual work.
