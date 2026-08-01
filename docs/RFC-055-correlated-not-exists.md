# RFC-055 — Correlated `NOT EXISTS`: outer-scope bindings in subqueries (query-subset v0.2)

- Status: DRAFT — pending 4-lens architect council (§2.3 of `CLAUDE.md`)
- Refs: #546 (W2.3 in ledger #547) · upstream refile of graph-specs-rust#94 · RFC-034 §6 non-goals · `docs/query-dsl.md` §"Grammar constraints"
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

Two consequences, both documented across the rule library:

1. **The anti-join rule class is unwritable.** "Items with zero callers", "params never wire-registered", "enums never matched on" all need `NOT EXISTS { MATCH ()-[:E]->(outer_var) }`. Today that shape **parses and silently means the wrong thing**: the inner variable is fresh, so the subquery tests *global* emptiness of the pattern, not correlation with the current row. An inner `WHERE outer.prop = x` is worse still — the unbound outer ref evaluates to `None` and the comparison is silently `false` (`.cfdb/predicates/README.md:26`).
2. **Eight shipped rule/example headers carry workarounds** for exactly this gap: `vertical-split-brain-drop.cypher:64-73` (writes out the wanted correlated form as a comment, ships a weaker rule + operator triage), `vsb-multi-resolver.cypher:28-30` (expands the would-be subquery into a fourth joined MATCH), `matchsite-external-type-fence.cypher:31-33`, `arch-ban-rfc-053-syn-visibility-split-resolution.cypher:34-39`, `t1-concept-unwired.cypher:16-18`, `raid-missing-canonical.cypher:12-14`, `raid-completeness.cypher:21-23`, `self-enrich-metrics.cypher:38-40`.

The codebase already treats correlation as a planned upgrade: `pattern_b_vertical_split_brain_drop.rs::both_keys_wire_registered_currently_fires_as_known_false_positive` pins a live false-positive class (expected rows = 2) and is written to **flip red when this RFC lands**, forcing the rule tightening.

RFC-034 §6 explicitly deferred subquery-grammar widening to "a separate parser-extension RFC"; `docs/query-dsl.md:165` lists the fresh-scope constraint. This is that RFC — scoped to *semantics only* (the grammar already admits the shape).

## 2. Scope

Ships:

- **Correlated evaluation of `NOT EXISTS` subqueries.** Outer-scope bindings are visible inside the inner MATCH (pattern positions) and inner WHERE (expression refs).
- **Documentation truth pass**: `docs/query-dsl.md` constraint list, `.cfdb/predicates/README.md:26`, and the eight workaround headers above are updated to the new semantics (only the statements that become false).
- **Pattern-B rule tightening**: `examples/queries/vertical-split-brain-drop.cypher` adopts the correlated form its own header wishes for; the pinned test flips expected 2 → 0.

Does NOT ship: any parser change, any AST change, any schema/wire change (see §6).

## 3. Design

### 3.1 Semantics (matches standard Cypher correlated subqueries)

Evaluating `Predicate::NotExists` for an outer row with bindings `B`:

1. The inner query is evaluated with **initial binding stream = one row containing `B`** (today: one empty row).
2. An inner pattern variable that shares a name with a variable in `B` is **correlated**: it denotes the already-bound node/edge. Inner label and property constraints on it apply conjunctively (a bound `(i:Item)` used inner as `(i:Crate)` fails the row — same re-check the multi-MATCH join already performs).
3. Inner variables not present in `B` remain **existential** (fresh), as today.
4. Inner WHERE expressions resolve refs through the seeded bindings — `WHERE other.name = wire.name` with outer `wire` now compares real values instead of silently evaluating `None`.
5. The predicate is true iff the seeded inner evaluation yields zero rows. Per-outer-row evaluation is unchanged (it is already per-row today; it merely ignored the row).
6. `$params` were already shared with the subquery; unchanged.

### 3.2 Mechanics — seed the existing join, add nothing

The evaluator's MATCH pipeline is a streaming join over `BindingStream` rows (`eval/mod.rs:99-104`), and **cross-clause correlation already exists**: multi-MATCH queries join later clauses against earlier bindings (`vsb-multi-resolver.cypher` relies on a four-clause implicit-AND join). Correlation for subqueries is therefore *only* a seeding change:

- New `pub(super)` entry on `Evaluator` (e.g. `run_seeded(query, seed: Bindings)`) that starts the stream at the seed row instead of the empty row; `run()` delegates with an empty seed.
- `Predicate::NotExists` eval passes the current row's `bindings` clone as seed.
- No AST change, no parser change, no new types. The existing `apply_*` join machinery provides label re-check, edge correlation, and inner-fresh existential semantics for free.

Perf note for council: seeding also *anchors* the inner scan when candidate enumeration exploits bound endpoints; where it does not, correctness still holds via join-filtering and the cost is bounded by today's uncorrelated inner scan. Sub-evaluator warnings are dropped today (`sub.rows` only is read); this RFC does not change that — noted as a pre-existing observability gap, not in scope.

### 3.3 Compatibility — the shipped-rule sweep (evidence, not hope)

The semantics change is observable only when an inner variable shadows an outer name. Sweep of every shipped `NOT EXISTS` (2026-08-01, `rg -n "NOT EXISTS" .cfdb/queries/ .cfdb/predicates/ examples/`):

| Surface | Uses | Shadowing? | Effect |
|---|---|---|---|
| `.cfdb/queries/self-enrich-rfc-docs.cypher:80`, `self-enrich-concepts.cypher:75-76` | label-anchored `MATCH ()-[:E]->(:Label)`, zero named variables | none | **no-op** |
| `.cfdb/predicates/*.cypher` | none use `NOT EXISTS` (README documents the constraint) | — | no-op |
| `examples/queries/vertical-split-brain-drop.cypher` | names `(ep)`/`other` in comment-documented weaker form | intentional flip | **the desired tightening** (pinned test) |
| companion (graph-specs-rust, pinned SHA) | verified mechanically by `ci/cross-dogfood.sh` in the implementing PR | — | must stay 0 findings |

No shipped rule silently changes meaning. Downstream rule authors get the same sweep protocol in the changelog entry (grep shape above).

### 3.4 Relationship to #564

Anti-join rules commonly pair with `count()` sentinels; #564 (`count()` over empty MATCH yields zero rows instead of one `0` row) is an adjacent **bug**, fixed separately without RFC gate. Neither blocks the other; both land before the next release cut so rule authors get the pair together.

## 4. Invariants

- **No wire-format change. No `SchemaVersion` bump. No graph-specs lockstep.** Evaluator-only.
- **Determinism**: `cfdb extract` untouched; `ci/determinism-check.sh` byte-stable trivially. Query evaluation stays deterministic (BTreeMap bindings, ordered streams).
- **Recall**: N/A — no extractor change; `cfdb-recall` corpus untouched.
- **Shipped-rule stability**: every `.cfdb/queries/*.cypher` row count unchanged on cfdb-self except the documented pattern-B example tightening. Cross-dogfood 0 findings at the pinned companion SHA.
- **No-ratchet**: no baseline/allowlist files; the pattern-B expected-count change is a reviewed `const`-style test edit in the same PR.

## 5. Architect lenses

Verdicts recorded inline by the council (§2.3). Each lens verifies the file:line claims in §1–§3 against the tree before ruling.

### 5.1 Clean architecture (`clean-arch`)

PENDING.

### 5.2 Domain-driven design (`ddd-specialist`)

PENDING.

### 5.3 SOLID / component principles (`solid-architect`)

PENDING.

### 5.4 Rust systems (`rust-systems`)

PENDING.

## 6. Non-goals

- **Positive `EXISTS { }`** — still parser-absent; no named consumer (RFC-034 §6 stands).
- **Inner-WHERE grammar widening** (`IN`/`AND`/`OR`/`NOT` in subquery WHERE) — real demand exists (`raid-completeness.cypher:21-36` wants inner `IN $rewrite`) but it is a *parser* extension; stays deferred per RFC-034 §6. Correlation does not require it: the anti-join shapes need no inner WHERE at all, or scalar Compare only.
- **`:CALLED_BY` reverse edge** (#546 option 2) — REJECTED: duplicates a derivable fact (`INVOKES_AT` traversed inward), costs a `SchemaVersion` bump + lockstep + keyspace growth, and adds zero expressiveness over the correlated anti-join. Recorded as the disposition of #546's alternative.
- **Correlated `OPTIONAL MATCH` + null-fill** (alternative floated in `arch-ban-rfc-053-…cypher:37-39`) — not pursued; correlated `NOT EXISTS` covers the rule class directly.
- **Sub-evaluator warning propagation** — pre-existing gap, unchanged (noted §3.2).
- **UDFs, template composition** — RFC-034 §6 unchanged.

## 7. Issue decomposition

One vertical slice (grammar exists; capability + docs + rule tightening are one observable behavior change):

**55-A — correlated `NOT EXISTS` end-to-end** (re-scopes #546):
seeded sub-evaluation (`run_seeded`), inner-WHERE outer-ref resolution, docs truth pass (query-dsl.md, predicates README, the eight workaround headers), pattern-B example rule tightened + pinned test flipped 2 → 0.

```
Tests:
  - Unit: eval predicate — correlated anti-join true/false per outer row; label re-check on a
    correlated variable; inner-fresh variable stays existential; inner WHERE outer-ref resolves
    (the README:26 None-footgun case, asserted both directions); empty-seed regression (run()
    ≡ run_seeded(empty)).
  - Self dogfood (cfdb on cfdb): tightened vertical-split-brain-drop rule on the vsb fixture —
    both_keys_wire_registered flips to 0 rows (test edit in same PR); full .cfdb/queries/*.cypher
    battery on cfdb-self keyspace: row counts byte-identical to develop (the §3.3 no-op sweep,
    executed not assumed).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): ci/cross-dogfood.sh — 0 findings
    (exit 30 on any rule row blocks merge).
  - Target dogfood (on qbot-core at pinned SHA): zero-callers anti-join
    (MATCH (i:Item) WHERE ... AND NOT EXISTS { MATCH ()-[:INVOKES_AT]->(i) } ...) executes on
    the qbot-core keyspace; report row count + wall time in the PR body for reviewer sanity-check.
```
