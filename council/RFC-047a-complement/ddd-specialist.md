# DDD-specialist verdict — RFC-047a

## Verdict

RATIFY — B1/B2/B3 and the correction of record introduce zero new domain vocabulary; every change is query-language or evaluator mechanics, not a stored-fact or bounded-context concern.

---

## Per-blocker analysis

### Correction of record (RFC-047 §3.2/§5 false premise)

Verified. `Param::List(Vec<PropValue>)` exists at `crates/cfdb-core/src/query/ast.rs:54-57`. The evaluator resolves it for `IN` via `eval_expr_list` at `crates/cfdb-petgraph/src/eval/predicate.rs:115-117`, consumed by `Predicate::In` at `:26-33`. The council cited only the raw CLI `--input`/`--params` surface (`commands/query.rs:39,104`), which RFC-047 §3.2 itself already scoped out. The correction is accurate. No vocabulary implication.

### B1 — open-range `*N..` parse gap

Verified at `crates/cfdb-query/src/parser/match_clause.rs:82-86`: both calls to `digits()` are required; there is no `.or_not()` on the second. The AST target is `EdgePattern.var_length: Option<(u32, u32)>` at `crates/cfdb-core/src/query/ast.rs:108`.

DDD position: this is a **Cypher-subset grammar** change only. `var_length` is a query-AST field on `EdgePattern`; it does not appear in `crates/cfdb-core/src/schema/labels.rs` and carries no stored-fact semantics. The complement's classification (query-language addition, not keyspace schema change) is correct. No `SchemaVersion` bump, no new label, no new attribute, no `graph-specs-rust` lockstep required. YAGNI check passes: the fix reuses `Option<(u32, u32)>` unchanged — `*1..` becomes `Some((1, u32::MAX))` with no new AST variant.

### B2 — evaluator depth cap contradicts its own doc

Verified at `crates/cfdb-petgraph/src/eval/mod.rs:64` (`DEFAULT_VAR_LENGTH_MAX = 5`, doc at `:62`) and `crates/cfdb-petgraph/src/eval/pattern/path.rs:205-208`. The cap applies to all var-length patterns, not just the open/omitted-upper form its own doc promises.

DDD position: this is a **query-evaluator mechanics** fix. The constant and the clamp logic live entirely within `cfdb-petgraph/src/eval/` — not in `cfdb-core` schema, not in any stored attribute. Fixing it to honour explicit bounds does not touch any stored fact. The open-form policy question (Q1) is a systems/arch concern; from the DDD lens it is irrelevant which policy is adopted so long as neither option introduces a new stored attribute or label. Neither does.

### B3 — `extract_workspace` emits no resolved `CALLS`

Verified at `crates/cfdb-extractor/src/lib.rs:18` (comment: "Out of scope for v0.1: resolved cross-crate `CALLS` (Item → Item)"). Resolved `CALLS` is HIR-only: `crates/cfdb-hir-extractor/src/emit.rs` / `call_site_emitter/`.

DDD position: the re-specification of 47-A's dogfood to use `cfdb extract --hir` is an **adapter-wiring** decision — which extraction path to call in a test. It introduces no new vocabulary. The `CALLS` edge label already exists at `crates/cfdb-core/src/schema/labels.rs:135`. No label addition, no attribute, no schema change.

---

## Homonym report

The key potential collision the BRIEF asks about: **"depth" / "reachability bound" in the query evaluator vs. the enrich-time reachability pass.**

These are confirmed **distinct concepts** with no naming collision in the codebase:

- **Enrich-time reachability** (`crates/cfdb-petgraph/src/enrich/reachability.rs:246`): function `bfs_call_graph`, unbounded, writes `reachable_from_entry` / `reachable_from_production_entry` as stored attributes. It does not use `DEFAULT_VAR_LENGTH_MAX`.
- **Query-time var-length bound** (`crates/cfdb-petgraph/src/eval/mod.rs:64`, `eval/pattern/path.rs:205-208`): `DEFAULT_VAR_LENGTH_MAX`, applies to the `traverse_bfs` evaluator. It does not write any stored attributes.

The term "blast radius" appears exactly once in the codebase in a test comment at `crates/cfdb-cli/tests/impact_seed_binding.rs:156` — it is informal test prose, not a type, label, or stored attribute. No homonym risk.

No new label, attribute, or concept is introduced by this complement. The `var_length` field (`ast.rs:108`) is confined to `cfdb-core::query::ast` (query-AST layer) and is read only by `cfdb-petgraph/eval/pattern/path.rs`. It never crosses into `cfdb-core::schema` (stored-fact vocabulary).

---

## Context relationship analysis

This complement touches two contexts only:

1. **Query-language context** (`cfdb-query` parser + `cfdb-core` AST) — B1 is a grammar extension within this context. The `(u32, u32)` tuple is already the published language for var-length in the `EdgePattern` AST; reusing it for `*N..` is a Conformist extension within the same context, not a context crossing.

2. **Evaluator context** (`cfdb-petgraph/eval`) — B2 is an internal alignment. `DEFAULT_VAR_LENGTH_MAX` never crosses into `cfdb-core` schema; it is implementation detail of the evaluator, not a contract exported to other bounded contexts.

B3 is a test-wiring choice: which extraction adapter to invoke. The `CALLS` edge (`labels.rs:135`) is already in the Published Language of `cfdb-core`. No new concept is needed.

---

## Contested-question positions (Q3 and Q4 per BRIEF)

### Q3 — HIR dogfood cost and 47-0 / 47-A boundary

The complement's split is linguistically sound from a DDD perspective: 47-0 owns "the query form is expressible and not silently truncated" (pure mechanics), 47-A owns "the canonical query behaves correctly against real CALLS data" (behaviour contract). These are two different reasons to change.

On the HIR-dogfood cost question (solid/rust-systems to lead): the DDD lens has no objection to the heavier `cfdb extract --hir` path in 47-A's test. The self-dogfood must exercise real `CALLS` edges — using `extract_workspace` would prove nothing about CALLS reachability because `extract_workspace` emits no resolved `CALLS` (`lib.rs:18`). A lighter in-process CALLS-resolution path does not currently exist and is not in scope. The test shape in 47-A as written is the only faithful dogfood for this fact kind. "CALLS graph" is not a new concept cfdb must introduce; it is an existing stored fact (`labels.rs:135`). The dogfood asserts existing vocabulary. No vocabulary concern.

### Q4 — correction of record: amend or supersede?

The complement approach (supersede RFC-047 §3.2/§5 in place, keep the parent as a historical record) is correct DDD hygiene. Amending a ratified RFC in place risks confusing the ratification trail — the council's verdict was made against a document that contained the false claim. Keeping the parent as-is and letting the complement be the canonical superseding document preserves the audit chain. The complement's header is explicit: "This supersedes its §3.2 mechanics." This is the right model. RATIFY on Q4.

---

## Test-surface prescription notes

The prescribed 4-row `Tests:` blocks for 47-0 and 47-A (complement §7) are correct from a DDD standpoint:

**47-0:** The `Self dogfood: none — rationale: query-mechanics slice` row is valid. The mechanics change (B1 parser + B2 evaluator) has no stored-fact surface to dogfood. The unit tests (parser round-trip for `*1..` → `(1, u32::MAX)`, explicit-bound honour check) are the right signal. The salvage test in `crates/cfdb-cli/tests/impact_seed_binding.rs` is the integration anchor — it exercises the full composition (list binding + var-length traversal + `IN $seeds` membership) against a fact-injected fixture, which is the closest thing to a dogfood without requiring HIR.

**47-A:** `Self dogfood: cfdb extract --hir` is the only faithful choice given B3. The assertion shape (seed a known leaf fn in cfdb-core, assert known callers in cfdb-petgraph/cfdb-cli appear) is specific and verifiable. No suggested changes.

No corrections to either `Tests:` block.

---

## Summary

This complement is a **correction + three mechanics fixes** with zero domain vocabulary consequence. The schema surface is clean: `labels.rs` is untouched, `SchemaVersion` is untouched, no new node or edge label appears. The `var_length` field is a query-AST concern already present in `cfdb-core/src/query/ast.rs:108` and confined to the query/evaluator layer. The two BFS algorithms ("enrich-time reachability" and "query-time var-length traversal") are distinct, separately-named concepts with no collision. DDD has no blocking concern.

**RATIFY.**
