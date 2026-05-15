# RFC-041 — string-literal extraction (`:Literal` fact type)

Status: **RATIFIED** 2026-05-15 — 4/4 lens RATIFY (clean-arch,
ddd-specialist, solid-architect, rust-systems) per CLAUDE.md §2.3.
Two rework rounds (ddd: 2 editorial; rust-systems: 5 precision).
Verdicts inline §5; recorded in `council/RATIFIED.md`.
Author: captain (a0 session 2026-05-15)
Supersedes/relates: RFC-032 (v0.2 extractor), RFC-037 (schema-producer alignment), RFC-033 (cross-dogfood lockstep)

## 1. Problem

Downstream consumer `yg/agentry` ratified a v2-finale council
(`agentry:council/v2-finale-fsm-collapse/synthesis.md`) that mandates
three cfdb ban rules as the structural fence against methodology
re-introduction. Two shipped (`arch-ban-briefstate-variants`,
`arch-ban-rundata-phase-names` — agentry PRs #540/#541). The third,
`arch-ban-phase-name-strings`, must flag string literals like
`"verifying"`/`"shipping"`/`"reviewing"` in `crates/orchestrator-*`
except inside the topology JSON parser. It is **infeasible today**:

> Verified against develop tip 2026-05-14: `MATCH (l:Literal) RETURN l`
> → `UnknownLabel`; same for `:Lit`, `:Str`. cfdb extracts only
> `:Item` (kind ∈ enum/fn/impl_block/method/static/struct/trait/
> type_alias) and `:CallSite`. String literals are not modelled.

This blocks `agentry#542` → `agentry#496` (gamma) → `agentry#497`
(migration ceremony) → closing `agentry#397/#487/#493`. The entire
v2-finale formal closure is gated on this capability. Filed as
`cfdb#367`; per CLAUDE.md §1 a new fact type is RFC-first, so #367 is
premature until this RFC ratifies.

A `:Literal` fact type is also broadly useful beyond agentry: ban
rules for hard-coded hosts/ports/credentials-shaped strings, magic
strings, and string-keyed split-brain detection — all currently
impossible in the Cypher subset.

## 2. Scope

**Ships:**

- A `:Literal` node, one per **string** literal in production source
  (`crates/*/src/**/*.rs`), with attributes: `value` (unescaped
  content), `file`, `line`, `col`, `crate`, `is_test`.
- `Label::Literal` variant in `cfdb-core` schema vocabulary.
- Additive `SchemaVersion` bump (minor within current major; G4
  monotonic).
- Extractor emission in the existing `syn`-walk (alongside
  `:CallSite`), gated by the same production-vs-test discriminator
  `:Item.is_test` already uses.
- Cypher-subset reachability: `MATCH (l:Literal) WHERE l.value =~ ...`
  with `file`/`crate`/`is_test` filters (no new Cypher construct —
  `:Literal` is just another node label the existing matcher handles).
- The lockstep `graph-specs-rust` `.cfdb/cross-fixture.toml` bump PR
  (RFC-033 §4 / Invariant I5) since `SchemaVersion` changes.

**Does not ship (see §6 Non-goals).**

## 3. Design

### 3.1 Node + schema

`cfdb-core/src/schema.rs` `Label` gains `Literal`. `cfdb-core/src/
fact.rs` `Node` already carries a generic attribute map (same
mechanism `:Item.kind`/`is_test` use); no struct change — `:Literal`
attributes ride the existing map:

| attr | type | notes |
|---|---|---|
| `value` | String | **raw inter-delimiter source bytes** (see "Value normalization" below), NOT Rust-decoded `LitStr::value()` |
| `file` | String | workspace-root-relative; same normalization as `:Item.file` |
| `line` | u32 | 1-indexed, start of the literal |
| `col` | u32 | 1-indexed |
| `crate` | String | owning crate |
| `is_test` | bool | true if inside `#[cfg(test)]`/`#[test]` — reuses the exact `:Item.is_test` predicate, not a reimplementation |

**Node ID:** `literal:<workspace-relative-file>:<line>:<col>`
(rust-systems lens, council 2026-05-15). `:Literal` has no owning
`:Item` in v0 (no `IN_ITEM` edge, §6), so the ID is derived purely
from position. Collision-free by Rust grammar: a `(file,line,col)`
admits exactly one literal start. Deterministic for a fixed input.

**Value normalization** (rust-systems lens — owning call, council
2026-05-15). `value` stores the **source bytes between the delimiting
quotes/pounds, WITHOUT Rust escape decoding** — NOT
`syn::LitStr::value()`. Rationale: the RFC's invariant is "a Cypher
`=~` matches what a developer would `grep` for in source."
`LitStr::value()` decodes `\t`→TAB, `\n`→LF, `\"`→`"`, `\u{..}`→char,
so `"phase\tname"` would store a literal TAB (10 bytes) while `grep`
sees `phase\tname` (backslash-t, 9 bytes) — the invariant would
break for any escaped literal. Storing raw inter-delimiter bytes
makes `\n`/`\t`/`\\` appear verbatim, so `=~ '\\n'` matches a source
`\n`. Raw strings (`r#"..."#`) store the inner bytes without the
`r`/`#` delimiters and (correctly) without escape expansion (raw
strings have none). Multiline literals store embedded newlines
verbatim — a documented edge: single-line-anchored Cypher `=~`
dialects will not span them; the downstream `arch-ban-phase-name-
strings` targets single-token words so this is immaterial to the
gating consumer. Implementation (slice 041-B) uses the literal
token's span source text (proc-macro2 span → source text is
deterministic for a fixed parsed file); the exact syn/proc-macro2
mechanism is the implementer's, the *contract* is "raw source bytes
between delimiters, no decode."

**No `kind` discriminator in v0** (ddd lens, council 2026-05-15).
v0 ships only string literals, so a `kind:"string"` attribute is
vacuous and, worse, a three-way homonym against `:Item.kind`
(declaration kind) and `:ConstTable.element_type` (RFC-040
deliberately avoided `kind` there). When non-string literals enter
scope (§6) the discriminator is introduced as `lit_syntax`
(`∈ {"str","bytes","numeric","char","bool"}`) — a name that does not
collide with `:Item.kind` — not `kind`.

No new `Edge` (v0 — ban rules `MATCH (l:Literal)` with attribute
filters; an `IN_ITEM` edge is a Non-goal, §6).

### 3.2 Extraction

In the existing `cfdb-extractor` `syn` visitor (the same pass that
already emits `:CallSite` per RFC-032's "out of scope unless needed"
carve-out): visit `syn::Lit::Str` (and `LitStr` inside `ExprLit`).
Skip literals inside `#[cfg(test)]` modules / `#[test]` fns via the
already-threaded test-context flag. Emit one `:Literal` Node with the
attributes above. Determinism: nodes emitted in source traversal
order, sorted by `(file, line, col)` before serialization — identical
discipline to `:CallSite`, preserving the sha256 byte-stable
re-extract invariant.

The `is_test` flag for a `:Literal` is **inherited from the enclosing
scope's `is_test` context exactly as `:CallSite` inherits it** — the
`bool` is passed down from the fn-body visitor's enclosing
`fn_is_test` result (the OR of `attrs_contain_cfg_test` /
`attrs_contain_hash_test` at `cfdb-extractor/src/attrs.rs:71,106` plus
the `is_in_test_mod` depth counter at `item_visitor/emit.rs:156`); it
is **not** re-evaluated at the literal AST node. There is exactly one
test-context resolver in the extractor (ddd lens verified, council
2026-05-15) and the literal visitor consumes it via the same
parameter-threading chain `:CallSite` uses, never a parallel path.

### 3.3 SchemaVersion + lockstep

`:Literal` is purely **additive** — no existing keyspace consumer
breaks (old queries never `MATCH (l:Literal)`; old keyspaces simply
lack the nodes). Per G4, bump the graph `SchemaVersion` minor. Per
CLAUDE.md §3 + RFC-033 §4 I5: the cfdb PR that bumps
`cfdb_core::SchemaVersion` MUST be accompanied by a draft
`yg/graph-specs-rust` PR bumping `.cfdb/cross-fixture.toml` to the
cfdb PR HEAD SHA; merge cfdb first, fixture bump within minutes;
graph-specs cross-dogfood may emit exit 20 in the window (documented).
The RFC's issue decomposition (§7) carries this as an explicit slice
with the runbook reference.

### 3.4 Cypher / CLI

No CLI verb, no new flag, no Cypher-subset construct. `:Literal` is a
node label the existing `MATCH (l:Literal) WHERE ... RETURN ...`
matcher handles for free (same path as `:CallSite`). The downstream
`arch-ban-phase-name-strings.cypher` (agentry#542) is the first
consumer; cfdb ships no rule itself.

## 4. Invariants

- **Determinism.** Two `cfdb extract` runs on an unchanged tree are
  sha256-identical. `:Literal` nodes sorted by `(file, line, col)`;
  no HashMap iteration order in the serialized output.
- **Recall.** The standing recall gate is extractor ≡ `cargo
  public-api`/rustdoc-json ground truth. **String literals are not in
  public-api or rustdoc-json** — there is no rustdoc ground truth for
  a literal expression. Therefore `:Literal` is **out of the recall
  corpus by construction**; its validation is (a) a self-dogfood
  assertion (cfdb extract on cfdb's own tree emits `:Literal` for a
  known-count set of fixture literals) and (b) a synthetic-workspace
  integration test asserting exact `(value,file,line,is_test)` tuples.
  This is the central design point for architect scrutiny — a new
  fact kind that the recall gate structurally cannot cover.
  **CLAUDE.md §5's "an RFC that adds a new fact type MUST extend the
  recall corpus before merge" clause is inapplicable to `:Literal` by
  construction** — no `rustdoc-json`/`cargo public-api` oracle exists
  for expression-leaf literals, so there is nothing to extend the
  recall corpus *with*. Slice 041-C's synthetic-workspace fixture is
  the ratified substitute and a future implementer MUST NOT treat the
  recall gate as a merge blocker for `:Literal` (clean-arch lens,
  council 2026-05-15; precedent: RFC-040 `:ConstTable`, likewise
  rustdoc-invisible). **The slice-041-C synthetic-workspace fixture
  is THE correctness gate for `:Literal` extraction** (exact
  `(value,file,line,col,is_test)` tuple assertions). The slice-041-B
  self-dogfood `count ≥ N` assertion is a **smoke test only** — it
  cannot catch an extraction bug that yields the right count via
  wrong literals; it must not be conflated with the correctness gate
  (rust-systems lens, council 2026-05-15).
- **No-ratchet.** No baseline/ceiling/allowlist file. The downstream
  ban rule lands zero-violation-or-fix-in-same-PR (agentry side).
- **Keyspace backward-compat.** Additive; old keyspaces/queries
  unaffected. `SchemaVersion` monotonic within major (G4).
- **is_test fidelity.** Reuses the existing `:Item.is_test`
  predicate; a divergent reimplementation is forbidden (homonym /
  split-brain risk — ddd lens).

## 5. Architect lenses

4-lens architect team (`rfc-041-literal-extraction-council`,
TeamCreate, 2026-05-15). Final: **4/4 RATIFY**.

### 5.1 Clean architecture (`clean-arch`) — **RATIFY**

No dependency-rule violation: `cfdb-extractor → cfdb-core` edge is
unchanged; `Label::LITERAL` is a `pub const &'static str` (precedent
`labels.rs:61` `CONST_TABLE`) touching zero imports.
`StoreBackend` (`store.rs`) gains no method — `:Literal` rides the
generic `Node` bag like `:CallSite`/`:ConstTable`. Composition root
(`cfdb-cli commands/extract.rs`) unchanged: `:Literal` arrives in the
existing `Vec<Node>`. Recall carve-out is a legitimate bounded
exception with RFC-040 `:ConstTable` precedent (also rustdoc-
invisible). Required clarification (recall-corpus extension
inapplicable by construction; 041-C is the substitute) — applied to
§4. Prescribed 041-A..E Tests rows.

### 5.2 Domain-driven design (`ddd-specialist`) — **RATIFY** (after 2 editorial rework)

`:Literal` is a coherent expression-occurrence node at the
`:CallSite` abstraction level. REQUEST CHANGES (both applied,
re-review confirmed): (1) `kind:"string"` was a three-way homonym vs
`:Item.kind` / `:ConstTable.element_type` → dropped; `lit_syntax`
reserved for future non-string kinds. (2) `is_test` single-resolver
verified in code — `attrs_contain_cfg_test`/`attrs_contain_hash_test`
(`attrs.rs:71,106`) + `is_in_test_mod` (`item_visitor/emit.rs:156`),
threaded as one `bool`; the §3.2 mandate (inherit, never re-evaluate
at the literal site, no parallel resolver) is enforceable, not
aspirational.

### 5.3 SOLID + component principles (`solid-architect`) — **RATIFY**

SRP holds: `:Item`/`:CallSite`/`:Literal` share one change-pressure
axis (syn API, `is_test` predicate, `Emitter` sink) → same component.
Correct granularity is a `literal_visitor.rs` submodule mirroring
`call_visitor.rs`, **not** a new crate (a `cfdb-literal-extractor`
crate fails CRP/REP — zero independent reusers). `Label` transparent
String newtype absorbs the blast across 8 `cfdb-core` dependents
(Ca=8, Ce=0, I=0) with zero source changes (no downstream exhaustive
match on label constants — evidence `cfdb-petgraph/src/graph.rs:305`,
`cfdb-query/src/list_items.rs:38`). SchemaVersion minor-additive is
G4-compliant (precedent: every prior label add). Prescribed
041-A..E Tests rows.

### 5.4 Rust systems (`rust-systems`) — **RATIFY** (after 5 precision rework)

Fundamental design sound (additive Label, existing `syn::Visit`
pass, `is_test` parameter-threading reuse, Vec+BTreeMap emission /
no HashMap, final-sort determinism). REQUEST CHANGES (all 5 applied,
re-review confirmed): (1) §6 exclude `cfg(feature=)` /
`#[serde(default=)]` strings (split-brain with `:Item.cfg_gate` /
`:CallSite`) + `macro_rules!` bodies (syn-opaque hard boundary);
keep `vec!`/`format!` expr-position literals (`call_visitor.rs:
134-164`). (2) `value` = raw inter-delimiter bytes, NOT
`LitStr::value()` (decoding breaks the `=~`-matches-grep invariant —
`"phase\tname"` 9-vs-10-byte). (3) node ID
`literal:<file>:<line>:<col>` (collision-free by grammar). (4) 041-C
synthetic fixture is THE correctness gate; 041-B self-dogfood count
is smoke-only. (5) §7 041-B/041-C carry the prescribed verbatim
4-row Tests blocks.

## 6. Non-goals

- Byte literals (`b"..."`), numeric, char, bool literals — separate
  future `lit_syntax` values under the same `:Literal` label if ever
  needed (NOT a `kind` attr — see §3.1 homonym rationale).
- Format-string interpolation arguments — the `"{}"` template is one
  literal; substituted runtime values are not source. Acceptable for
  the phase-name-strings use case.
- Attribute-embedded strings (`#[doc=...]`) — default OUT in v0.
- **`cfg(feature="...")` strings — EXCLUDED** (rust-systems lens
  ruling, council 2026-05-15). They are already extracted by
  `attrs.rs::extract_cfg_feature_gate` (`meta_to_feature_gate`,
  `cfdb-extractor/src/attrs.rs:230-238`) into the `:Item.cfg_gate`
  fact. Dual-emitting the same value as a `:Literal` is the exact
  parallel-extraction split-brain §4 forbids for `is_test`. The
  downstream `arch-ban-phase-name-strings` rule targets production
  fn-body literals in `crates/orchestrator-*`, not cfg attributes, so
  exclusion does not weaken it.
- **`#[serde(default = "...")]` strings — EXCLUDED** (rust-systems).
  Already modelled as `:CallSite` (the string is a fn path, not a
  value) via `attrs.rs::extract_serde_default_attr`. Dual-emission is
  a split-brain.
- **Literals inside `macro_rules!` declarative-macro bodies —
  UNREACHABLE, OUT** (rust-systems). `ItemMacro` token trees are
  opaque to `syn::visit::Visit`; this is a hard boundary, not a
  choice. (Expression-position literals inside *macro invocations*
  like `vec![..]` / `format!(..)` remain reachable via the existing
  `call_visitor.rs:134-164` re-parse and ARE in scope.)
- `include_str!`/`concat!` *output* — out (the path/piece literals
  are reachable as source; the macro-expanded result is not source
  text, so `value` is the source literal, never the expansion).
- An `(:Item)-[:CONTAINS_LITERAL]->(:Literal)` edge — `MATCH`+attr
  filter suffices for ban rules; edge is a future RFC if a consumer
  needs literal→owning-item joins.
- cfdb shipping any `.cypher` rule — the consumer (agentry#542) owns
  the rule.

## 7. Issue decomposition

Filed only after ratification (§2.4). Vertical slices, one issue
each, each carrying the verbatim §2.5 `Tests:` 4-row block (architects
fill during council).

- **041-A — `Label::Literal` + SchemaVersion minor bump (cfdb-core).**
  Schema vocabulary + monotonic version. `Tests:` — Unit: Label
  round-trip + SchemaVersion monotonic assert; Self dogfood: none —
  rationale: no extraction yet; Cross dogfood: graph-specs fixture
  bump is slice 041-D, not here; Target dogfood: none — rationale:
  schema-only.
- **041-B — extractor emits `:Literal` (cfdb-extractor).** New
  `literal_visitor.rs` submodule (solid lens — mirrors
  `call_visitor.rs`, NOT a new crate); raw-token-bytes `value`;
  `is_test` inherited via the existing threading; node ID
  `literal:<file>:<line>:<col>`; deterministic final sort.
  `Tests:` (rust-systems-prescribed, council 2026-05-15) —
  - Unit: pure `build_literal_node(lit, file, is_test) -> Node` —
    asserts Label="Literal", `value`=raw inter-delimiter bytes (NOT
    `LitStr::value()`), no `kind` attr, `is_test` propagated, id
    `literal:<file>:<line>:<col>`.
  - Self dogfood (cfdb on cfdb): `cfdb extract --workspace .` then
    `MATCH (l:Literal) RETURN count(l)` ≥ N (N = grep lower bound of
    string literals in cfdb `crates/*/src`, floor not ceiling — no
    ratchet); ≥1 `is_test=false` and ≥1 `is_test=true`. Smoke test
    only — NOT the correctness gate (§4).
  - Cross dogfood (graph-specs-rust @ pinned SHA): every existing
    `.cfdb/queries/*.cypher` returns zero rows — `:Literal` emission
    triggers no existing rule (regression guard).
  - Target dogfood (qbot-core @ pinned SHA): report `:Literal` count
    + top-10 values in PR body; informational, no gate.
- **041-C — synthetic-workspace integration fixture (THE correctness
  gate, §4).** `Tests:` (rust-systems-prescribed) —
  - Integration: synthetic Cargo workspace asserting the exact set of
    `(value,file,line,col,is_test)` tuples for: (a) plain
    `"verifying"` prod fn → is_test=false; (b) raw `r#"shipping"#` →
    value=`shipping`, is_test=false; (c) multiline `"line1\nline2"`
    → value has backslash-n verbatim, NOT a newline; (d) literal in
    `#[cfg(test)] mod` → is_test=true AND absent with is_test=false;
    (e) literal in `#[test] fn` → is_test=true; (f) literal in
    `const FOO: &str = "constant";` → is_test=false; (g) two literals
    on different lines/cols in one fn → both emitted, distinct
    coords. Plus determinism: two sequential extracts byte-identical
    (sha256 of serialized `:Literal` set).
  - Unit / Self / Cross / Target dogfood: none — rationale: this
    slice IS the integration fixture; self/cross/target are 041-B/D.
- **041-D — graph-specs-rust lockstep `.cfdb/cross-fixture.toml`
  bump.** Draft PR on the companion per RFC-033 §4 / docs/
  cross-fixture-bump.md; merge-order discipline. `Tests:` — Cross
  dogfood: the lockstep IS the test.
- **041-E — downstream enablement note.** Comment on `agentry#542`
  that cfdb supports `:Literal` once 041-A..D land + the
  `.cfdb/cfdb.rev` bump procedure for agentry. `Tests:` none —
  rationale: cross-repo coordination comment, not code.

## Refs

- `cfdb#367` (premature impl issue — superseded by this RFC)
- `agentry#542`, `agentry#496`, `agentry#497`, `agentry#397`
- `agentry:council/v2-finale-fsm-collapse/synthesis.md`
- RFC-033 §4 (cross-dogfood lockstep), RFC-037 (schema-producer alignment)
