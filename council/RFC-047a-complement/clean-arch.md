# Clean-arch verdict — RFC-047a

## Verdict

RATIFY — the correction, B1/B2/B3 design, and re-cut slices are
architecturally sound. Layering and dependency direction are unchanged.
Both rust-systems' REQUEST CHANGES amendments are architectural non-issues from
my lens (code anchor in path.rs, and explicit confirmation that
vsb-multi-resolver truncation is in 47-0 scope) — I endorse them.

**Note:** my initial pre-Phase-B draft said "Awaiting broadcast before
finalising Q1." After reading rust-systems' broadcast, I confirm RATIFY
with the two amendments rust-systems requires absorbed into my test-prescription
notes below.

---

## Per-blocker analysis

### Correction of record (§1)

**VERIFIED.** The "no list-binding path exists" finding in RFC-047 §3.2/§5
was false. The clean-arch verdict in RFC-047 §5 cited `commands/query.rs:39`
(`--input` stub) and `query.rs:104` (`bind_single_param` rejects arrays) —
but these are the raw `--params`/`--input` CLI surface, which RFC-047 §3.2
itself explicitly scoped out ("not by routing through the CLI
`--params`/`--input` surface"). The actual in-process path was shipping at
council deliberation time:

- `Param::List(Vec<PropValue>)` exists: `crates/cfdb-core/src/query/ast.rs:54-57`
- `eval_expr_list` resolves it: `crates/cfdb-petgraph/src/eval/predicate.rs:115-117`
- `Predicate::In` consumes it: `predicate.rs:26-33`
- `param_resolver.rs:90-95` binds `list:<a,b,c>` → `Param::List`
- `raid_plan_queries.rs:127-129,255-282` exercises `Param::List` into `IN`
  predicates across five raid templates in production fixture tests

The "blocking gap" in RFC-047 clean-arch was a misread of scope. This
complement correctly retracts it.

### B1 — open-range `*N..` parse gap

**VERIFIED.** `crates/cfdb-query/src/parser/match_clause.rs:82-86`: `range`
requires `digits()` on both sides of `..`. The upper bound is not
`.or_not()`. The `EdgePattern.var_length: Option<(u32, u32)>` AST field
(`crates/cfdb-core/src/query/ast.rs:106-108`) accommodates `u32::MAX` as
the sentinel for open upper bound with zero AST change — the YAGNI claim
is verified.

**Layering verdict (CLEAN):** The fix belongs in `cfdb-query` (parser),
confirmed innermost layer below `cfdb-cli`. Cargo.toml evidence:
- `cfdb-query/Cargo.toml`: depends on `cfdb-core` only — no `cfdb-petgraph`,
  no `cfdb-cli`
- `cfdb-petgraph/Cargo.toml`: depends on `cfdb-core` + `cfdb-query` — no
  `cfdb-cli`
- `cfdb-cli/Cargo.toml`: depends on all three

No dependency rule violation; no inner layer imports an outer layer.

**Schema classification (CONFIRMED, load-bearing):** This is a Cypher-subset
grammar change — a query-language construct, not a keyspace schema change.
Nothing persisted changes; `SchemaVersion`, `CALLS` edges, node/edge labels
are untouched. No `graph-specs-rust` lockstep is needed. I confirm this
classification.

**Sentinel footgun (from rust-systems, endorsed):** The `u32::MAX` sentinel
for open-upper is a convention, not an enforced grammar invariant — nothing
prevents a literal `*1..4294967295` in a query. RFC-047a §3.2 should
document `u32::MAX` as a reserved sentinel. This is a doc gap, not a
design flaw; architecturally fine.

### B2 — explicit-bound clamp

**VERIFIED.** `crates/cfdb-petgraph/src/eval/pattern/path.rs:205-208`:
```rust
let max_depth = max_depth
    .max(min_depth)
    .min(DEFAULT_VAR_LENGTH_MAX.max(min_depth));
```
This clamps ALL var-length traversals — including explicit `*1..10` — to 5.
The constant's doc at `eval/mod.rs:62`: "Maximum BFS depth when a
variable-length pattern **omits** its upper bound." The code contradicts
the doc for explicit bounds.

**LIVE PRODUCTION BUG CONFIRMED (from rust-systems, verified independently):**
- `.cfdb/queries/vsb-multi-resolver.cypher:67`: `MATCH (h)-[:CALLS*1..10]->(f:Item {kind: 'fn'})` — truncated to 5
- `examples/queries/vertical-split-brain.cypher:119-120`: `[:CALLS*1..8]` — truncated to 5
- `examples/queries/vertical-split-brain-drop.cypher:135-136`: `[:CALLS*1..8]` — truncated to 5

The vsb-multi-resolver query is CI-enforced (`violations` gate). With the
truncation, call chains deeper than 5 hops are invisible to the split-brain
detector. This is a live correctness bug, not a theoretical concern.

**Layering verdict (CLEAN):** The fix belongs in `cfdb-petgraph` (evaluator).
No `cfdb-core` or `cfdb-query` interface change needed. The fix is confined
to `traverse_bfs` in `path.rs:200-237`. Does not require any change to the
`StoreBackend` port trait in `cfdb-core`.

### B3 — HIR-dogfood

**VERIFIED.** `crates/cfdb-extractor/src/lib.rs:18-21`: "Out of scope for
v0.1: resolved cross-crate `CALLS` (Item → Item)." The syn-based extractor
emits `INVOKES_AT` + stub nodes (`synthesize_referenced_items`, `lib.rs:254`)
only.

**Layering verdict (CLEAN).** Running `cfdb extract --hir` in a test to
obtain a CALLS-populated keyspace is adapter-level composition-root behaviour.
The test lives in `cfdb-cli/tests/` (the outermost layer, correct). No query-
layer or evaluator logic is added. The HIR quarantine (`cfdb-hir-extractor`
owns the trait, `cfdb-hir-petgraph-adapter` implements it, `cfdb-petgraph`
has zero `ra-ap-*` dependency) is preserved — confirmed by Cargo.toml.

---

## Contested-question positions

### Q1 — open-form `*N..` semantics: cap at 5 or visited-set-unbounded?

**Phase B converged position (after rust-systems broadcast):** VISITED-SET
UNBOUNDED, with a mandatory code anchor in `path.rs`.

The asymptotic argument is correct: `traverse_bfs` at `path.rs:211,230` uses
a `BTreeSet<NodeIndex>` visited-set; every node enters the queue at most once
(the `visited.insert(target)` guard at :230). Therefore the walk terminates
after at most `|V|` frontier pops, total work O(V+E) regardless of `max_depth`.
A depth cap on open forms buys no algorithmic safety — it silently truncates
blast radius instead of reporting it, which is the worse failure mode for an
`impact` query.

RFC-047 §3.2 promised "unbounded by default." There are zero open-form queries
in the tree today (parser rejected them); the blast radius of this policy is
bounded to new queries post-B1. Both safety-cap and unbounded are
architecturally clean (the policy lives entirely in `eval/pattern/path.rs`,
inside `cfdb-petgraph`, the correct layer, with no interface change either
way). Clean-arch has no blocking objection to the cap, but endorses unbounded
as the correct semantic per RFC-047's own stated intent.

**rust-systems' code-anchor requirement (endorsed):** The chosen policy MUST
appear as a comment or const at the call site in `path.rs` — not buried in an
RFC — so implementers and future readers know what `M == u32::MAX` means
without a document search. This is a doc-correctness requirement, not an
architecture question; I support it.

### Q2 — explicit-bound clamp: standalone `fix:` or part of 47-0?

**Phase B converged position (with rust-systems):** B2 explicit-bound fix
MUST land in 47-0, not deferred. The vsb-multi-resolver.cypher:67 truncation
is a **live production CI bug** — the ban rule silently misses call chains
of depth 6-10 today. Keeping this truncation through 47-A development means
the violations gate produces false negatives during all of 47-0 and 47-A work.

Clean-arch position: B1+B2 belong together in 47-0 for cohesion (one
reason to change: how var-length bounds are interpreted). The RFC-047a §7
re-cut already places B1+B2 in 47-0. What was missing was explicit
acknowledgment that the explicit-bound clamp fix is unconditional (no Q1
dependency) — the vsb-multi-resolver truncation should be repaired as part
of B2 regardless of how Q1 resolves.

**Engaged:** rust-systems. Converged on: B1+B2 both in 47-0; explicit-bound
fix is unconditional; vsb-multi-resolver regression test belongs in 47-0
unit row.

### Q4 — amend RFC-047 in place vs. let complement supersede?

**My position:** complement-supersedes is the correct record-keeping choice.
RFC-047 is ratified and merged; amending it in-place would silently rewrite
what the council actually decided (including the false finding) and erase the
evidence trail of the *council foundation claims need verification* failure
mode this complement exemplifies.

Concrete recommendation: RFC-047 §3.2 and §5 should each carry a single
added line: `> Superseded by RFC-047a §1 (list-binding) and RFC-047a §3 (query
mechanics).` This is an annotation, not a rewrite. The complement is the
living document. This matches rust-systems' position.

**Engaged:** all. Converged on complement-supersedes + pointer annotation
in RFC-047.

---

## Test-surface prescription notes

### 47-0

The existing `Tests:` block is correct as-is. Three additions:

1. **Unit row addition (B2 regression):** explicitly assert that `*1..10`
   traverses `10` hops and not 5; and that `*1..8` traverses `8` hops and
   not 5. This pins the vsb-multi-resolver truncation regression. Name these
   tests against the affected queries so the test name is self-documenting.
2. **Unit row addition (B2 `u32::MAX` sentinel):** assert that `*1..` (open
   form, post-B1) does NOT clamp to 5 under the chosen Q1 policy, and that
   `traverse_bfs` reaches all nodes reachable by the visited-set on a small
   fixture graph.
3. **Sentinel doc requirement:** the `path.rs` call site where `M == u32::MAX`
   triggers open-form semantics must carry an inline comment naming the policy.

The `Self dogfood: none` for 47-0 is correct — the CALLS-graph dogfood needs
HIR; 47-0 is query-mechanics only.

### 47-A

The `Tests:` block is correct. One addition, aligned with rust-systems:

**Self dogfood row** must be feature-gated: the test calling `cfdb extract --hir`
MUST be `#[cfg_attr(not(feature = "integration-live"), ignore)]` or
`CFDB_INTEGRATION` env-gated. Without this gate the test either (a) passes
trivially on a syn-only (zero-CALLS) keyspace, or (b) times out CI. The 47-A
tests row should read:

```
  - Self dogfood (cfdb on cfdb): `cfdb extract --hir` cfdb-self to a temp
    keyspace (resolved CALLS), seed a known leaf fn in cfdb-core, assert its
    known callers in cfdb-petgraph/cfdb-cli appear in the affected set. Gate
    behind `#[cfg_attr(not(feature = "integration-live"), ignore)]` to stay
    under the 5-minute CI timeout budget.
```

No metric ratchets detected anywhere in the complement or re-cut slices.
