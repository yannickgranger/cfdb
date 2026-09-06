# RFC-043 — `:CallSite` argument facts for receiver-type fences

**Status:** Ratified
**Date:** 2026-05-26
**Companion:** agentry EPIC #793 (consumer-side ratified RFC at agentry:docs/rfc/RFC-verb-coverage-harvest.md, 2026-05-22). Sibling upstream graph-specs RFCs: graph-specs-005-verb-coverage-report (verb-coverage report), graph-specs-006-verb-anchoring (verb-anchoring).
**Consumer issue:** agentry tracking issue #1144; upstream request issue (yg/cfdb) #441.

## §1 — Problem

cfdb's `:CallSite` infrastructure (shipped #84/#85c/#86, v0.2+) emits `:CallSite` nodes plus `CALLS` and `INVOKES_AT` edges. These facts answer "what fn is called from where" — sufficient for entry-point catalogs and resolved-call graphs.

They are NOT sufficient for **receiver-type / value-origin fences** of the form "every call to `X::new` MUST receive an argument that is NOT a `.clone()` of variable `Y`." That class of fence is the agentry consumer's `INV-brief_state_stream-reaper-dedicated-connection` and the projector's parallel invariant: the reaper's `RedisInventory::new` MUST receive a *dedicated* `Conn`, not a clone of the dispatch loop's connection (#748/#776). Today this is enforced by a text-grep in `scripts/arch-check.sh` because cfdb cannot express it.

The agentry consumer RFC §8 names this as **B1 cross-dogfood**: cfdb `:CallSite` receiver-typed edges. The §8 text frames it as "needed only if the `DedicatedConn` newtype is rejected due to orphan-rule + caller-cascade caveat" — i.e., a fallback for the Tier-0 path that fails. But the broader argument-extraction capability is reusable for any future "what does this call receive" fence, so cfdb-043-call-site-arguments frames it as a general extension, not solely as the B1 fallback.

## §2 — Scope

In scope:

1. New `Label::ARGUMENT` and `EdgeLabel::HAS_ARG` string constants on the existing open-class newtype impls in `cfdb-core/src/schema/labels.rs` (cfdb uses `Label(pub String)` + `EdgeLabel(pub String)` newtypes, NOT `#[non_exhaustive]` enums — additive non-breaking by virtue of open-class string vocabulary, not enum-variant additivity). `(:CallSite)-[:HAS_ARG]->(:Argument {position})` connects each call site to its positional arguments (`position` is a node attribute on `:Argument`, NOT an edge property — preserves vocabulary consistency with `:Param.index` `nodes.rs:237`, `:Field.index` `nodes.rs:169`, `:Variant.index` `nodes.rs:211`).
2. Each `:Argument` carries:
   - `source_text: String` — verbatim source text of the argument expression (e.g., `"conn.clone()"`, `"&self.conn"`, `"42"`, `"RedisInventory::new(...)"`).
   - `kind: String` — coarse syntactic classification: `"path"` (identifier or path expression), `"method_call"` (e.g., `x.clone()`), `"call"` (free-fn invocation), `"ref"` (borrow expression), `"literal"`, `"other"`. Closed set; future variants additive. **Cypher ban-rules MUST NOT filter on `kind='other'`** ("other" is honest extractor-ignorance signaling, not a domain concept).
   - `position: u32` — zero-indexed positional location in the call expression's argument list. For `ExprMethodCall`, position 0 is the implicit `self` (the receiver); for `ExprCall`, position 0 is the first positional argument. The convention is exposed as `pub const RECEIVER_POSITION: u32 = 0;` in `cfdb-core` so cypher rule authors have a stable named reference.
   - `file: PathBuf` + `line: u32` + `col: u32` — source location of the argument expression.
3. Optional HIR-resolved type information (deferred to a future RFC; this RFC is **source-text only** to keep the syn-extractor compatible). A future RFC adds a `TYPE_OF` edge from `:Argument` to its resolved `:Item{kind:type}` when HIR resolution succeeds.
4. The syn-extractor (`cfdb-extractor`) is extended to emit `:Argument` nodes + `HAS_ARG` edges for every Rust call expression it visits. The HIR-extractor (`cfdb-hir-extractor`) inherits the same node shape (consistency across resolvers) but does not yet add type-resolved facts (deferred).
5. Schema bump: `cfdb-core::SchemaVersion` bumps from `V0_4_0` (current per `labels.rs:425`) to `V0_5_0` (new label class warrants minor bump per the precedent: V0_2_0 entry points, V0_3_0 schema-producer alignment, V0_4_0 literals). Consumers (graph-specs cross-dogfood pin, agentry `.cfdb/cfdb.rev`) bump in lockstep PRs per graph-specs-002-cross-dogfood#4.

Out of scope (§6 expands):

- HIR-resolved argument types — separate RFC adds `TYPE_OF` from `:Argument` to resolved `:Item{kind:type}`.
- Value-origin / data-flow tracking (e.g., "this `conn.clone()` cloned which variable?"). Requires HIR data-flow analysis. Future RFC.
- Trait-method-call argument extraction beyond what `syn::Expr::MethodCall` and `syn::Expr::Call` already produce. No new visitor patterns.
- Const / static / macro_rules! argument extraction. Macros produce post-expansion ASTs that the syn extractor already walks; macro-time arguments are not separately modeled.

## §3 — Design

### §3.1 — New schema vocabulary

`cfdb-core/src/schema/labels.rs` (verified at the path) gains new constants on the existing open-class newtype impls (NOT enum variants):

- `pub const ARGUMENT: &str = "Argument";` as a sibling constant in the existing `impl Label` (line 17-84 contains the existing constants `CALL_SITE`, `ENTRY_POINT`, `PARAM`, `LITERAL`, `CONST_TABLE`).
- `pub const HAS_ARG: &str = "HAS_ARG";` as a sibling constant in the existing `impl EdgeLabel` (line 101+ contains the existing edge-label constants).
- `pub const RECEIVER_POSITION: u32 = 0;` as a sibling constant in cfdb-core's schema module — the stable named reference for the method-call-receiver convention. Cypher rule authors reference this constant conceptually; future cypher tooling MAY codegen against it.
- Both labels gain descriptors in `cfdb-core/src/schema/describe/{nodes,edges}.rs` (existing pattern already routes both extractors through these single descriptor sources).

The `position: u32` attribute lives on `:Argument` itself, NOT on the `HAS_ARG` edge. This mirrors `:Param.index`, `:Field.index`, `:Variant.index` placement.

**Shared classifier helper:** because `syn` types cannot enter `cfdb-core` (cfdb-032-v02-extractor#3 boundary precedent for `ra_ap_*` applies equally to `syn`), the `kind` classifier helper lives in a NEW internal crate `cfdb-extractor-shared` that is depended on by both `cfdb-extractor` and `cfdb-hir-extractor`. Function signature: `pub fn classify_arg_kind(expr: &syn::Expr) -> &'static str` returns one of the closed-set strings `"path" | "method_call" | "call" | "ref" | "literal" | "other"`. Both extractors call this single function — no per-extractor classification logic.

**Node-ID formula:** `cfdb-core/src/qname/node_id.rs` gains `pub fn argument_node_id(callsite_id: &str, position: u32) -> String` returning `format!("arg:{callsite_id}#{position}")`. The callsite_id is already resolver-scoped (per the existing `cs_id` formula in `call_visitor.rs:175` and `call_site_emitter.rs:328`), so `:Argument` node IDs INHERIT the resolver scope. **Cross-extractor `:Argument` identity is NOT a Slice A design goal** — syn-emitted `:CallSite` and HIR-emitted `:CallSite` already have divergent IDs by design (per cfdb-032-v02-extractor#3 resolver-discriminator contract), and their `:Argument` children inherit that divergence. See Invariant 9 below.

### §3.2 — Syn extractor extension

`cfdb-extractor` (the syn-based extractor) gains a new visitor pass over `syn::Expr::Call` and `syn::Expr::MethodCall` expressions:

- For each `syn::ExprCall { func, args, .. }`: the call site's `:CallSite` is already emitted by the existing extractor; the new code emits one `:Argument` per `arg` in `args` (a `Punctuated<Expr, Token![,]>`), connected by `HAS_ARG{position}`.
- For each `syn::ExprMethodCall { receiver, args, .. }`: the receiver is treated as `position: 0` (the implicit `self`); each `arg` in `args` becomes `position: 1..N`.
- Argument node properties:
  - `source_text` = `arg.to_token_stream().to_string()` (proc-macro2's token-stream pretty-print; deterministic for a given AST).
  - `kind` = match-based classification on `syn::Expr` variant (`Path → "path"`, `MethodCall → "method_call"`, `Call → "call"`, `Reference → "ref"`, `Lit → "literal"`, `_ → "other"`).
  - `file` + `line` + `col` from `arg.span().start()` (via `proc-macro2`'s `Span::start()`; cfdb-extractor already enables `proc-macro2` `["span-locations"]` per the existing graph-specs adapter precedent).

### §3.3 — HIR extractor parity (no new HIR facts)

`cfdb-hir-extractor` (the HIR-backed extractor) emits the SAME `:Argument` schema for the call sites it resolves. **HIR-side emission algorithm:** the HIR extractor does NOT currently hold raw argument expressions at the `emit_resolved_call` site (`call_site_emitter.rs`); it receives `callee: Function` and `call_syntax: &SyntaxNode`. The new code:

1. Walks `ast::MethodCallExpr::arg_list()` or `ast::CallExpr::arg_list()` from the `call_syntax` `SyntaxNode` (both are `ra_ap_syntax::ast` AST nodes).
2. Iterates `ArgList::args()` to get each `ast::Expr` argument.
3. For each `arg`: extracts `arg.syntax().text_range()` for the source-text slice (using `db.file_text(file_id)`'s string content), and `LineIndex::line_col(offset)` (already imported in the HIR pipeline) for `line` + `col`.
4. Calls the shared `cfdb_extractor_shared::classify_arg_kind(arg)` helper — converting `ast::Expr` to `syn::Expr` via the existing `ra_ap_syntax → syn::parse_str` round-trip path the extractor already uses for other facts. (If the round-trip is judged too expensive at scale, Slice A may add a parallel HIR-native `classify_ast_arg_kind(ast: &ast::Expr) -> &'static str` in the shared crate; both must yield identical strings for identical expressions — verified by unit test on a shared fixture set.)

**Cross-resolver `kind` precedence rule:** the two extractors emit SEPARATE `:Argument` nodes per the resolver-discriminator contract (cfdb-032-v02-extractor#3). syn-emitted `:Argument` nodes attach to `:CallSite{resolver:"syn"}`; HIR-emitted `:Argument` nodes attach to `:CallSite{resolver:"hir"}`. They COEXIST in the graph, distinguished by parent. Cypher rules that need a single canonical classification MUST filter on the parent `:CallSite.resolver` value. There is no merge / overwrite; the resolver-discriminator pattern carries down to children. The classifier itself returns identical strings for identical AST shapes (the shared helper enforces this), so disagreement only arises when HIR resolves a path-expression to a method-call (the AST shapes differ); in that case, both classifications are correct for their respective resolver and the cypher author chooses which resolver to query.

No `TYPE_OF` edges from `:Argument` in this RFC — deferred to keep the syn/HIR contract symmetric. (Future RFC adds HIR-only `TYPE_OF { confidence: "resolved" }` edges; the syn extractor never emits `TYPE_OF` from `:Argument`.)

### §3.4 — Schema version bump + cross-dogfood

`cfdb-core::SchemaVersion` bumps from its current `V0_4_0` (verified `CURRENT = Self::V0_4_0` at `labels.rs:425`) to `V0_5_0` (new label class warrants minor bump per V0_2_0 / V0_3_0 / V0_4_0 precedent). Per graph-specs-002-cross-dogfood#4 cross-dogfood protocol: graph-specs-rust's `.cfdb/cross-fixture.toml` bumps to point at this RFC's merge SHA; agentry's `.cfdb/cfdb.rev` bumps in a coordinated downstream PR.

NDJSON / wire format: `:Argument` nodes serialize per the existing node-JSON shape — `{"label":"Argument","id":...,"props":{"source_text":...,"kind":...,"position":...,"file":...,"line":...,"col":...}}`. `HAS_ARG` edges serialize per the existing edge-JSON shape with NO `position` property (position is a node attribute on `:Argument`, NOT an edge property — mirrors `:Param.index` placement).

### §3.5 — Example cypher consumer (agentry use case demonstration)

After this RFC + graph-specs `.cfdb/cfdb.rev` bump lands, agentry can replace the text-grep fence in `scripts/arch-check.sh:80-107` (reaper dedicated-conn fence) with a `.cfdb/queries/arch-ban-reaper-clone-conn.cypher` rule:

```cypher
MATCH (cs:CallSite)-[r:INVOKES_AT]->(callee:Item)
WHERE callee.qname IN ['orchestrator_infra::RedisInventory::new', 'orchestrator_infra::RedisReaperSink::new']
MATCH (cs)-[:HAS_ARG]->(arg:Argument)
WHERE arg.position = 0
  AND arg.kind = 'method_call'
  AND arg.source_text MATCHES '.*\\.clone\\(\\)'
RETURN cs.id, callee.qname, arg.source_text, arg.file, arg.line
```

The rule fires when any call site to `RedisInventory::new` / `RedisReaperSink::new` passes a `.clone()`-receiver expression in argument position 0. The existing text-grep fence retires atomically when this cypher rule lands + `arch-check.sh` swaps the script step for the cypher rule.

## §4 — Invariants

1. **Schema additive.** `Label::ARGUMENT` and `EdgeLabel::HAS_ARG` are new string constants on the existing open-class newtype impls (`Label(pub String)` + `EdgeLabel(pub String)`) — non-breaking by virtue of open-class string vocabulary, not enum-variant additivity. Existing consumers ignore unknown labels per the standing cfdb-core schema-evolution rule.
2. **syn / HIR shape parity.** Both extractors emit `:Argument` + `HAS_ARG` with identical schema (`source_text`, `kind`, `file/line/col`, `position`). HIR is NOT yet permitted to emit additional `:Argument` properties this RFC doesn't define — that's a future RFC.
3. **No HIR `TYPE_OF` from `:Argument`** in Slice A. Adding `TYPE_OF` would force the syn extractor to emit incomplete facts (no types) which breaks the resolver-discriminator contract from cfdb-032-v02-extractor#3 (`resolver: "syn" | "hir"` on `:CallSite`). The HIR-only type resolution is a separate RFC.
4. **Argument enumeration is positional.** `:CallSite` → `HAS_ARG` → `:Argument {position}` preserves the source-order positions. Cypher rules match on `arg.position = 0` (the method-call receiver / first positional arg) deterministically. Use `cfdb_core::schema::labels::RECEIVER_POSITION` (conceptual reference) for the named convention.
5. **Argument source-text is verbatim.** `proc-macro2`'s token-stream pretty-print is deterministic per syn version. Cypher rules MAY match on `source_text` regex; consumers SHOULD prefer `kind` for coarse classification.
6. **Cross-dogfood pinning lockstep.** Per graph-specs-002-cross-dogfood#4: when this RFC's PR lands on cfdb develop, graph-specs `.cross-fixture.toml` and agentry `.cfdb/cfdb.rev` bump in coordinated PRs (no longer than 24h drift). Until both pins update, the SchemaVersion bump is the only signal to downstream consumers that the new vocabulary exists.
7. **Atomic Slice A.** New labels + new descriptors + new `cfdb-extractor-shared` crate + new `argument_node_id` helper + `RECEIVER_POSITION` constant + syn-extractor emission + HIR-extractor parity emission + the `cfdb-core::SchemaVersion` bump (V0_4_0 → V0_5_0) all land in the same PR. Splitting would either ship an unbumped schema with new facts (inconsistent serialization) or a bumped schema with no emission (broken downstream cypher rules). Mirrors graph-specs-006-verb-anchoring#3.3 atomicity rule.

8. **`:Argument` lifecycle (parallel to cfdb-036-cfdb-v2 CP4 for `:EntryPoint`).** `:Argument` lifecycle is coupled to its `:CallSite` owner by convention (cfdb's property-graph model does not enforce structural cascades). Re-extraction of a keyspace where the caller fn is deleted MUST remove the associated `:Argument` nodes and `HAS_ARG` edges in the same transaction. The shared `argument_node_id` formula (deriving from `callsite_id`) makes this constraint mechanically enforceable: deleting all nodes with id prefix `arg:{callsite_id}#` removes the children. Both extractors implement this in their delete-pass.

9. **`:Argument` node-ID resolver scope.** `:Argument` node IDs inherit the resolver scope of their parent `:CallSite`: syn and HIR emit DIFFERENT node IDs for the same source-position argument because their parent `:CallSite` IDs differ (per the existing cfdb-032-v02-extractor#3 resolver-discriminator contract). Cross-extractor `:Argument` identity is NOT a Slice A design goal. Consumers querying `:Argument` SHOULD filter on the parent `:CallSite.resolver` to disambiguate.

10. **`kind="other"` cypher prohibition.** `:Argument.kind = "other"` is honest extractor-ignorance signaling (an `syn::Expr` variant the classifier doesn't model). Cypher ban-rules MUST NOT use it as a normative fence filter. The `Label::ARGUMENT` descriptor description string carries this caveat.

## §5 — Architect lenses

### §5.1 — Clean architecture

### §5.2 — Domain-driven design

### §5.3 — SOLID + component principles

### §5.4 — Rust systems

## §6 — Non-goals

- HIR-resolved argument types via `TYPE_OF` edges from `:Argument`. Future RFC.
- Value-origin / data-flow analysis (e.g., "this `clone()` was on variable Y defined at file/line"). Future RFC; requires HIR + a sub-data-flow pass.
- Argument extraction from macro-input source text (pre-expansion). The syn extractor walks post-expansion AST; macro-time arguments are not modeled.
- Generic type-parameter extraction at call sites (e.g., `Vec::<u32>::new()`'s turbofish). Future RFC; out of scope for the immediate B1 use case.
- A separate `cfdb-arguments` crate. The new vocabulary lives in cfdb-core; emission lives in the existing extractor crates. **One new internal crate `cfdb-extractor-shared` IS added** to hold the `classify_arg_kind` pure helper — `syn` types cannot cross into `cfdb-core` per cfdb-032-v02-extractor#3 boundary; the shared crate is the composition root for syn-typed shared code between the two extractor crates.
- `source_text` as an optional / flag-gated attribute. If profiling on large workspaces reveals an extraction bottleneck, a future RFC adds `--no-arg-source-text` as a `cfdb-cli extract` flag (emit empty string or omit the property). Not in Slice A scope.
- Updating any downstream consumer (graph-specs, agentry) in this PR. Per graph-specs-002-cross-dogfood#4, lockstep PRs land within 24h; this RFC's PR is the upstream half.

## §7 — Issue decomposition

Two vertical slices.

### §7.1 — Slice A — schema vocabulary + syn-extractor emission + HIR-extractor parity + SchemaVersion bump

**Scope:** new `Label::ARGUMENT` + `EdgeLabel::HAS_ARG` string constants on the existing open-class newtype impls in cfdb-core + their descriptors in `cfdb-core/src/schema/describe/{nodes,edges}.rs`; `pub const RECEIVER_POSITION: u32 = 0;` in cfdb-core; `argument_node_id(callsite_id, position)` helper added to `cfdb-core/src/qname/node_id.rs`; **NEW internal crate `cfdb-extractor-shared`** holding the pure helper `pub fn classify_arg_kind(expr: &syn::Expr) -> &'static str` called by both extractors; syn-extractor visitor pass over `Expr::Call` + `Expr::MethodCall` emitting `:Argument` + `HAS_ARG` (using the shared classifier); HIR-extractor parity emission via `ast::ArgList` traversal + `LineIndex` (using the same shared classifier via the round-trip path documented in §3.3); `cfdb-core::SchemaVersion` bump from `V0_4_0` to `V0_5_0`; integration tests on synthetic fixtures.

**Tests** (round 1 prescriptions; round 2 may extend):
- Unit: pure-function assertions on the `kind` classification for each `syn::Expr` variant; node-ID derivation determinism.
- Self dogfood (cfdb on cfdb): cfdb extracts itself; the new `:Argument` count is non-zero and stable across runs.
- Cross dogfood (cfdb on graph-specs at pinned SHA): emission stable; downstream graph-specs can read the new schema version without errors.
- Behavioral: at least one fixture exercising the agentry-style use case (a `.clone()` argument to a `RedisInventory::new`-shaped fn) producing the expected `:Argument` with `kind: "method_call"` + `source_text` matching the `clone()` regex.

### §7.2 — Slice B — downstream lockstep coordination

**Scope:** documentation in `docs/cross-fixture-bump.md` describing the new schema entries; lockstep PRs on `yg/graph-specs-rust` (bumps `.cfdb/cross-fixture.toml`) and `yg/agentry` (bumps `.cfdb/cfdb.rev` + adds the example `arch-ban-reaper-clone-conn.cypher` rule + retires the corresponding `scripts/arch-check.sh:80-107` text-grep fence).

**Tests** (round 1 prescriptions):
- Cross-dogfood matrix green after both lockstep PRs land.
- agentry's new cypher rule fires on a synthetic violation (manually-introduced `.clone()` arg to `RedisInventory::new` in a test crate) and stays silent on the existing clean tree.

## §8 — Companion consumer

agentry's `INV-brief_state_stream-reaper-dedicated-connection` (currently enforced by `scripts/arch-check.sh:80-107` text-grep) and the projector's parallel invariant convert to cypher fences when this RFC lands + the agentry-side lockstep PR bumps `.cfdb/cfdb.rev` and adds the cypher rule. The text-grep fences in arch-check.sh retire atomically in the same agentry PR.

For the Tier-0 alternative (the `DedicatedConn` newtype path agentry RFC §8 also names): that path retires the text-grep fences differently (via Rust type system). Both paths are valid; choosing between them is operator/architect decision per the agentry-side conditional. cfdb-043-call-site-arguments ships the cypher path; the newtype path proceeds independently.

## §9 — Cross-references

- Consumer-side ratified RFC: `agentry:docs/rfc/RFC-verb-coverage-harvest.md` §8 (B1 cross-dogfood).
- Consumer EPIC: https://agency.lab:3000/yg/agentry/issues/793 .
- Consumer tracking issue (B1): https://agency.lab:3000/yg/agentry/issues/1144 .
- Upstream RFC request issue: https://agency.lab:3000/yg/cfdb/issues/441 .
- cfdb cfdb-032-v02-extractor#3 (HIR-extractor boundary): `docs/cfdb-032-v02-extractor-v02-extractor.md` — the resolver discriminator contract this RFC preserves.
- cfdb graph-specs-002-cross-dogfood#4 + cfdb-033-cross-dogfood (cross-dogfood with graph-specs): cross-fact locking + lockstep schema bump protocol.
- cfdb `specs/concepts/cfdb-core.md` (current schema vocabulary) + `cfdb-hir-extractor.md` (current `:CallSite` shape).
- cfdb `specs/concepts/cfdb-extractor.md` (syn extractor — currently emits `:Item`, `:Crate`, `:Module`, etc.; this RFC adds `:Argument` to its emission).
