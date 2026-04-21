# Verdict — rust-systems

## Read log
- [x] docs/RFC-034-query-dsl.md
- [x] council/49/BRIEF.md
- [x] crates/cfdb-query/src/parser/mod.rs
- [x] crates/cfdb-query/src/parser/predicate.rs
- [x] crates/cfdb-petgraph/src/eval/mod.rs
- [x] crates/cfdb-petgraph/src/eval/predicate.rs
- [x] crates/cfdb-core/src/query/ast.rs
- [x] crates/cfdb-query/Cargo.toml
- [x] crates/cfdb-cli/Cargo.toml
- [x] crates/cfdb-concepts/Cargo.toml

---

## Reframe

VERDICT: RATIFY (the reframe of "no new DSL / no new crate"), subject to the REQUEST CHANGES below on two specific defects surfaced by parser-reuse stress-testing.

The reframe decision — extend `cfdb-query` with a `param_resolver` module, place the verb in `cfdb-cli`, and use the existing chumsky parser for all predicate forms — is architecturally sound from a Rust systems standpoint. No new parser crate, no new proc-macro boundary, no new AST enum, and the `cfdb-concepts` dependency direction is pre-approved by the crate's own Cargo.toml comment at `crates/cfdb-concepts/Cargo.toml:21`: "`cfdb-query` must be able to depend on this crate without accidentally pulling in the 1M+ LoC HIR stack." The `cfdb-concepts` dep is trivially addable to `crates/cfdb-query/Cargo.toml` (it brings only `serde + thiserror + toml`, all already workspace-deps). The reframe is correct; the issues are in two specific parser claims.

---

## Per-lens questions

### Q-RS-1: Is it safe to assume every future predicate expresses as Cypher, or should this RFC pre-document a UDF mechanism?

The existing evaluator (`crates/cfdb-petgraph/src/eval/predicate.rs:111-121`) already has a hard-wired UDF dispatch table (`eval_call`), currently with five entries (`regexp_extract`, `size`, `starts_with`, `ends_with`, `last_segment`, `signature_divergent`). The extension point already exists — it is a `match name { ... }` arm. Future predicate forms that Cypher's structural operators cannot express (e.g., semantic diff, cross-keyspace lookups, source-text inspection) can be added as new `Expr::Call` variants in the evaluator without any AST or parser change.

The "shell-grep escape hatch" from issue #49 is addressed by `MATCH (f:File) WHERE f.path =~ $pat` (already supported — `Predicate::Regex` is wired at `eval/predicate.rs:35-44`), so no shell-out UDF is needed now. The RFC correctly classifies UDFs as a future RFC concern. Documenting the extension point in §6 Non-goals ("A future RFC can add UDFs if a predicate form emerges that Cypher + param resolver cannot express") is sufficient — the mechanism already exists in code.

**Verdict on Q-RS-1:** SAFE to defer. The UDF dispatch table at `eval/predicate.rs:111-121` is the documented extension point. The RFC's §6 non-goal statement is adequate; no new extension-point surface is needed.

---

## Critical findings (grounds for REQUEST CHANGES)

### Finding 1 — Positive `EXISTS { }` is not in the v0.1 parser; the RFC §3.5 canonical example is invalid

RFC §3.5 contains this example query (lines 183-191):
```cypher
AND EXISTS {
  MATCH (i)-[:RE_EXPORTS]->(i2:Item)-[:DECLARED_IN]->(c2:Crate)
  WHERE c2.name IN $context_portfolio
}
AND NOT EXISTS {
  MATCH (i)-[:IMPLEMENTS_FOR]->(t:Trait)
  WHERE t.name = $adapter_trait
}
```

The positive form `EXISTS { MATCH ... }` does NOT exist in the parser or AST:

- `crates/cfdb-query/src/parser/predicate.rs:43-48` — `predicate_parser` has one subquery form: `kw("not").ignore_then(kw("exists"))...`. There is no `kw("exists")` without a preceding `kw("not")`.
- `crates/cfdb-core/src/query/ast.rs:119-153` — `Predicate` enum has only `NotExists { inner: Box<Query> }`. There is no `Exists` variant.
- `crates/cfdb-petgraph/src/eval/predicate.rs:45-47` — `eval_predicate` matches on `Predicate::NotExists`; no `Exists` arm.

The RFC's canonical example query in §3.5 will not parse with the current grammar. The `AND EXISTS { ... }` clause would produce a `ParseError::Syntax` at that position. The RFC's claim "no new parser, no new AST variant" is therefore contradicted by its own §3.5 example: either the example must be rewritten to use `NOT NOT EXISTS` (double-negation, ugly) or an `Exists` AST variant + parser arm + evaluator arm must be added (which invalidates the "no new AST variant" claim).

This is a scope-consistency defect: the RFC commits to no new AST variant, then shows an example that requires one. The author must choose one of:

1. Rewrite §3.5 and the RFC-table row for "re-exported FROM crate" to avoid positive `EXISTS` — use `NOT NOT EXISTS` or restructure the query with `WITH ... WHERE` to achieve the same semantics without the positive subquery form.
2. Add `Predicate::Exists { inner: Box<Query> }` to `cfdb-core/src/query/ast.rs`, the matching parser arm to `cfdb-query/src/parser/predicate.rs`, and the evaluator arm to `cfdb-petgraph/src/eval/predicate.rs`, and revise §3.2 / §5.4 to acknowledge the AST addition.

Option 1 is preferred — it preserves the "no new AST variant" invariant. Option 2 is also acceptable but must be explicitly scoped in the RFC.

### Finding 2 — Inner WHERE of `NOT EXISTS` is Compare-only; the §3.5 example uses `IN` inside `EXISTS`

The inner predicate parser inside `subquery_parser` (`predicate.rs:131-139`) is limited to `Compare`-only predicates — it does NOT support `IN`, `REGEX`, `AND`, `OR`, or `NOT`. The comment at line 117-120 ("we only need the top-level expression comparisons here… For v0.1 scope this covers the F6 use cases") is explicit.

The RFC §3.5 example above uses `WHERE c2.name IN $context_portfolio` inside the `EXISTS` subquery. That is an `IN` predicate inside a subquery. Even if positive `EXISTS` were added, the inner `WHERE` parser cannot accept `IN`. The same limitation affects `NOT EXISTS` inner queries: if someone writes:
```cypher
NOT EXISTS { MATCH (c)-[:IMPLEMENTS_FOR]->(t:Trait)
  WHERE t.name IN $allowed_traits }
```
this would also fail with the current inner parser.

RFC §3.3 table row "WITHOUT TranslationAdapter impl" uses a simple `WHERE t.name = 'TranslationAdapter'` (Compare), which does parse. But the §3.5 full example is inconsistent with the parser's actual capabilities.

The author must:
- Either rewrite §3.5 to use only Compare predicates inside subqueries, or
- Acknowledge and scope the inner predicate extension (support `IN` inside subqueries), and if so, this is a minor parser extension (not a new AST variant, but a new combinator in `subquery_parser`).

---

## Cross-lens binding

### Reframe acceptance

The reframe ("no new crate, no new grammar") is correct and justified. The `cfdb-concepts` dep addition to `cfdb-query` is already pre-approved by `cfdb-concepts/Cargo.toml:21`. The compile-cost pledge is realistic (below).

### Compilation cost estimate

Current `cfdb-query` module footprint: 2,576 LOC across 16 source files (wc -l measured). The RFC adds approximately:
- 200 LOC for `param_resolver.rs` — realistic for 4 param forms + 4 error variants + hermeticity test
- 50 LOC for shared test fixtures
- The `cfdb-concepts` dep adds `serde + thiserror + toml` — all already in `cfdb-query`'s dep tree. Zero new proc-macro boundaries. Zero new heavy-compile deps.

The cfdb-cli additions (150 LOC for `check_predicate.rs` + dispatch wiring) are also realistic given the existing `check.rs` is 639 LOC for a more complex verb.

The LOC budget is plausible. No compile-time threshold risk.

### Feature-flag choice

The RFC's decision to NOT gate `check-predicate` behind a feature flag is sound. `cfdb-concepts` is a lightweight dep (serde + thiserror + toml, no proc-macros beyond derive). The feature-flag cost model (evaluated at every `cargo check`) is not justified for a dep this small. The existing `hir` and `git-enrich` feature flags in `cfdb-cli/Cargo.toml` exist because they gate `ra-ap-*` (90-150s cold compile) and `git2` (C library) respectively — no such weight exists here.

### Orphan rule analysis

No orphan risk. The param resolver constructs `cfdb_core::query::Param::List(Vec<PropValue>)` and `Param::Scalar(PropValue::Str(...))` — these are construction calls on upstream types using their own constructors, not `impl ForeignTrait for ForeignType`. `ParamResolveError` is a new type in `cfdb-query`, which owns it. No orphan-rule exposure.

### Object safety

No new traits. `cfdb-query` has no trait objects. N/A.

### Issue decomposition (§7)

Slices 1-5 are well-sequenced and right-sized. The prescribed `Tests:` blocks are correctly filled. One concern: Slice 1's "Self dogfood" test calls `resolve_params(workspace_root=cfdb_root, ...)` — this test is integration-shaped and will require cfdb's own `.cfdb/concepts/cfdb.toml` to be present at test time. Implementers must ensure the test path is an integration test (in `tests/`) not a unit test (in `src/`) so that the workspace root is available. This is not a blocker but should be noted in the issue body.

### Non-goals (§6)

All non-goals are appropriate. The explicit "Not a UDF framework" exclusion is correct given the existing `eval_call` dispatch table already serves as the extension point. The "Not a Shell-grep escape hatch" reframing is sound — `MATCH (f:File) WHERE f.path =~ $pat` is deterministic and Cypher-native; a shell-out would break determinism invariant §4.1.

One missing non-goal: "Not an extension to the inner-subquery WHERE grammar." The RFC should explicitly state that `NOT EXISTS { MATCH ... WHERE ... }` inner predicates remain Compare-only in this RFC. Without this, an implementer might assume they need full predicate support in subqueries, causing scope creep.

### Invariants (§4)

§4.1 Determinism — inherited correctly; `ORDER BY` + sorted param binding is sufficient.
§4.5 Param-resolver hermeticity — the `resolve_param(workspace_root, cli_arg)` pure-path signature is correct. Implementers must ensure `cfdb_concepts::load_concept_overrides` is the sole TOML reader, per §4.6.
§4.6 Canonical-bypass non-regression — well-specified. Delegating to `cfdb_concepts::load_concept_overrides` is the right mechanism; `cfdb-concepts/Cargo.toml:21` confirms this was anticipated.

---

## Overall

VERDICT: REQUEST CHANGES

The reframe is ratified. The Rust-systems fundamentals (no new proc-macro boundary, no orphan risk, sound compile-cost budget, correct dep direction, no feature-flag needed) are all correct. Two defects must be resolved before ratification:

1. **[REQUIRED] §3.5 canonical example uses `EXISTS { }` (positive), which does not exist in the parser or AST** (`parser/predicate.rs:43-48`, `ast.rs:147`). The RFC must either (a) rewrite §3.5 and the §3.3 table to avoid positive `EXISTS`, or (b) explicitly scope `Predicate::Exists` as a new AST variant and revise the "no new AST variant" pledge accordingly.

2. **[REQUIRED] §3.5 uses `IN` inside a subquery WHERE clause**, which the `subquery_parser` inner predicate grammar does not support (`predicate.rs:131-139`, Compare-only). The RFC must either (a) rewrite §3.5 to use only Compare predicates inside subqueries, or (b) scope the inner-predicate extension and add "Not an extension to inner-subquery WHERE grammar" to §6 Non-goals if not doing so now.

3. **[MINOR] Add non-goal**: "Not an extension to inner-subquery WHERE grammar" — makes the v0.1 scope explicit for implementers.
