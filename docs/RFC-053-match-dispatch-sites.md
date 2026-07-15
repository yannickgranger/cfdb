# RFC-053 — `:MatchSite` + `MATCHES_ON`: enum-dispatch facts for split-resolution-point fences

```
status: RATIFIED — R2 council 4/4 RATIFY, 2026-07-15 (R1: 4× REQUEST CHANGES → amendments
        applied → R2 unanimous). Verdict record: council/RFC-053/RATIFIED.md
author: A0 (use-case analysis session 2026-07-15; R1 council same day)
schema: V0_6_0 → V0_7_0 (one bump, landing in slice 53-A: one node label + two edge labels)
refs: #279 (W17 audit EPIC), #430 + RFC-044 §3.7 (wildcard-arm policy record), boy-scout #107
      (commit 2aedd013 — the historical Visibility split-brain fix), RFC-040, RFC-041 (literal
      extraction), RFC-043, RFC-036 (Draft), RFC-037 §7, agentry #496 (fence family)
```

## 1. Problem

**Use-case-first framing.** The capability gap was derived by analyzing the debt classes cfdb is
contracted to fence in its target repos (agentry, qbot-core) and in itself, not by inventorying
missing syntax nodes. Five use cases were analyzed; four are already covered or in flight; one is
a genuine blind spot.

| UC | Debt class | Evidence (source-verified at R1 council) | Structural signal needed | Status today |
|---|---|---|---|---|
| UC1 | **Split resolution point** — the same enum dispatched to values/behavior at N sites | agentry FSM phase enum (agentry #496 fence family — construction-side fences 1–2 shipped; the *matching-side* fence is inexpressible today); cfdb's own historical `parse_syn_visibility` split-brain (fixed 2026-04-20, boy-scout #107 / commit `2aedd013` — see narrative below); #279's W17-audit list also names a `--format` flag with 3 implementations (liveness NOT re-verified — 53-C must re-verify before using it as a fence target) | *which type each `match` dispatches on, per site* | **INVISIBLE — this RFC** |
| UC2 | Same formula reimplemented (`qname` 2 paths, `last_segment` 2 sites) | #279 W17 audit Pattern 1 | body-shape similarity (`dup_cluster_id`) | RFC-036 (Draft, pending R2) — no new vocabulary needed |
| UC3 | Const/alias table divergence | qbot currency alias maps (RFC-040 origin) | `:ConstTable.entries_hash` overlap | Shipped (RFC-040) |
| UC4 | Hardcoded literal domain scattered | agentry phase-name strings | `:Literal` value overlap | Shipped (RFC-041 literal extraction, v0.4.0) |
| UC5 | Sites have already *diverged* (site A maps a variant to `"pub(crate)"`, site B to `"crate"`) | drift risk inherent to UC1 | arm→output pairing | **Deliberately NOT built** — forensic, not preventive; once UC1 narrows 40k items to 2 fns, an agent reads both bodies. Arm-level nodes stay retired (RFC-037 §7) |

**The worked instance — and why it argues for a fence, not a fix.** cfdb had a real UC1
split-brain: `parse_syn_visibility` used to construct `Visibility` variants directly, bypassing
`Visibility::FromStr` — two resolution points for the same mapping. `audit-split-brain` (a
semantic tool, run during a manual audit) caught it; boy-scout #107 (commit `2aedd013`,
2026-04-20) collapsed it. Five days later the W17 audit EPIC (#279, filed 2026-04-25) still
documented it as live — **the prose debt record was stale within a week of being written.**
Today the conversion is genuinely single-owner, and `crates/cfdb-extractor/src/item_visitor.rs`
self-documents it as "the canonical (and only) AST → Visibility mapping" — i.e. canonicality is
currently enforced by **a doc-comment alone**. Nothing structural prevents a second site from
reappearing, and cfdb cannot express the rule that would catch it, because matching/destructuring
is invisible to extraction. That is the precise blind spot: cfdb sees enum-variant
**construction** (`:CallSite`, RFC-043) and mapping **outputs** (`:Literal`, RFC-041), never the
**match** half of a resolution point. `/audit-all`'s structural detectors found ZERO of the W17
Pattern-1 instances for exactly this reason (#279 dogfood evidence).

**Secondary consumer — wildcard-arm policy.** RFC-044 §3.7 AC (c) prescribed explicit,
non-silent handling for the `_ =>` arm on a schema enum, and #430 is the standing amendment
record for that prescription — evidence that *where wildcard arms may appear on evolving enums*
is a real, reviewed policy concern in this repo. A `wildcard` fact per match site makes
"wildcard arms on `#[non_exhaustive]` schema enums only at designated evolution points" a
writable rule instead of a per-PR review-memory item.

**Why not a direct `(:Item{fn})-[:MATCHES_ON]->(:Item{enum})` edge?** The verified instance
kills it: `parse_syn_visibility` matches on `syn::Visibility` — an **external** type with no
workspace `:Item` node. A direct edge would either miss external-type dispatch entirely or
require a synthesized stub node, which this repo has rejected before (a stub discriminator =
two types conflated and re-split with a flag). The ratified precedent for "per-site fact whose
target may not resolve" is `:CallSite`: a site node carrying the **name-level path as written**,
plus a resolved edge only when resolution succeeds. This RFC follows that precedent
symmetrically.

## 2. Scope

Ships:

1. `:MatchSite` node — one per `syn::ExprMatch` × distinct matched-path prefix, emitted by the
   syn extractor during the established per-fn-body visitor walk.
2. `MATCHES_AT` edge — `(:Item{kind∈{fn,method}})-[:MATCHES_AT]->(:MatchSite)`, mirroring
   `INVOKES_AT` (one verb root — *match* — across the whole family).
3. `MATCHES_ON` edge — `(:MatchSite)-[:MATCHES_ON]->(:Item{kind:"enum"})`, emitted post-walk by
   the existing deferred-resolution pipeline when the name-level prefix resolves to a workspace
   enum.
4. `SchemaVersion` bump `V0_6_0 → V0_7_0` + graph-specs lockstep PR (RFC-033 §4 I2) — **once**,
   in slice 53-A; 53-B and 53-C add no schema surface.
5. A split-resolution-point fence rule template (`examples/queries/`) + the first live fence
   (the `syn::Visibility` regression guard — zero-violation baseline exists today, §7 53-C).

Does not ship: anything in §6 (Non-goals).

## 3. Design

### 3.1 `:MatchSite` node

One node per (match expression, distinct matched-path prefix) pair. Id is **extractor-local**,
mirroring the verified `:CallSite` formula (`call_visitor.rs:190-193`:
`callsite:{caller_qname}:{callee_path}:{local_idx}`) as
`matchsite:{fn_qname}:{prefix}:{local_idx}`, where `local_idx` is a per-prefix-text occurrence
counter within one fn body — deliberately NOT a `cfdb_core::qname` helper, because RFC-032 §3's
resolver-discriminator contract keeps site-id schemes out of core (syn-tier and HIR-tier site
ids must be free to differ). The prefix is a mandatory id component (one match expression can
emit several sites), and **prefix dedup must happen per match expression BEFORE the counter
increments** — a naive per-arm-emit-then-dedup implementation over-increments the shared
counter (R1 rust-systems: a correctness bug, not just a determinism bug; 53-A carries both
dedup tests).

| attr | type | semantics |
|---|---|---|
| `matched_path` | string | **Name-level, unresolved** — the all-but-last-segment prefix of a multi-segment arm-pattern path, *as the author wrote it* (`Visibility`, `syn::Visibility`, `cfdb_core::visibility::Visibility` are three distinct values for the same type). Same doctrine as `:CallSite.callee_path`. Named `matched_path`, NOT `matched_type`: it is a syntactic pattern-path prefix, not the scrutinee's resolved type — that distinct concept is reserved for the future HIR tier (§6). |
| `file` | string | workspace-relative path |
| `line` | u32 | 1-indexed, match expression start |
| `arm_count` | u32 | number of arms of the enclosing match expression |
| `wildcard` | bool | true iff the match has a wildcard arm — RFC-044 §3.7's vocabulary. Detection: a top-level `Pat::Wild`, or a bare `Pat::Ident` with no sub-pattern whose identifier starts lowercase (Rust naming convention distinguishes a fresh binding from a unit-variant/const path; syn does no name resolution, so this is a **documented heuristic** — named recall limit #2, measured by the 53-A fixture). The implementation carries a doc-comment stating the flag covers BOTH forms — literal `_` and lowercase catch-all bindings (R2 ddd: the Reference reserves "wildcard" for `_` alone; the Book groups both as catch-all patterns). |
| `is_test` | bool | same `#[cfg(test)]`-depth propagation as `:CallSite` / `:Literal` — the predicate is threaded, never re-evaluated (RFC-041 §4 fidelity invariant applies verbatim) |
| `crate` | string | owning crate |

**Prefix extraction rule (deterministic, closed).** Implemented as a pure function in its own
module (`match_visitor/prefix.rs` — §3.3). Walk each arm's `syn::Pat` recursively through the
closed variant list (syn 2.0.117, `Pat` is `#[non_exhaustive]`, 16 variants — R1
rust-systems verified against the pinned registry): `Path`, `TupleStruct`, `Struct`
(path-bearing); `Ident` (recurse `@` sub-pattern if present), `Reference`, `Or`, `Paren`,
`Tuple`, `Slice` (recurse containers); `Wild`, `Rest`, `Lit`, `Range`, `Const`, `Macro`,
`Verbatim`, `Type` (leaves — contribute no path). The compiler forces a trailing `_ => {}` arm
against `#[non_exhaustive]` regardless, so future syn minor bumps cannot panic here — they fall
through to no-contribution. For every path with ≥ 2
segments, the prefix is all but the last segment. Collect distinct prefixes across all arms of
one match expression; emit one `:MatchSite` per prefix (so `Some(Visibility::Pub)` yields one
site with `matched_path = "Visibility"`; the single-segment `Some` is skipped). Single-segment
pattern paths (`use Visibility::*; match v { Pub => … }`) are **skipped** — indistinguishable
from bindings at syn level. Named recall limit #1, measured by the 53-A fixture.

### 3.2 Edges

```
(:Item{kind:"fn"|"method"}) -[:MATCHES_AT]-> (:MatchSite)          # always, walk-time
(:MatchSite)                -[:MATCHES_ON]-> (:Item{kind:"enum"})  # post-walk, when resolved
```

`MATCHES_ON` resolution is a **standalone short `resolve_deferred_match_targets` in
`resolver.rs`**, calling the *same pure primitives* the RETURNS/TYPE_OF passes use —
`resolve_type_string` (tier 1: exact qname; tier 2: unique last-segment via
`build_last_segment_index`; ambiguous drops silently; both promoted `pub(crate)` — currently
private). The reuse is **primitive-level, deliberately not function-level** (R1 converged
position, rust-systems ↔ clean-arch ↔ solid): the three orchestrations genuinely diverge —
different queue tuple arity, MATCHES_ON has no tier-3 and a `kind="enum"` filter the others
lack — so a generic combinator would need enough parameters to be worse than three short
siblings, while a copy of the full orchestration would be the exact debt class this RFC fences.
The primitives are currently zero-unit-tested; 53-B adds their direct unit tests as a
prescribed byproduct. Tier 3 (wrapper unwrap) does not apply —
prefixes are paths, not types. Resolution targets are constrained to `kind = "enum"`; a prefix
resolving to a struct/trait emits nothing (struct destructuring is not dispatch — §6).

External types (`syn::Visibility`) therefore get a `:MatchSite` with a name-level `matched_path`
and **no** `MATCHES_ON` edge — the honest representation. Workspace-local enums additionally get
the resolved edge. **The two are complementary fence predicates**: an unqualified
`Visibility::…` arm yields the same `matched_path` prefix for the workspace enum and an imported
external type — name-level regex alone cannot distinguish same-named types; the presence or
absence of `MATCHES_ON` can (§3.5).

### 3.3 Extractor integration

New **directory module** `crates/cfdb-extractor/src/match_visitor/{mod.rs, prefix.rs}` — a
directory from day one, not a flat file (R1 solid: `type_render.rs` is at 496/500 LOC and
`item_visitor/emit/mod.rs` at 452/500 against the 500-LOC gate; the near-gate-file failure mode
is known). `prefix.rs` holds the pure §3.1 extraction function; `:MatchSite` emission is
self-contained in the module and NOT routed through `item_visitor/emit/mod.rs`.

The visitor is a **third independent `syn::visit::visit_block` pass** per fn body, driven from
the same invocation site as the existing two — this is the established, shipped pattern
(`item_visitor/visits.rs:103-119` already runs `walk_call_sites_with_test_flag` and
`walk_literals_in_block` as separate passes), not a deviation. Deferred `(site_id,
prefix_string)` entries queue on a new `Emitter.deferred_match_targets` — a third deferred queue
is a natural extension of Emitter's single deferred-resolution responsibility; no
`DeferredResolution` trait (R1 clean-arch: `.discovery/239.md:219` records "no trait dispatch"
as the intentional prior choice; YAGNI).

**Prescribed in-slice boy-scout (R1 rust-systems + solid, rule of three):** `walk_macro_tokens`
is already duplicated **functionally byte-for-byte** between `call_visitor.rs` and
`literal_visitor.rs`. 53-A does NOT add a third copy — it factors the existing two into one
shared helper (own module, generic over the visitor), closing the pre-existing zero-unit-test
gap on that logic in the same change. `match_visitor` uses the shared helper too, which means
**match expressions inside re-parseable macro *invocation* bodies ARE extracted**, consistent
with how call sites and literals already behave — only `macro_rules!` *definitions* are opaque
(§3.6, §6).

Determinism per the resolver.rs G1 note: deferred entries append in walk order; edges land
before the final `edges.sort_by(sort_key)` in `extract_workspace`, so on-disk ordering is
queue-order-independent.

### 3.4 Wire format / schema

- `EdgeLabel::MATCHES_AT`, `EdgeLabel::MATCHES_ON` consts in
  `cfdb-core/src/schema/labels.rs`; `Label::MATCH_SITE` node label.
- `EdgeLabelDescriptor` entries with explicit `provenance: Extractor`, `from`/`to` grammar as in
  §3.2; `NodeLabelDescriptor` for `:MatchSite` with the §3.1 attribute table.
- `SchemaVersion::V0_7_0`, `CURRENT = V0_7_0`. Per the V0_6_0/G4 precedent (50-A): additive
  vocabulary still bumps, because pre-V0_7_0 readers must refuse graphs whose rules may depend
  on `:MatchSite`. Pre-V0_7_0 keyspaces carry zero `:MatchSite` nodes (same compat language as
  `:Literal`). One bump total, in 53-A.
- New `pub` types get `specs/concepts/cfdb-core.md` entries (`make graph-specs-check`).

### 3.5 Cypher / CLI

No new Cypher constructs, no new CLI verb. Fence rules compose the two predicates from §3.2:

```cypher
// external-type fence: nothing outside the canonical module may match syn::Visibility.
// External type ⇒ no MATCHES_ON edge exists; the name-level regex is the only handle,
// and same-named workspace types are excluded by requiring the edge's ABSENCE upstream
// (via the resolved-edge complement query in the same rule file).
MATCH (m:MatchSite)
WHERE m.matched_path =~ '(^|::)Visibility$'
  AND m.is_test = false
  AND NOT m.file =~ '^crates/cfdb-extractor/src/item_visitor'
RETURN m.file, m.line, m.matched_path
```

For workspace enums the fence anchors on the resolved edge instead —
`MATCH (t:Item{qname:$enum})<-[:MATCHES_ON]-(m:MatchSite) WHERE NOT m.file =~ $canonical …` —
which is homonym-proof. 53-C ships both template forms.

### 3.6 Fence semantics — multiplicity is not a violation

Matching on your own enum from many fns is normal Rust. Raw `MATCHES_ON` in-degree is a *survey*
metric, not a ban signal. A **fence** requires a designation: "type T's dispatch-to-values is
owned by module/fn F" — expressed as a scoping predicate inside a reviewed `.cypher` rule file.
This is not a metric-ratchet file (§3 no-ratchet rule): the allow-scope is closed, RFC-gated in
kind (RFC-035/038/040 precedent), and guarded by the rule (R1 solid): **one fence file per
fenced type, at most one canonical-site NOT-clause per file, never an accreting exception
list** — a rule file that grows NOT-clauses has become a de facto allowlist and is rejected on
sight. cfdb ships the fact + the rule templates; each repo designates its own canonical sites
(agentry: phase enum; qbot-core: alias/normalization enums; cfdb: the `syn::Visibility`
conversion at its self-documented canonical site).

**Documented evasion paths** (fence docs must name all three): match arms inside
`macro_rules!` **definitions** (genuinely opaque — syn never parses an `ItemMacro` token tree;
note macro *invocation* bodies are NOT an evasion path, §3.3); **`matches!()` /
`assert_matches!` invocations** (§6 — excluded by name in v0; the idiom appears in 26
production `src/` files workspace-wide, so this is a measured in-scope limit, not a corner
case); single-segment patterns under glob imports (§3.1 limit #1).

## 4. Invariants

- **Determinism (G1).** Two extracts of an unchanged tree are byte-identical: walk-order
  emission + final sort, prefix-bearing position-discriminated ids (§3.1), `BTreeMap` props.
  `ci/determinism-check.sh` covers it automatically once emission lands. 53-A carries the
  multi-prefix-same-expression dedup-before-id test.
- **Recall.** rustdoc-json carries no match-expression facts — `cfdb-recall`'s rustdoc oracle
  cannot cover this fact kind. Per §2.5 hierarchy the substitute is (a) a fixture crate with a
  hand-counted match-site inventory and (b) self-dogfood assertions against the two
  source-verified canonical sites in cfdb's own tree (§7 53-A). **Three named recall limits**
  are measured by the fixture and documented, never silently absorbed: single-segment patterns
  (§3.1), the lowercase-heuristic on bare-ident wildcard detection (§3.1), and `matches!()`
  invocations (§6).
- **No ratchet.** No baseline/allowlist files; fence scoping per §3.6 with the
  one-clause guardrail.
- **Keyspace backward compat.** V0_7_0 is a G4 breaking bump: V0_6_0 readers refuse V0_7_0
  graphs by design. Lockstep graph-specs PR per RFC-033 §4 I2 / `docs/cross-fixture-bump.md`
  §4 — merge cfdb first, fixture bump within minutes; exit-20 window documented.
- **RFC-037 §7 stays ratified.** `:MatchSite` extends the *site-node* family (`:CallSite`
  precedent), not a `:Statement`/`:Expression` granularity reopening — R1 ddd verified the
  actual retirement rationale targets general AST-as-graph modeling, not narrow site nodes.
  Arms are counted (`arm_count`) and flagged (`wildcard`), never modeled as nodes.

## 5. Architect lenses — R1 verdicts (2026-07-15 agent-team council)

All four lenses independently found the core architecture sound (site-node + optional resolved
edge, `:CallSite` precedent, L1 placement, resolver reuse). All four returned **REQUEST
CHANGES** on a bounded fix set, applied in this revision. R2 confirmation pending.

### 5.1 Clean architecture — R1: REQUEST CHANGES → applied

Found: dependency direction, StoreBackend purity, L1 classification all clean; two-pass visitor
precedent verified at `visits.rs:103-119` (third pass ratified; do not compose one `Visit`
impl); Emitter third queue is not god-struct accretion (no trait — YAGNI per `.discovery/239.md`);
**the claimed `cfdb_core::qname` call-site id helper does not exist** — `:CallSite` id is a
deliberate extractor-local inline format (RFC-032 §3 discriminator), §3.1 corrected to follow
it; flagship citation defect (see §5 header + Appendix).

### 5.2 DDD — R1: REQUEST CHANGES → applied

Found: `matched_type` renamed `matched_path` (it is a pattern-path prefix under the
`callee_path` doctrine, not a resolved scrutinee type — that name is reserved for the future HIR
tier); `DISPATCHES_AT` renamed `MATCHES_AT` ("dispatch" is established cfdb vocabulary for
*call-target resolution*; one verb root across the family); `wildcard` KEPT (RFC-044 §3.7's own
vocabulary — the "catch-all" gloss was unsourced and is removed); `:CallSite.callee` corrected
to `callee_path`; RFC-037 retirement compatibility and `kind="enum"` restriction ratified as
domain-correct; flagship citation defect (dated the staleness: fix merged 5 days before the
audit EPIC documenting it was filed).

### 5.3 SOLID / component — R1: RATIFY architecture, REQUEST CHANGES disposition → applied

Found: directory module `match_visitor/{mod.rs,prefix.rs}` mandatory from day one (496/500 and
452/500 near-gate precedents); 53-A/53-B are genuine vertical slices (RFC-045 45-D precedent);
single SchemaVersion bump, stated explicitly in 53-B's issue text; §3.6 NOT-clause scoping is
not a ratchet but needs the one-clause guardrail (adopted); `walk_macro_tokens` byte-duplicate
factoring prescribed in 53-A (rule of three); resolver pure helpers get their first direct unit
tests in 53-B; 53-C "Unit: none" escape hatch validly precedented (RFC-034/048); 53-A
self-dogfood row rewritten to the real single-site oracle; 53-C ordering clause deleted
(baseline already exists).

### 5.4 Rust systems — R1: REQUEST CHANGES → applied

Found: **flagship claim fabricated as cited** — no live 3-site Visibility duplication exists
(one `syn::Visibility` match site, self-documented canonical; `as_wire_str` matches the
workspace enum; `FromStr` matches `&str` and emits zero `:MatchSite` under §3.1) — §1 rebuilt on
the verified anatomy; **`matches!()` invisibility** — its `<expr>, <pat> [if <guard>]` grammar
is structurally incompatible with all three existing `walk_macro_tokens` re-parse tiers
(`Punctuated<Expr,Comma>` / `Block` / single `Expr`; none parse a bare `syn::Pat`) — resolved as
a named v0 exclusion with a specified upgrade path (§6); `syn::Pat` closed list expanded
(§3.1: `Ident@`, `Tuple`, `Slice` recursed; `Rest`, `Lit`, `Range`, `Const`, `Macro`,
`Verbatim`, `Type` leaves; `#[non_exhaustive]` fall-through moots the future-variant panic
worry); prefix-in-id + dedup-before-counter required (adopted, §3.1 + 53-A tests); wildcard
lowercase-heuristic specified as named recall limit #2 (solid ratified); resolver reuse settled
at **primitive level** after cross-lens deliberation — standalone short orchestration fn, no
generic combinator (§3.2); macro claim split into precise halves — invocation bodies extractable
via the shared helper, `macro_rules!` definitions opaque, `matches!()` named exclusion (§3.3,
§3.6, §6; solid corrected the first cited example — `tests/` targets aren't extracted at all
per `lib.rs:346`, the in-scope evidence is 26 production `src/` files); cost-at-scale credible
as one additional O(body) pass alongside the existing two (53-A target dogfood reports the
measured delta).

## 6. Non-goals

- **`:MatchArm` / `:Statement` / `:Expression` nodes** — RFC-037 §7 retirement stands; UC5
  (content-divergence forensics) is an agent read, not a graph fact.
- **`matches!()` / `assert_matches!` invocations** — excluded by name in v0, same template as
  the if-let exclusion below (add only when a consumer rule demands it). Rationale: their
  `<expr>, <pat> [if <guard>]` token grammar fits none of the three existing macro re-parse
  tiers (verified against `syn::Arm`'s definition — no tier parses a bare `syn::Pat`), and a
  boolean-returning one-arm check carries less split-brain weight than a full dispatch site.
  The idiom is common (26 production `src/` files in cfdb's own workspace), so this is
  **named recall limit #3**, measured by the 53-A fixture and called out in fence docs (§3.6).
  Forward guidance so a future amendment need not re-derive it (R1 council, doubly-confirmed):
  the fourth re-parse tier is **known-low-cost** — `syn::Pat::parse_multi_with_leading_vert`
  is public API (syn 2.0.117 `pat.rs:383`, the same fn syn's own `Arm` parser uses), making the
  tier a ~15–20-line addition reusing §3.1's Pat recursion; when implemented, it lives as a
  `match_visitor`-local wrapper around the shared macro-token helper, never as a parameter
  threaded through it (ISP). It becomes a new slice the first time a live fence demonstrably
  needs it. Do not re-propose `crates/cfdb-cli/tests/signatures.rs` as the motivating live
  instance: Cargo `tests/` integration targets are excluded from extraction entirely by the
  lib-or-bin target filter (`cfdb-extractor/src/lib.rs:346`), independent of this RFC.
- **`if let` / `while let` / `let-else`** — two-arm sugar; no on-record split-brain instance is
  if-let-shaped. Add only when a consumer rule demands it.
- **Struct destructuring patterns** — destructuring is not dispatch; `MATCHES_ON` targets
  enums only.
- **HIR-resolved tier** — a future upgrade could type-resolve scrutinees the way HIR CALLS
  resolves callees (catching glob-import single-segment arms, `matches!()` via HIR bodies, and
  external types as typed facts) — and would own the `matched_type` name reserved by §3.1.
  Deferred until 53-B's measured resolution rate proves name-level recall insufficient on a
  real fence.
- **Polyglot (PHP `match`/`switch`, TS `switch`/discriminated unions)** — carried by the
  TreeSitterProducer generalization (#476) once RFC'd; this RFC is Rust-only.
- **Literal-scrutinee matches** (`match n { 0 => … }`) — no type prefix, no fact (this is why
  `Visibility::FromStr`'s `&str` match correctly emits nothing).
- **Divergence *content* detection** — see UC5.

## 7. Issue decomposition

Vertical slices; each is observable end-to-end (extract → query returns rows). R1 council
prescriptions incorporated; R2 confirms.

### 53-A — `:MatchSite` + `MATCHES_AT` end-to-end (schema + visitor + bump + lockstep)

Label consts, descriptors, `match_visitor/{mod.rs, prefix.rs}` directory module, the
`walk_macro_tokens` dedup-to-shared-helper boy-scout, `V0_7_0` bump (all three vocabulary
additions), graph-specs lockstep PR, `specs/concepts/` entries.

```
Tests:
  - Unit: prefix-extraction pure fn (match_visitor/prefix.rs) on a fixture pattern set —
    multi-segment, nested Some(X::Y), or-patterns, ref patterns, ident@ sub-patterns, tuple and
    slice containers, single-segment skip (recall limit #1), wildcard heuristic incl. the
    lowercase-ident ambiguity cases (recall limit #2), matches!() non-emission (recall limit #3),
    arm_count; BOTH dedup-before-id tests — multi-prefix-same-expression (distinct sites,
    prefix-bearing ids, no collision) AND multi-arm-single-prefix (the real post-#107
    parse_syn_visibility 3-arm shape → exactly ONE site, occurrence counter increments once);
    shared walk_macro_tokens helper unit tests (first coverage of previously duplicated,
    untested logic) incl. a match-expression-inside-macro-invocation extraction case.
  - Self dogfood (cfdb on cfdb): exactly ONE :MatchSite with matched_path =~ '^syn::Visibility$'
    in crates/cfdb-extractor/src/item_visitor.rs (the self-documented canonical site); ≥1
    :MatchSite with matched_path = 'Visibility' in crates/cfdb-core/src/visibility.rs
    (as_wire_str); zero :MatchSite emitted for Visibility::FromStr's &str match; every
    :MatchSite has a MATCHES_AT parent; determinism-check green.
  - Cross dogfood (graph-specs-rust at pinned SHA): zero rule rows (exit 30 blocks merge);
    lockstep .cfdb/cross-fixture.toml bump PR open before cfdb merge.
  - Target dogfood (qbot-core at pinned SHA): report node count, top-10 matched_path values,
    and extract wall-clock delta vs develop in the PR body.
```

### 53-B — `MATCHES_ON` resolution pass

Standalone short `resolve_deferred_match_targets` calling the shared primitives
`resolve_type_string` / `build_last_segment_index` (both promoted `pub(crate)`; no generic
combinator, no parallel tier copy — §3.2 converged position); enum-side survey query in
`examples/queries/` (top matched-on enums). Issue
text states explicitly: **schema vocabulary landed in 53-A; this slice adds no labels and no
SchemaVersion change** — no second lockstep PR.

```
Tests:
  - Unit: first direct unit tests on resolve_type_string + build_last_segment_index (prescribed
    byproduct — currently zero-unit-tested); resolution fixtures — exact qname hit, unique
    last-segment hit, ambiguous drop, struct-prefix drop, external-prefix (syn::Visibility)
    yields node-without-edge.
  - Self dogfood (cfdb on cfdb): cfdb_core's Visibility enum has ≥1 incoming MATCHES_ON from a
    site in crates/cfdb-core/src/visibility.rs; the syn::Visibility site from 53-A has NO
    MATCHES_ON edge; zero MATCHES_ON edges whose dst kind ≠ enum.
  - Cross dogfood: zero rule rows at pinned SHA.
  - Target dogfood (qbot-core): report resolved-edge count + resolution rate
    (MATCHES_ON ÷ MatchSite) in PR body — the number that decides whether the HIR tier (§6)
    ever gets an RFC.
```

### 53-C — split-resolution-point fence templates + first live fence

Both template forms (§3.5: external-type regex form; workspace-enum resolved-edge form) in
`examples/queries/`, fence-semantics docs (§3.6 incl. all three named evasion paths), and the
first `.cfdb/queries/` fence: the **`syn::Visibility` regression guard** — no site outside
`item_visitor.rs` may match `syn::Visibility`. The zero-violation baseline exists on develop
today (single canonical site since boy-scout #107) — **no ordering dependency; this slice is
unblocked now.** Before reusing #279's `--format` instance in docs or as a second fence, its
liveness must be re-verified against the current tree (the W17 list has already been shown to
go stale).

```
Tests:
  - Unit: none — rationale: the rule files are declarative; behavior is the dogfood rows
    (escape-hatch use precedented by RFC-034 / RFC-048).
  - Self dogfood (cfdb on cfdb): syn::Visibility fence returns 0 rows on develop (proof in PR);
    red-test companion — a fixture branch adding a second syn::Visibility match site trips
    exit 30.
  - Cross dogfood: zero rule rows at pinned SHA (templates must not fire on companion).
  - Target dogfood (qbot-core): run the workspace-enum template scoped to one qbot alias enum;
    report row count in PR body for maintainer triage (rows here are *findings*, not merge
    blockers — qbot fences are qbot-repo decisions).
```

## Appendix — decision record

**Why the direct-edge design was rejected.** First sketch was
`(:Item{fn})-[:MATCHES_ON {file,line}]->(:Item{enum})`, no site node. Killed during design
verification, and the argument is structural, not instance-bound: **a match on any external
type is unrepresentable as a direct edge**, because the dst `:Item` does not exist in the
workspace graph (live verified example: `parse_syn_visibility` matching `syn::Visibility` —
one site, external type). Alternatives: (a) drop external-type sites — misses the exact class
the capability exists for; (b) synthesize stub nodes — rejected pattern; (c) site node with
name-level prop + optional resolved edge — the `:CallSite` precedent. (c) ships.

**R1 document-integrity postscript.** The R1 draft's flagship example cited "#478's three
Visibility sites" — the council (all four lenses, independently) established that #478 is an
unrelated issue, the real historical instance was already fixed by boy-scout #107 three months
prior, and the "3 sites" anatomy was wrong even historically. The council's correction produced
a *stronger* Problem statement: the debt-record staleness it exposed (a W17 audit bullet stale
within 5 days of filing) is itself the argument for tree-derived structural fences over prose
debt records, and the already-collapsed canonical site gives the first fence a zero-violation
baseline with no prerequisite work. Recorded here per the gate-domain amendment discipline: the
evidence trail was repaired, not repointed.
