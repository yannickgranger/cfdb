# RFC-055 — Correlated `NOT EXISTS`: outer-scope bindings in subqueries (query-subset v0.2)

- Status: **RATIFIED 4/4** (2026-08-01, R2)
- Refs: #546 (W2.3 in ledger #547) · upstream refile of graph-specs-rust#94 · cfdb-034-query-dsl#6 non-goals · `docs/query-dsl.md` §"Grammar constraints"
- Downstream demand: agentry#897 / #900 / #909 (zero-dead invariant, audit queue), graph-specs-rust per-context dead-rules, cfdb's own pattern-B rule tightening

## 1. Problem

The Cypher subset parses `NOT EXISTS { MATCH ... }` (`Predicate::NotExists`, since the initial portage) but evaluates the inner query in a **fresh evaluator scope**:

```rust
// crates/cfdb-petgraph/src/eval/predicate.rs:44-47
Predicate::NotExists { inner } => {
    let sub = Evaluator::new(self.state, self.params).run(inner);
    sub.rows.is_empty()
}
```

Two consequences, both documented across the tree:

1. **The anti-join rule class is unwritable.** "Items with zero callers", "params never wire-registered", "enums never matched on" all need `NOT EXISTS { MATCH ()-[:E]->(outer_var) }`. Today that shape **parses and silently means the wrong thing**: the inner variable is fresh, so the subquery tests *global* emptiness of the pattern, not correlation with the current row. An inner `WHERE outer.prop = x` is worse still — the unbound outer ref evaluates to `None` and the comparison is silently `false` (`.cfdb/predicates/README.md:26`).
2. **Six surfaces carry documented workarounds for exactly this gap**: `crates/cfdb-cli/src/check.rs:15-24` (the canonical statement — T1/T3 triggers are Rust code instead of Cypher rules partly because "outer-bound vars [are] inaccessible in NOT EXISTS"), `examples/queries/vertical-split-brain-drop.cypher:64-80` (writes out the wanted correlated form as a comment, ships a weaker rule + operator triage), `matchsite-external-type-fence.cypher:31-33`, `arch-ban-rfc-053-syn-visibility-split-resolution.cypher:34-39`, `t1-concept-unwired.cypher:16-18`, `raid/raid-missing-canonical.cypher:12-14`.

The codebase already treats correlation as a planned upgrade: `pattern_b_vertical_split_brain_drop.rs::both_keys_wire_registered_currently_fires_as_known_false_positive` pins a live false-positive class (expected rows = 2) and is written to **flip red when this RFC lands**, forcing the rule tightening.

cfdb-034-query-dsl#6 explicitly deferred subquery-grammar widening to "a separate parser-extension RFC"; `docs/query-dsl.md:165` lists the fresh-scope constraint. This is that RFC — scoped to *semantics only* (the grammar already admits the shape).

## 2. Scope

Ships:

- **Correlated evaluation of `NOT EXISTS` subqueries.** Outer-scope bindings are visible inside the inner MATCH (pattern positions) and inner WHERE (expression refs).
- **Correlation visibility**: when seeding correlates at least one inner variable, the *outer* evaluator records a notice on its existing warnings channel naming the correlated variables. Observability only — never affects rows or exit codes.
- **Documentation truth pass — two sweeps, two scopes**. The *compat sweep* (§3.3) covers executable rule bodies only. The *prose-truth sweep* below covers every surface that states the now-false constraint; the implementing issue enumerates each file's exact clause (clause-level edits, not blanket rewrites):

  | File | Clause to correct |
  |---|---|
  | `docs/query-dsl.md:164-165` | fresh-scope constraint (165) AND the stale "Compare-only inner WHERE" claim (164 — see §6, already false since `e1a58e9`) |
  | `.cfdb/predicates/README.md:25-26` | same two clauses |
  | `examples/queries/vertical-split-brain-drop.cypher:64-80` | superseded wholesale — rule adopts the correlated form (§2 tightening) |
  | `matchsite-external-type-fence.cypher:31-33`, `arch-ban-rfc-053-…cypher:34-39` | correlation clause now false; the "principled complement" becomes *possible* but is NOT adopted in this slice — say so |
  | `t1-concept-unwired.cypher:16-18` | correlation clause now false; T1 stays Rust-side (its other two limitations remain, per `check.rs`) |
  | `raid/raid-missing-canonical.cypher:12-14` | correlation clause now false |
  | `raid/raid-completeness.cypher:21-26` | stale Compare-only claim corrected — a PRE-EXISTING doc bug (`e1a58e9` closed the gap 2026-04-25; the file predates the fix by two days and was never revisited). Per fix-or-file this lands in the 55-A PR but as its OWN clearly-labeled boy-scout commit, not attributed to this RFC's motivation. Disposition of the rule itself: the correlated `IN $rewrite` lift becomes fully expressible once 55-A ships (grammar already there + correlation from this RFC), but ADOPTING it is explicitly deferred — examples-corpus rule, no pinning test, no live consumer; the corrected header states the lift is now expressible and unadopted |
  | `.cfdb/queries/self-enrich-metrics.cypher:38-40` | "cannot reference outer bindings" clause now false; rule design unchanged |
  | `crates/cfdb-cli/src/check.rs:15-24` | module doc must state cfdb-055-correlated-not-exists resolves exactly ONE of its "three anti-join limitations" (correlation) — the other two (OPTIONAL-MATCH null-fill, collect/IN) remain, and T1/T3 stay Rust-side. NOTE: `check/t1.rs`/`check/t3.rs` inline comments cite the OPTIONAL-MATCH limitation, not correlation — they are correctly OUT of this pass |
  | `docs/split-resolution-fences.md:74-81` | correlation clause now false (doc-level companion of matchsite-external-type-fence) |

- **Pattern-B rule tightening**: `examples/queries/vertical-split-brain-drop.cypher` adopts the correlated form its own header wishes for; the pinned test flips expected 2 → 0.

Does NOT ship: any parser change, any AST change, any schema/wire change (see §6). Note `vsb-multi-resolver.cypher:26-32` needs **no** edit: its blocker is the absence of positive `EXISTS` (a §6 non-goal), not correlation — its header stays accurate.

## 3. Design

### 3.1 Semantics (matches standard Cypher correlated subqueries)

Evaluating `Predicate::NotExists` for an outer row with bindings `B`:

1. The inner query is evaluated with **initial binding stream = one row containing `B`** (today: one empty row, `eval/mod.rs:196`). **`B` is the current row wherever the predicate sits**: after a `WITH`, the row contains only that WITH's re-projected aliases (`apply_with` builds a fresh row from projections, `with_clause.rs:15-50`) — a NOT EXISTS positioned post-WITH correlates only against re-projected names; a pre-WITH-only name is unbound there (standard Cypher WITH rescoping, pinned by test, §7).
2. An inner pattern variable that shares a name with a variable in `B` is **correlated**: it denotes the already-bound node/edge. Inner label, property, AND binding-kind constraints on it apply conjunctively — a bound `(i:Item)` used inner as `(i:Crate)` fails the row (`candidate_nodes` label-filters before `emit_bound_node`'s `matches_existing` re-check, `pattern.rs:105-123`, `coupling.rs:255-257`), and a name bound to a non-node (edge/value) used in a node position likewise fails the row, symmetric across FROM/TO endpoints (requires the §3.2 `resolve_endpoint` fix — today's FROM arm silently treats it as fresh).
3. Inner variables not present in `B` remain **existential** (fresh), as today.
4. Inner WHERE expressions resolve refs through the seeded bindings — `WHERE other.name = wire.name` with outer `wire` now compares real values instead of silently evaluating `None`. This is a mechanical consequence of the same seed (`eval_expr` already resolves via `bindings.get`), not a second change — and it applies **uniformly across the full shipped inner-WHERE grammar** (In/And/Or/Not/Regex/nested NotExists; the Compare-only restriction died at `e1a58e9`, see §6), pinned by a compound-predicate test (§7).
5. The predicate is true iff the seeded inner evaluation yields zero rows. Per-outer-row evaluation is unchanged (it is already per-row today; it merely ignored the row).
6. `$params` were already shared with the subquery; unchanged.
7. **Correlation visibility**: when `seed_keys ∩ inner_pattern_variables ≠ ∅`, the OUTER evaluator (not the discarded sub-evaluator) records one notice per distinct (subquery, correlated-variable-set) per query run — NOT per outer row — on the existing warnings channel (`Evaluator.warnings` RefCell), surfaced wherever warnings already surface (incl. `--explain`). This requires one **additive, non-breaking `WarningKind` variant in cfdb-core** (no existing variant fits; runtime-only — warnings are not persisted except the 54-A contention class, so no `SchemaVersion` implication) — disclosed in §4. Rationale for the modified form: an always-on *warning* would fire on 100% of intentional correlated queries — shadowing IS the correlation syntax — so this ships as observability, not an alarm. It still closes the gap: an author who accidentally reused a short name has, for the first time, a channel that shows the correlation happened. Never affects row counts or exit codes.

### 3.2 Mechanics — seed the existing join, add nothing

The evaluator's MATCH pipeline is a streaming join over `BindingStream` rows (`eval/mod.rs:99-104`), and **cross-clause correlation already exists**: `emit_node_bindings` (`pattern.rs:71-85`) dispatches on `bindings.contains_key(var)`; the bound branch re-checks via `matches_existing` + `node_props_match`. Multi-MATCH queries exercise this today (`vsb-multi-resolver.cypher`'s four-clause implicit-AND join; `eval/cross_match_tests.rs`). Seeding a nested `BindingStream` is itself an existing live pattern — `apply_optional_row` (`pattern.rs:240`) does it for OPTIONAL MATCH. Correlation for subqueries is therefore *only* a seeding change:

- New `run_seeded(query, seed: Bindings)` on `Evaluator` — **eval-module-private** (the only consumer is `eval::predicate`, a descendant module, so private is sufficient and doesn't expose a raw arbitrary-seed entry point crate-wide; `run` stays `pub(crate)` because the composition root genuinely calls it) — starts the stream at the seed row; `run()` delegates with an empty seed (which IS the prescribed empty-seed regression test).
- `Predicate::NotExists` eval passes the current row's `bindings` clone as seed. Clone cost is the same order as the `Bindings` clones already pervasive in this module (`emit_new_var_node`, `unwind_row`, `apply_optional_row`).
- No AST change (`Predicate::NotExists` already carries a full `Query`, `ast.rs:149-151`), no parser change, no new types. Borrow-trivial: the `NotExists` arm already constructs a fresh per-call `Evaluator` today.
- Implementation note: the §3.1.7 notice needs the inner pattern's variable set — `collect_pattern_vars` (`coupling.rs:266`) computes exactly this but is `pub(super)` inside `eval::pattern`; widen to `pub(in crate::eval)` (precedented at `path.rs:19`) rather than duplicating.

**Performance — stated plainly:** endpoint anchoring is asymmetric. A bound FROM endpoint anchors O(1) (`path.rs:128-140`); a bound TO endpoint is NEVER anchored — it is discovered by forward traversal from the (possibly full-scanned) FROM candidates and filtered post-hoc (`build_path_binding`, `path.rs:110-117`). The canonical anti-join shape — anonymous FROM, outer-bound TO — therefore costs O(outer_rows × (V+E)): the **same pre-existing per-outer-row full-scan the uncorrelated path always had, not a new cost — but this RFC is what first makes that dormant path worth exercising at production scale** (same failure class as the #409 hang). `apply_path_pattern` also has no candidate cache (the #409 fix covered node patterns only), and the per-row fresh sub-`Evaluator` (incl. fresh `regex_cache`) prevents cross-row amortization. Consequence: §7's target dogfood is an *asserted bound*, not an eyeball report, and TO-endpoint anchoring is the named follow-up if scale demands it.

**In-scope symmetry fix:** a FROM-endpoint name bound to a non-node (`Binding::EdgeRef`/`Value`) silently falls through to existential (`resolve_endpoint`, `path.rs:134-138`) instead of rejecting the kind mismatch as the TO path does (`build_path_binding`, `path.rs:110-117`). This pre-existing bug (reachable today via multi-clause MATCH reusing an edge/scalar-bound name) falsifies §3.1.2 as stated, and this RFC increases traffic through exactly that path. 55-A fixes it — the FROM arm returns the bound `NodeIndex` for `NodeRef` and zero candidates for any other binding kind, mirroring the TO arm — pinned by a 2-case test (NodeRef still correlates; EdgeRef/Value yields zero rows, not fresh hits).

### 3.3 Compatibility — the shipped-rule sweep (evidence, not hope)

The semantics change is observable only when an inner variable shadows an outer name. Sweep of every shipped `NOT EXISTS` (2026-08-01, `rg -n "NOT EXISTS" .cfdb/queries/ .cfdb/predicates/ examples/`):

| Surface | Uses | Shadowing? | Effect |
|---|---|---|---|
| `.cfdb/queries/self-enrich-rfc-docs.cypher:80`, `self-enrich-concepts.cypher:75-76` | the ONLY two live executable clauses; label-anchored `MATCH ()-[:E]->(:Label)`, zero named variables | none | **no-op** |
| `.cfdb/predicates/*.cypher` | none use `NOT EXISTS` | — | no-op |
| `examples/queries/vertical-split-brain-drop.cypher` | today's executable rule is the weaker form (its `NOT EXISTS` exists only in comments) | intentional flip | **the desired tightening** (pinned test) |
| companion (graph-specs-rust, pinned SHA) | verified mechanically by `ci/cross-dogfood.sh` in the implementing PR | — | must stay 0 findings |

No shipped rule silently changes meaning. Downstream rule authors get the same sweep protocol in the changelog entry (grep shape above), plus the §3.1.7 correlation notice as the ongoing authoring fence.

### 3.4 Relationship to #564

Anti-join rules commonly pair with `count()` sentinels; #564 (`count()` over empty MATCH yields zero rows instead of one `0` row) is an adjacent **bug**, fixed separately without RFC gate. Neither blocks the other; both land before the next release cut so rule authors get the pair together.

## 4. Invariants

- **No wire-format change. No `SchemaVersion` bump. No graph-specs lockstep.** Evaluator (`cfdb-petgraph`) **plus one additive, non-breaking `WarningKind` variant in `cfdb-core`** for the §3.1.7 notice (no existing variant fits, `result.rs:76-95`; non-breaking is compiler-enforced — the enum is `#[non_exhaustive]`, `result.rs:75`; runtime-only — the only persisted warning class is 54-A's contention set, so the new variant never reaches disk and the wire-format claim stands literally). Zero Cargo.toml edits — the parser↔evaluator separation is mechanically enforced (`tests/architecture_dep_rule.rs` + `.cfdb/workspace-dep-rules.toml`; cfdb-query is dev-only in cfdb-petgraph).
- **Determinism**: `cfdb extract` untouched; `ci/determinism-check.sh` byte-stable trivially. Query evaluation stays deterministic (BTreeMap bindings, ordered streams).
- **Recall**: N/A — no extractor change; `cfdb-recall` corpus untouched.
- **Shipped-rule stability**: every `.cfdb/queries/*.cypher` row count unchanged on cfdb-self except the documented pattern-B example tightening. Cross-dogfood 0 findings at the pinned companion SHA.
- **No-ratchet**: no baseline/allowlist files; the pattern-B expected-count change is a reviewed test edit in the same PR.

## 5. Architect lenses (R1 verdicts; R2 = confirmation of the folds)

### 5.1 Clean architecture (`clean-arch`) — R1: REQUEST CHANGES → R2 finding folded → final: RATIFY

### 5.2 Domain-driven design (`ddd-specialist`) — R1: REQUEST CHANGES → folds applied; R2: RATIFY (modified §3.1.7 confirmed)

### 5.3 SOLID / component principles (`solid-architect`) — R1: RATIFY, revised to REQUEST CHANGES → folds applied

### 5.4 Rust systems (`rust-systems`) — R1: REQUEST CHANGES → final six-item list, all folded

## 6. Non-goals

- **Positive `EXISTS { }`** — still parser-absent; `vsb-multi-resolver.cypher:26-32`'s workaround (fourth joined MATCH) is *this* gap's evidence, and stays as-is (cfdb-034-query-dsl#6 stands).
- **Inner-WHERE grammar** — **no change needed, and no longer claimed as deferred work**: contrary to cfdb-034-query-dsl#6's text and the stale docs (`query-dsl.md:164`, `predicates/README.md:25`), the live `subquery_parser` already threads the full recursive predicate grammar into inner WHERE (restriction removed at `e1a58e9`, the repo's second commit; verified at `parser/predicate.rs:102-144` by two lenses). The §2 truth pass corrects the stale Compare-only claims. Whether specific lifts (e.g. `raid-completeness`'s `IN $rewrite`) work end-to-end is NOT verified by this RFC and is not claimed.
- **`:CALLED_BY` reverse edge** (#546 option 2) — REJECTED: duplicates a derivable traversal (`CALLS` walked inward), costs a `SchemaVersion` bump + lockstep + keyspace growth, zero expressiveness gain over the correlated anti-join. Recorded as the disposition of #546's alternative. Confirmed against `labels.rs:156-157`.
- **Correlated `OPTIONAL MATCH` + null-fill** (floated in `arch-ban-rfc-053-…cypher:37-39`) and the other two `check.rs:15-24` limitations — not pursued; T1/T3 stay Rust-side.
- **Path-pattern TO-endpoint anchoring / path candidate cache** — the perf fix for §3.2's asymmetry is a tracked follow-up (filed from §7's escape hatch if the bound-assertion route proves infeasible), not part of this slice.
- **Sub-evaluator warning propagation** — the §3.1.7 notice is recorded by the OUTER evaluator; propagating the discarded inner evaluator's own warnings stays out of scope.
- **UDFs, template composition** — cfdb-034-query-dsl#6 unchanged.

## 7. Issue decomposition

One vertical slice (grammar exists; capability + visibility + docs + rule tightening share one reason-to-change):

**55-A — correlated `NOT EXISTS` end-to-end** (re-scopes #546):
seeded sub-evaluation (`run_seeded`, eval-module-private), correlation notice (§3.1.7), `resolve_endpoint` FROM-endpoint symmetry fix (§3.2 — its own commit within the PR: a precondition fix independently reachable via ordinary multi-clause MATCH, kept legible apart from the "ship correlation" commit), docs truth pass (§2 table, clause-level; raid-completeness staleness as its own boy-scout commit), pattern-B example rule tightened + pinned test flipped 2 → 0.

```
Tests:
  - Unit: eval predicate — correlated anti-join true/false per outer row; label re-check on a
    correlated variable (bound (i:Item) reused as (i:Crate) → row fails); inner-fresh variable
    stays existential; inner WHERE outer-ref resolves (the README:26 None-footgun case, asserted
    both directions); compound-predicate correlation (outer ref inside an AND/IN-composed inner
    WHERE, not just bare Compare — pins §3.1.4's full-grammar uniformity); post-WITH rescoping
    (NOT EXISTS after WITH correlates only re-projected aliases; a pre-WITH-only name is unbound,
    not an error — pins §3.1.1); empty-seed regression (run() ≡ run_seeded(empty)); correlation
    notice fires on a shadowed-name fixture and stays silent on a non-shadowed one, and never
    alters rows/exit codes, and fires O(1) per NOT EXISTS occurrence — not O(outer_rows) — on a
    multi-row fixture; resolve_endpoint symmetry fix 2-case pin (NodeRef-bound FROM still
    correlates; EdgeRef/Value-bound FROM yields zero rows, not fresh hits).
  - Self dogfood (cfdb on cfdb): tightened vertical-split-brain-drop rule on the vsb fixture —
    both_keys_wire_registered flips to 0 rows (test edit in same PR); full .cfdb/queries/*.cypher
    battery on cfdb-self keyspace: row counts byte-identical to develop (the §3.3 no-op sweep,
    executed not assumed); prose-truth check: none of check.rs / split-resolution-fences.md /
    query-dsl.md / predicates README / the §2 cypher headers still state the unconditional
    "outer vars are inaccessible" or "Compare-only inner WHERE" claims post-merge.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): ci/cross-dogfood.sh — 0 findings
    (exit 30 on any rule row blocks merge).
  - Target dogfood (on qbot-core at pinned SHA): zero-callers anti-join
    (MATCH (i:Item) WHERE ... AND NOT EXISTS { MATCH ()-[:CALLS]->(i) } ...) on the qbot-core
    keyspace, as an ASSERTED wall-time bound patterned on cross_match_indexed_completes_under_100ms
    (ceiling chosen by the implementer from measured evidence, recorded in the test). Escape hatch
    (only with measured evidence of infeasibility at full scale): bound the corpus to a stated
    sub-scope (e.g. one crate_tier) AND file the TO-endpoint-anchoring follow-up issue (§3.2,
    #409 class) in the same PR — never ship the unbounded eyeball-report form.
```
