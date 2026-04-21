# R2 Verdict — rust-systems

## Read log
- [x] council/49/SYNTHESIS-R1.md
- [x] docs/RFC-034-query-dsl.md §3.3, §3.5, §6, §7 Slice 2

## Finding 1 (positive EXISTS)
VERDICT: RESOLVED

RFC §3.5 now shows a top-level `MATCH (c:Crate) WHERE c.name IN $context_a AND c.name IN $context_b` query with no `EXISTS { }` form of any kind; §6 (Non-goals) explicitly states "Not positive `EXISTS { }` in the parser (R1 C5)" with citation to `parser/predicate.rs:43-48`, `ast.rs:147`, and `eval/predicate.rs:45-47` — precisely the three locations identified in Finding 1.

## Finding 2 (inner WHERE grammar)
VERDICT: RESOLVED

The revised §3.3 table shows the one `NOT EXISTS` usage (`WITHOUT <Trait> impl`) with inner WHERE `WHERE t.name = '<Trait>'` — a Compare predicate, not `IN` — and §6 adds the explicit non-goal "Not an extension to inner-subquery WHERE grammar (R1 C5)" citing `parser/predicate.rs:131-139` (the Compare-only `inner_pred` combinator, confirmed by reading the source). No seed predicate in §3.5 uses `IN` or any multi-operator form inside a subquery WHERE.

## Finding 3 (non-goal)
VERDICT: RESOLVED

RFC §6 now carries two new non-goal bullets added by R1 C5: one banning inner-subquery WHERE extension (with file:line citation) and one banning positive `EXISTS { }` (with three file:line citations). Both match the exact text requested in Finding 3.

## Overall R2 verdict
VERDICT: RATIFY

All three R1 findings are cleanly resolved. The RFC's canonical §3.5 examples now use only parser-supported constructs: top-level `MATCH` + `WHERE c.name IN $list` for positive set membership, and `NOT EXISTS { MATCH ... WHERE t.name = '<value>' }` (Compare-only inner predicate) for trait-impl absence. The §3.3 table correctly defers the `RE_EXPORTS`-dependent row to a future RFC. The §6 non-goals section now explicitly forecloses inner-subquery WHERE extension, positive `EXISTS`, and schema vocabulary addition — all three were the open surface areas identified in R1. Seed predicate #1 ("context-homonym-crate-in-multiple-contexts") is grammatically valid against the current parser: `MATCH (c:Crate) WHERE c.name IN $context_a AND c.name IN $context_b` uses `Predicate::In` (supported at `predicate.rs:34-38`) and `Predicate::And` (supported at `predicate.rs:76-81`) at the top level — no subquery, no positive `EXISTS`. The RFC is sealed from the rust-systems lens and is ready for issue decomposition.
