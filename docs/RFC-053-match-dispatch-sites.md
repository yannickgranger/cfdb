# RFC-053 — `:MatchSite` + `MATCHES_ON`: enum-dispatch facts for split-resolution-point fences

```
status: Draft — pending 4-lens council review
author: A0 (use-case analysis session 2026-07-15)
schema: V0_6_0 → V0_7_0 (new node label + two edge labels)
refs: #478, #430, RFC-040, RFC-041 (literal extraction), RFC-043, RFC-036 (Draft), RFC-037 §7
```

## 1. Problem

**Use-case-first framing.** The capability gap was derived by analyzing the debt classes cfdb is
contracted to fence in its target repos (agentry, qbot-core) and in itself, not by inventorying
missing syntax nodes. Five use cases were analyzed; four are already covered or in flight; one is
a genuine blind spot.

| UC | Debt class | On-record instances | Structural signal needed | Status today |
|---|---|---|---|---|
| UC1 | **Split resolution point** — the same enum dispatched to values/behavior at N sites | `Visibility` AST→wire→enum at 3 sites (#478); output `--format` flag with 3 implementations (2026-W17 audit Pattern 1); agentry FSM phase enum (agentry #496 fence family) | *which type each `match` dispatches on, per site* | **INVISIBLE — this RFC** |
| UC2 | Same formula reimplemented (`qname` 2 paths, `last_segment` 2 sites) | 2026-W17 audit Pattern 1 | body-shape similarity (`dup_cluster_id`) | RFC-036 (Draft, pending R2) — no new vocabulary needed |
| UC3 | Const/alias table divergence | qbot currency alias maps (RFC-040 origin) | `:ConstTable.entries_hash` overlap | Shipped (RFC-040) |
| UC4 | Hardcoded literal domain scattered | agentry phase-name strings | `:Literal` value overlap | Shipped (RFC-041 literal extraction, v0.4.0) |
| UC5 | Sites have already *diverged* (site A maps `PubCrate → "pub(crate)"`, site B → `"crate"`) | #478 drift risk | arm→output pairing | **Deliberately NOT built** — forensic, not preventive; once UC1 narrows 40k items to 2 fns, an agent reads both bodies. Arm-level nodes stay retired (RFC-037 §7) |

**The blind spot, precisely.** cfdb sees enum-variant **construction** (a call expression →
`:CallSite`, RFC-043) and string **outputs** of a mapping (`:Literal`, RFC-041). It cannot see
variant **matching/destructuring** — the `match`/dispatch half of every split resolution point.
Consequence: the fence for "a second site now dispatches on this enum" is inexpressible as a
Cypher rule, and `/audit-all` found ZERO of the W17 Pattern-1 instances for exactly this reason
(#279 dogfood evidence).

**Secondary consumer — wildcard-arm hygiene.** cfdb's own gate-domain record documents the
`_ =>` catch-all-on-schema-enum hazard (#430, RFC-044 §3.7 AC (c)): a wildcard arm silently
absorbs future variants. A `wildcard` flag per match site makes "no catch-all arms on
`#[non_exhaustive]` schema enums outside the designated evolution point" a writable rule.

**Why not a direct `(:Item{fn})-[:MATCHES_ON]->(:Item{enum})` edge?** The flagship instance
kills it: #478's three sites match on `syn::Visibility` — an **external** type with no workspace
`:Item` node. A direct edge would either miss the flagship use case or require a synthesized
external-type stub node, which is the exact anti-pattern this repo has rejected before
(stub-discriminator = two types conflated and re-split with a flag). The ratified precedent for
"per-site fact whose target may not resolve" is `:CallSite`: a site node carrying the
**name-level path as written**, plus a resolved edge only when resolution succeeds. This RFC
follows that precedent symmetrically.

## 2. Scope

Ships:

1. `:MatchSite` node — one per `syn::ExprMatch` × distinct matched-type prefix, emitted by the
   syn extractor during the existing fn-body walk.
2. `DISPATCHES_AT` edge — `(:Item{kind∈{fn,method}})-[:DISPATCHES_AT]->(:MatchSite)`, mirroring
   `INVOKES_AT`.
3. `MATCHES_ON` edge — `(:MatchSite)-[:MATCHES_ON]->(:Item{kind:"enum"})`, emitted post-walk by
   the existing deferred-resolution pipeline when the name-level prefix resolves to a workspace
   enum.
4. `SchemaVersion` bump `V0_6_0 → V0_7_0` + graph-specs lockstep PR (RFC-033 §4 I2).
5. A split-resolution-point fence rule template (`examples/queries/`) + the first live fence.

Does not ship: anything in §6 (Non-goals).

## 3. Design

### 3.1 `:MatchSite` node

One node per (match expression, distinct matched-type prefix) pair. Id follows the `:CallSite`
discipline: deterministic, position-discriminated, built by a `cfdb_core::qname` helper (slice
53-A reuses/extends the existing call-site id helper; no new id scheme).

| attr | type | semantics |
|---|---|---|
| `matched_type` | string | **Name-level, unresolved** — the all-but-last-segment prefix of a multi-segment arm-pattern path, *as the author wrote it* (`Visibility`, `syn::Visibility`, `cfdb_core::visibility::Visibility` are three distinct values for the same type). Same "textual view" doctrine as `:CallSite.callee` (`call_visitor.rs` header). |
| `file` | string | workspace-relative path |
| `line` | u32 | 1-indexed, match expression start |
| `arm_count` | u32 | number of arms of the enclosing match expression |
| `wildcard` | bool | true iff the match has a `_` or unbound-identifier catch-all arm (consumer: #430-class rules) |
| `is_test` | bool | same `#[cfg(test)]`-depth propagation as `:CallSite` / `:Literal` — the predicate is threaded, never re-evaluated (RFC-041 §4 fidelity invariant applies verbatim) |
| `crate` | string | owning crate |

**Prefix extraction rule (deterministic, closed).** Walk each arm's `syn::Pat` recursively
(`Pat::Path`, `Pat::TupleStruct`, `Pat::Struct`, through `Pat::Reference` / `Pat::Or` /
`Pat::Paren` / nested tuple-struct args). For every path with ≥ 2 segments, the prefix is all
but the last segment. Collect distinct prefixes across all arms of one match expression; emit
one `:MatchSite` per prefix (so `Some(Visibility::Pub)` under `match opt` yields one site with
`matched_type = "Visibility"`; the single-segment `Some` is skipped). Single-segment pattern
paths (`use Visibility::*; match v { Pub => … }`) are **skipped** — a single segment cannot be
distinguished from a unit-struct/binding name at syn level. Documented recall limit, measured by
the 53-A fixture.

### 3.2 Edges

```
(:Item{kind:"fn"|"method"}) -[:DISPATCHES_AT]-> (:MatchSite)          # always, walk-time
(:MatchSite)                -[:MATCHES_ON]->    (:Item{kind:"enum"})  # post-walk, when resolved
```

`MATCHES_ON` resolution reuses the RFC-037 §3.2/§3.4 deferred-resolution pipeline
(`crates/cfdb-extractor/src/resolver.rs`) tiers 1–2 exactly: exact-qname match against
`emitted_item_qnames`, else unique-last-segment via the `by_last_segment` index, ambiguous
drops silently (safer than mis-attribution). Tier 3 (wrapper unwrap) does not apply — prefixes
are paths, not types. Resolution targets are constrained to `kind = "enum"`; a prefix that
resolves to a struct/trait emits nothing (struct destructuring is not dispatch — §6).

External types (`syn::Visibility`) therefore get a `:MatchSite` with a name-level
`matched_type` and **no** `MATCHES_ON` edge — the honest representation, and sufficient for
regex-scoped fence rules. Workspace-local enums additionally get the resolved edge, which
disambiguates same-named types across crates and enables enum-side aggregation.

### 3.3 Extractor integration

New `match_visitor.rs` in `cfdb-extractor`, sibling of `call_visitor.rs` / `literal_visitor.rs`,
driven from the same `syn::visit::visit_block` walk with the same `is_test` threading. Emits the
`:MatchSite` node + `DISPATCHES_AT` edge inline; queues `(site_id, prefix_string)` on a new
`Emitter.deferred_match_targets` for the post-walk `MATCHES_ON` pass. Determinism per the
resolver.rs G1 note: deferred entries append in walk order; edges land before the final
`edges.sort_by(sort_key)` in `extract_workspace`, so on-disk ordering is queue-order-independent.

### 3.4 Wire format / schema

- `EdgeLabel::DISPATCHES_AT`, `EdgeLabel::MATCHES_ON` consts in
  `cfdb-core/src/schema/labels.rs`; `Label::MATCH_SITE` node label.
- `EdgeLabelDescriptor` entries with explicit `provenance: Extractor`, `from`/`to` grammar as in
  §3.2; `NodeLabelDescriptor` for `:MatchSite` with the §3.1 attribute table.
- `SchemaVersion::V0_7_0`, `CURRENT = V0_7_0`. Per the V0_6_0/G4 precedent (50-A): additive
  vocabulary still bumps, because pre-V0_7_0 readers must refuse graphs whose rules may depend
  on `:MatchSite`. Pre-V0_7_0 keyspaces carry zero `:MatchSite` nodes (same compat language as
  `:Literal`).
- New `pub` types get `specs/concepts/cfdb-core.md` entries (`make graph-specs-check`).

### 3.5 Cypher / CLI

No new Cypher constructs, no new CLI verb. The fence rule is plain existing subset:

```cypher
// split-resolution-point fence (concept-scoped — see §3.6)
MATCH (m:MatchSite)
WHERE m.matched_type =~ '(^|::)Visibility$'
  AND m.is_test = false
  AND NOT m.file =~ '^crates/cfdb-core/src/visibility'
RETURN m.file, m.line, m.matched_type
```

### 3.6 Fence semantics — multiplicity is not a violation

Matching on your own enum from many fns is normal Rust. Raw `MATCHES_ON` in-degree is a *survey*
metric, not a ban signal. A **fence** requires a designation: "type T's dispatch-to-values is
owned by module/fn F" — expressed either as a rule-file regex allowlist-of-one (the `NOT m.file`
clause above; note this is a scoping predicate inside a reviewed `.cypher` source file, not a
metric-ratchet baseline file — §3 no-ratchet rule is not implicated) or via the existing concept
overlay (`CANONICAL_FOR`). cfdb ships the fact + the rule template; each repo designates its own
canonical sites (agentry: phase enum; qbot-core: alias/normalization enums; cfdb: `Visibility`
after #478 collapses the 3 sites to `cfdb-core::visibility`).

## 4. Invariants

- **Determinism (G1).** Two extracts of an unchanged tree are byte-identical: walk-order
  emission + final sort, position-discriminated ids, `BTreeMap` props. `ci/determinism-check.sh`
  covers it automatically once emission lands.
- **Recall.** rustdoc-json carries no match-expression facts — `cfdb-recall`'s ground truth
  cannot oracle this fact kind (`includes_private: false` corpus is item-level). Per §2.5
  hierarchy the prescribed substitute is (a) a fixture crate with a hand-counted match-site
  inventory (unit-level ground truth) and (b) self-dogfood assertions against source-verified
  known sites (#478's three `Visibility` conversion sites are a pre-counted oracle). The
  single-segment-pattern recall limit (§3.1) is measured by the fixture and reported.
- **No ratchet.** No baseline/allowlist files; fence scoping lives in reviewed `.cypher` rule
  source (§3.6).
- **Keyspace backward compat.** V0_7_0 is a G4 breaking bump: V0_6_0 readers refuse V0_7_0
  graphs by design. Lockstep graph-specs PR per RFC-033 §4 I2 / `docs/cross-fixture-bump.md`
  §4 — merge cfdb first, fixture bump within minutes; exit-20 window documented.
- **RFC-037 §7 stays ratified.** `:MatchSite` extends the *site-node* family (`:CallSite`
  precedent), not a `:Statement`/`:Expression` granularity reopening; arms are counted
  (`arm_count`) and flagged (`wildcard`), never modeled as nodes.

## 5. Architect lenses

> Verdicts to be captured inline by the agent-team council (CLAUDE.md §2.3). Questions are
> pre-drafted; each lens also prescribes/ratifies the `Tests:` blocks in §7.

### 5.1 Clean architecture — VERDICT: PENDING

- Is the deferred-resolution reuse (resolver.rs) a clean extension point, or does a second
  deferred queue on `Emitter` start accreting a god-struct? (Emitter already carries
  `deferred_returns`; assess before a third queue lands.)
- `match_visitor.rs` sibling placement vs. folding into `call_visitor.rs` — one walk or two?
  (Perf: the block is already visited once for call sites; a second `visit_block` pass per fn
  body is O(2×). Council decides whether 53-A composes visitors in a single pass.)

### 5.2 DDD — VERDICT: PENDING

- Homonym check: `matched_type` vs `:CallSite.callee` vs `:Item.qname` — is "name-level path as
  written" the same ubiquitous concept in all three, and is the attribute name right?
- Is `DISPATCHES_AT` the right verb (vs `MATCHES_AT`)? `INVOKES_AT` symmetry argues for
  `*_AT` + a distinct verb per site family.
- `wildcard` naming: `has_catch_all` may be closer to domain language (#430 uses "catch-all").

### 5.3 SOLID / component — VERDICT: PENDING

- SRP on `match_visitor.rs`: prefix extraction (§3.1) is a pure function — prescribe its
  extraction into a unit-testable module from day one (500-LOC gate pressure on visitors is
  a known failure mode, #467).
- Does `:MatchSite` belong in the pure code-facts core (it does — L1 per #279's layer map) and
  does anything here leak toward the classifier layer? (The fence *rule* is a consumer artifact,
  not core vocabulary — verify the RFC keeps that boundary.)

### 5.4 Rust systems — VERDICT: PENDING

- `syn::Pat` recursion completeness: confirm the §3.1 pattern-kind list covers `Pat::Ident`
  with sub-pattern (`ident @ Visibility::Pub`), slice patterns, and rest patterns without
  panicking on future syn minor versions.
- Extraction cost at qbot-core scale (238 crates): the walk is already O(body); prefix
  extraction adds O(arms × pattern-depth). Expect noise-level; 53-A target dogfood reports
  wall-clock delta.
- Macro-expanded matches: `syn` sees only source tokens — matches generated by `macro_rules!`
  are invisible (RFC-041 §6 precedent). Confirm this is documented as an evasion path in the
  fence rule docs rather than "fixed" via expansion.

## 6. Non-goals

- **`:MatchArm` / `:Statement` / `:Expression` nodes** — RFC-037 §7 retirement stands; UC5
  (content-divergence forensics) is an agent read, not a graph fact.
- **`if let` / `while let` / `let-else`** — two-arm sugar; no on-record split-brain instance is
  if-let-shaped. Add only when a consumer rule demands it (new RFC or amendment).
- **Struct destructuring patterns** — destructuring is not dispatch; `MATCHES_ON` targets
  enums only.
- **HIR-resolved tier** — a future `MATCHES_ON` upgrade could type-resolve scrutinees the way
  HIR CALLS resolves callees (catches `use X::*` single-segment arms and external types as
  typed facts). Deferred until name-level recall proves insufficient on a real fence.
- **Polyglot (PHP `match`/`switch`, TS `switch`/discriminated unions)** — carried by the
  TreeSitterProducer generalization (#476) once RFC'd; this RFC is Rust-only.
- **Literal-scrutinee matches** (`match n { 0 => … }`) — no type prefix, no fact.
- **Divergence *content* detection** — see UC5.

## 7. Issue decomposition

Vertical slices; each is observable end-to-end (extract → query returns rows). Architects
finalize each `Tests:` block at council time; drafts below.

### 53-A — `:MatchSite` + `DISPATCHES_AT` end-to-end (schema + visitor + bump + lockstep)

Label consts, descriptors, `match_visitor.rs` (prefix extraction as a pure module),
`V0_7_0` bump, graph-specs lockstep PR, `specs/concepts/` entries.

```
Tests:
  - Unit: prefix-extraction pure fn on a fixture pattern set — multi-segment, nested
    Some(X::Y), or-patterns, ref patterns, ident@ sub-patterns, single-segment skip,
    wildcard/arm_count counting; hand-counted fixture-crate match-site inventory equality.
  - Self dogfood (cfdb on cfdb): MATCH (m:MatchSite) WHERE m.matched_type =~ '(^|::)Visibility$'
    returns ≥ the source-verified #478 site list; every :MatchSite has a DISPATCHES_AT parent;
    determinism-check green.
  - Cross dogfood (graph-specs-rust at pinned SHA): zero rule rows (exit 30 blocks merge);
    lockstep .cfdb/cross-fixture.toml bump PR open before cfdb merge.
  - Target dogfood (qbot-core at pinned SHA): report node count, top-10 matched_type values,
    and extract wall-clock delta vs develop in the PR body.
```

### 53-B — `MATCHES_ON` resolution pass

Deferred queue + resolver tiers 1–2 constrained to `kind="enum"`; enum-side survey query in
`examples/queries/` (top matched-on enums).

```
Tests:
  - Unit: resolution tiers on fixture — exact qname hit, unique last-segment hit, ambiguous
    drop, struct-prefix drop, external-prefix (syn::Visibility) yields node-without-edge.
  - Self dogfood (cfdb on cfdb): cfdb_core Visibility enum has ≥1 incoming MATCHES_ON from
    crates/cfdb-core/src/visibility.rs sites; zero MATCHES_ON edges whose dst kind ≠ enum.
  - Cross dogfood: zero rule rows at pinned SHA.
  - Target dogfood (qbot-core): report resolved-edge count + resolution rate
    (MATCHES_ON ÷ MatchSite) in PR body — the number that decides whether the HIR tier
    (§6) ever gets an RFC.
```

### 53-C — split-resolution-point fence template + first live fence

`examples/queries/split-resolution-point.cypher` template + docs section (fence semantics §3.6,
macro evasion path §5.4) + the first `.cfdb/queries/` fence on cfdb-self. **Ordering:** the
Visibility fence requires #478 (site collapse) merged first for a zero-violation baseline —
the ban-rule-lands-with-proof rule (§3 dogfood table); if #478 stalls, council picks an
alternative already-canonical concept for the first fence.

```
Tests:
  - Unit: none — rationale: the rule file is declarative; its behavior is the dogfood row.
  - Self dogfood (cfdb on cfdb): fence rule returns 0 rows on develop (proof in PR);
    red-test companion — a fixture branch adding a second Visibility match site trips exit 30.
  - Cross dogfood: zero rule rows at pinned SHA (template must not fire on companion).
  - Target dogfood (qbot-core): run the template scoped to one qbot alias enum; report row
    count in PR body for maintainer triage (rows here are *findings*, not merge blockers —
    qbot fences are qbot-repo decisions).
```

## Appendix — why the direct-edge design was rejected (decision record)

First sketch was `(:Item{fn})-[:MATCHES_ON {file,line}]->(:Item{enum})`, no site node. Killed
during design verification: #478's sites match on `syn::Visibility`, which has no workspace
`:Item` dst. The alternatives were (a) drop external-type sites — misses the flagship instance;
(b) synthesize stub nodes for external types — rejected pattern (stub discriminator = conflated
types); (c) site node with name-level prop + optional resolved edge — the `:CallSite` precedent.
(c) ships. This also keeps `Edge` bag-semantics simple (no parallel-edge prop discrimination
needed) and gives fence rules a regex surface that works identically for internal and external
types.
