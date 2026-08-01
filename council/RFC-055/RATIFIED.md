# RFC-055 — Council ratification record

- RFC: `docs/RFC-055-correlated-not-exists.md` (correlated `NOT EXISTS`, query-subset v0.2)
- Date: 2026-08-01
- Mechanism: 4-lens agent-team council (CLAUDE.md §2.3) — teammates with shared task list + cross-messaging; every foundation claim verified at source by at least one lens, most by two or more.
- Outcome: **RATIFIED 4/4, unanimous** (R1: 1× RATIFY, 3× REQUEST CHANGES → author fold → R2: 4× RATIFY). No author override needed.

## Verdicts

| Lens | R1 | R2 (final) | Load-bearing verifications |
|---|---|---|---|
| clean-arch | REQUEST CHANGES | **RATIFY** | Dependency Rule structurally unviolatable (cfdb-query is dev-only in cfdb-petgraph, enforced by `architecture_dep_rule.rs`); found the 3-of-8 evidence miscitation AND the `e1a58e9` stale-grammar archaeology; caught R2's own scope drift (correlation notice needs an additive cfdb-core `WarningKind` → §4 disclosure); tightened `run_seeded` to eval-module-private |
| ddd-specialist | REQUEST CHANGES | **RATIFY** (unconditional) | Caught the RFC's own worked-example edge bug (`INVOKES_AT` never targets `:Item` → `CALLS`); extended the prose-truth pass to `check.rs`/`split-resolution-fences.md` then narrowed it (t1/t3 cite the OPTIONAL-MATCH limitation — correctly OUT); required the shadowing-visibility fence (folded as the §3.1.7 notice — confirmed the observability-not-alarm form); pushed `resolve_endpoint` from pin-or-fix to required fix |
| solid-architect | RATIFY → revised REQUEST CHANGES | **RATIFY** | ONE-slice CCP ruling; ISP by consumer enumeration; SDP/ADP boundary by construction; independently root-caused the §6 falsity via `e1a58e9`'s commit message + `parse_not_exists_with_in_predicate`; found `apply_optional_row`'s live stream-reseeding precedent; required the notice cardinality pin (O(1) per NOT EXISTS, not per outer row) |
| rust-systems | REQUEST CHANGES | **RATIFY** | Foundation claim confirmed at source (seed row `mod.rs:196`; bound-var reuse+re-check `pattern.rs:71-123`); found the FROM/TO endpoint anchoring asymmetry → target dogfood upgraded to an asserted wall-time bound (#409 class); found the `resolve_endpoint` kind-mismatch bug; the WITH-rescoping interaction; verified the `WarningKind` variant is non-breaking via workspace-wide exhaustive-match sweep |

## Council-corrected foundations (recorded so they are not re-derived wrong)

1. **Inner-WHERE subquery grammar was NEVER the gap** (three lenses, three verification paths): commit `e1a58e9` (2026-04-25, "wire recursive predicate through subquery_parser", #271/EPIC #273) shipped the full recursive predicate grammar inside `NOT EXISTS { ... WHERE ... }`. `docs/query-dsl.md:164`, `.cfdb/predicates/README.md:25`, `raid-completeness.cypher:21-26`, and RFC-034 §6's own text were all stale (W17-audit class). RFC-055's truth pass corrects them.
2. **`resolve_endpoint` FROM-endpoint kind-mismatch** (`path.rs:134-138`) silently treats an edge/value-bound name as fresh — pre-existing bug, independently reachable, fixed in 55-A as its own commit.
3. **Bound-TO endpoints are never anchored** — the canonical anti-join shape is O(outer_rows × (V+E)); dormant until this RFC. Target dogfood is an asserted bound; TO-endpoint anchoring is the named follow-up if scale demands.
4. **Post-WITH `B`** = the WITH-projected row only (`with_clause.rs:15-50`); specified §3.1.1, test-pinned.

## Decomposition

One slice: **55-A** (re-scopes #546), Tests block prescribed in RFC §7 — carried to the issue verbatim per CLAUDE.md §2.4.
