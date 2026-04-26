# RFC-039: Foreign-item stubs for cross-workspace edge endpoints

**Status:** RATIFIED (4/4 — clean-arch + solid-architect at R1; ddd-specialist + rust-systems at R2)
**Author:** yg
**Created:** 2026-04-26
**Ratified:** 2026-04-26
**Refs:** issue #306, RFC-037 §3.7 edge-liveness, RFC-cfdb §A.14 ItemKind, RFC-038 ContextSource precedent

## 1. Problem

Every `:Item -[E]-> :Item` edge that names a dst qname not defined in the
walked workspace is **silently dropped at petgraph ingest**
(`crates/cfdb-petgraph/src/graph.rs` ingest_one_edge, lines 215-225).

The extractor emits the edge correctly. The dst id is well-formed
(`item:<qname>`). But the petgraph mapping `id_to_idx.get(&edge.dst)`
returns `None` because no `:Item` node with that qname was emitted —
the trait/type lives in `std`, `serde`, `thiserror`, or any non-workspace
crate. Ingest pushes a warning and `return`s without keeping the edge.

Concrete impact, measured on cfdb-self at develop tip (commit `cde91b2`,
SchemaVersion v0.4.0, fresh extract on 2026-04-26):

| Edge label | Surviving count | Cause of drops |
|---|---|---|
| IMPLEMENTS | **0** | Almost every `impl Trait for T` implements a foreign trait (`Display`, `FromStr`, `Debug`, `Error`, `Serialize`) |
| IMPLEMENTS_FOR | 98 | Some drops on foreign target types (e.g. `impl X for Vec<T>`); workspace-local targets survive |
| RETURNS | partial | Drops when return type is a foreign type (`Result<T, E>`, `Vec<T>`, etc. depending on rendering) |
| TYPE_OF | partial | Drops when field/param type is foreign |
| HAS_PARAM | structural — `:Item -> :Param` — internal, not affected |

IMPLEMENTS is the loud case (zero survivors); the others are silent
because workspace-local edges drown out foreign-target drops in the
totals. RFC-037 §3.7 surfaced this as an edge-liveness gap on PR #305 CI.

The schema documents IMPLEMENTS as:

> "An impl Item implements a trait Item." (`crates/cfdb-core/src/schema/describe/edges.rs:58-63`)

The shape is `:Item -[IMPLEMENTS]-> :Item`. Today the dst :Item never
exists for foreign traits, so the edge is dead-on-arrival despite being
in the schema. Either the schema is wrong (foreign traits aren't real
graph nodes) or the extractor is incomplete (foreign traits should be
nodes). This RFC argues the latter.

## 2. Scope

**Ships:**
- New `external: bool` attribute on `:Item` — `true` iff the item is
  referenced from but not defined in the walked workspace.
- Extractor synthesis pass that emits `:Item{kind=<resolved>, external=true, name, qname, ...}`
  exactly once per foreign qname referenced by IMPLEMENTS, IMPLEMENTS_FOR,
  RETURNS, or TYPE_OF (deduplicated workspace-wide via a `BTreeSet<String>`).
- Schema describer update to document `:Item.external` and the
  edge-liveness expectation post-fix.
- Dogfood assertion that IMPLEMENTS edge count > 0 on cfdb-on-cfdb.

**Does not ship:**
- HIR-level type resolution for foreign types (their qname is whatever
  `resolve_target_qname` produces — heuristic, may be partially-qualified).
- Foreign-item attribute richness (visibility, deprecation, etc.) — only
  `kind`, `name`, `qname`, `external` are emitted.
- Reverse direction: inferring rust-stdlib trait shapes from rustdoc — out
  of scope, pure local synthesis from the extracted edge dst.
- Changes to `cfdb-recall` corpus to enumerate stdlib items — see §4.

## 3. Design

### 3.1. Considered alternatives

| Option | Mechanism | Trade-off | Verdict |
|---|---|---|---|
| **A — Allow-dangling at ingest** | Ingest creates a phantom graph node when dst is unknown, keyed by qname | Petgraph requires NodeIndex on both sides; phantoms with label=Unknown break label-indexed queries | **Rejected** — moves the synthesis to the wrong layer (cfdb-petgraph rather than cfdb-extractor) and produces under-typed nodes that label queries can't filter |
| **B — Extractor stubs (this RFC)** | Extractor emits `:Item{kind, external=true}` for foreign endpoints | Touches schema (one new bool attr); preserves graph invariants; reuses ItemKind | **Recommended** |
| **C — `:ExternalRef` separate label** | New `:ExternalRef` node label; IMPLEMENTS becomes polymorphic `:Item -> (:Item ∪ :ExternalRef)` | Schema vocabulary expansion; `to:` polymorphism; query authors must union | Rejected — heavier schema lift than B for the same semantic outcome |
| **D — Document as known limitation** | Update schema describer to say "IMPLEMENTS only carries workspace-local trait impls"; foreign traits visible only via `:Item.impl_trait` prop | Zero code change; but edge stays dead-on-arrival per RFC-037 §3.7; doesn't fix RETURNS/TYPE_OF | Rejected — accepts the bug rather than fixing it; downstream queries that walk IMPLEMENTS to a trait node continue to return empty |

The original issue body listed A/B/C/D verbatim; this RFC refines B
to use the existing council-ratified `ItemKind` (so foreign traits stay
`kind="trait"`, foreign structs stay `kind="struct"`, etc., with the
`external` flag as the cross-workspace discriminator) rather than
introducing a new `external_trait` `ItemKind` variant which would
require expanding the council enum (RATIFIED.md §A.14).

### 3.2. Type additions

**New type in cfdb-core** — `ItemSource` enum (mirrors RFC-038's
`ContextSource` shape; closed two-valued discriminator):

```rust
// crates/cfdb-core/src/item_source.rs (new file)
pub enum ItemSource {
    Workspace,  // Item defined in the walked workspace (default).
    External,   // Item referenced from but not defined in the workspace
                // (foreign trait, foreign type). Synthesized by
                // cfdb-extractor so edges to it survive ingest.
}

impl ItemSource {
    /// Closed-set wire convention (RFC-038 §3.1): returns &'static str.
    pub fn as_wire_str(&self) -> &'static str { ... }
}

impl FromStr for ItemSource { ... }

/// Reader helper — absent attr defaults to ItemSource::Workspace,
/// preserving backward-compat with pre-RFC-039 keyspaces. Mirrors
/// `cfdb_core::context_source::parse_or_default` from RFC-038.
pub fn parse_or_workspace(prop: Option<&PropValue>) -> ItemSource { ... }
```

`:Item` gains one optional attribute:

```
:Item.source — string enum (default "workspace", omitted when "workspace")
  type_hint: "string"
  description: "Provenance discriminator: `\"workspace\"` if defined
    in the walked workspace; `\"external\"` if referenced from but
    not defined in the workspace (synthesized stub for a foreign
    trait or type so edges survive ingest). Closed two-valued
    enum — see crates/cfdb-core/src/item_source.rs ItemSource.
    Mirrors :Context.source (RFC-038)."
  provenance: Provenance::Extractor
```

**Wire-format emit-when-non-default rule (§3.5).** The `source` prop
is inserted into the `Node.props` map ONLY when value is `External`.
Workspace items omit the key entirely — absence is the canonical
representation of `Workspace`. This matches RFC-038's emission
discipline and `cfdb-recall`'s expectations.

No new `Label` variants. No new `EdgeLabel` variants. No new
`Provenance` variant. No new `ItemKind` variant.

**Naming rationale** (resolves clean-arch R1 nit). `source` was
chosen over `external` / `out_of_workspace` for two reasons:
(1) parallels the RFC-038 `:Context.source` discriminator —
two RFCs in flight, both two-valued, both naming the prop `source`,
keeps query authors' mental model uniform; (2) `bool` discriminators
are below the floor RFC-038 set in §3.1 (closed enums use
`&'static str`, open sets use `String`) — using a typed enum lets a
future RFC extend the variant set (e.g. `Vendored`) without a
schema-breaking rename. The corresponding Rust type is named
`ItemSource` (not `ItemProvenance`) to avoid collision with the
existing `Provenance` enum at `crates/cfdb-core/src/schema/descriptors.rs`,
which already names *which extract pass wrote a value* — different
concept, same word would confuse query authors.

Externals are emitted as workspace `:Item` nodes with these guarantees:

- `id` = `item:<qname>` — same id formula as workspace items, so the
  edge dst already pointing to `item:std::fmt::Display` resolves to the
  synthesized stub. Built via `cfdb_core::qname::item_node_id` (the
  single canonical producer of the `item:` prefix).
- `kind` ∈ council-ratified `ItemKind` (no new variant) — best-effort
  inferred from the edge label (e.g. IMPLEMENTS dst is always
  `kind="trait"`; RETURNS/TYPE_OF dst is `kind="struct"` as a
  fallback when the kind is genuinely unknown).
- `name` = last `::`-separated segment of qname (`Display` for
  `std::fmt::Display`). Same rule as workspace items —
  disambiguation is via `qname` + `source`, not `name`.
- `qname` = the resolved qname string emitted by the extractor.
- `source` = `"external"` (emit-when-non-default rule).
- `crate` = first `::`-separated segment of qname (`std`, `serde`, ...)
  — provides the prop without needing to walk Cargo.toml.
- **No `bounded_context`** — foreign items belong to no crate in the
  walked workspace and `cfdb_concepts::compute_bounded_context` is
  driven by workspace crate membership. Queries that `GROUP BY
  bounded_context` continue to use RFC-038's absence-handling
  semantic; foreign stubs simply fall outside any context bucket.
- No `module`, `visibility`, `is_deprecated`, `file_path`,
  `span_line` — these are unknown for externals and are omitted
  (already optional per current emission).

### 3.3. Synthesis algorithm

**Emitter access prerequisite (resolves rust-systems R1 blocker).** The
synthesis pass iterates the emitted edge list to find foreign dst
qnames. `Emitter::edges` is currently a private field
(`crates/cfdb-extractor/src/emitter.rs:39`); the synthesis pass cannot
read it from `crates/cfdb-extractor/src/resolver.rs`. S2 MUST promote
the field to `pub(crate)` (matching the existing `pub(crate)`
visibility of `emitted_item_qnames`, `deferred_returns`,
`deferred_type_of` on the same struct) so the resolver module can
read it directly. This is the consistent pattern with how the existing
RETURNS / TYPE_OF resolvers access deferred queues — no accessor
method is added; the field-visibility promotion matches established
practice.

In `cfdb-extractor::extract_workspace`, after the per-file walk and
the existing post-walk RETURNS/TYPE_OF resolver passes (i.e. as
**Step 5**, between `resolve_deferred_type_of` and `Emitter::finish`):

1. Initialize `external_kinds: BTreeMap<String, &'static str>` empty.
   The map key is the foreign qname; the value is the resolved kind.
2. For every emitted edge in `Emitter::edges` (insertion order — G1
   deterministic):
   - Strip the `item:` prefix from `edge.dst`. (If the dst has a
     different prefix, skip — current extractor only emits
     `item:<qname>` for these labels.)
   - If `qname` is in `Emitter::emitted_item_qnames` → workspace-local,
     skip.
   - Else → look up the kind for `edge.label` from the priority table
     below; insert into `external_kinds` via the monotone aggregation
     rule (§3.3.1).
3. For every `(qname, kind)` pair in `external_kinds` (BTreeMap
   sorted iteration — G1 deterministic), emit one
   `:Item{source=External, kind=<resolved>, name, qname, crate}`
   node via `Emitter::emit_node`.

#### 3.3.1. Kind-priority table and monotone aggregation

The `(label → kind)` priority table is a pure map, defined as:

| Edge label | Priority | Resolved kind |
|---|---|---|
| `IMPLEMENTS` | 0 (highest) | `"trait"` |
| `IMPLEMENTS_FOR` | 1 | `"struct"` |
| `RETURNS` | 2 | `"struct"` (fallback) |
| `TYPE_OF` | 2 | `"struct"` (fallback) |
| (any other label whose dst is `item:<qname>`) | 99 | `"struct"` (fallback) |

Aggregation rule: when inserting `(qname, kind, priority)` into
`external_kinds`, keep the entry with the lowest priority seen.
This is monotone — final value depends only on the SET of priorities
seen for a qname, not on iteration order. A foreign qname referenced
by IMPLEMENTS (priority 0) wins kind `"trait"` regardless of how many
times it also appeared as RETURNS / TYPE_OF dst. Asserted by a unit
test (S2). Documented imprecision: a foreign qname referenced ONLY
by RETURNS or TYPE_OF that happens to be a stdlib trait (e.g.
`dyn Iterator` in a return type) gets `kind="struct"` (fallback) —
this is acknowledged in OQ2 and §6 non-goals; HIR-level kind
inference is a follow-up.

### 3.4. Schema describer update

`crates/cfdb-core/src/schema/describe/nodes.rs` — `:Item` descriptor
gains the `source` attribute (closed enum, see §3.2). The IMPLEMENTS
edge descriptor's description (`crates/cfdb-core/src/schema/describe/edges.rs:58-63`)
is updated to:

> "An impl Item implements a trait Item. Foreign traits are emitted as
> `:Item{source=\"external\"}` stubs (RFC-039) so the edge always resolves
> at ingest. Query authors filter on `t.source = \"workspace\"` to walk
> only into workspace-defined trait definitions; absence of the attr
> defaults to `\"workspace\"`."

The S1 schema_describe golden test asserts both the new `:Item.source`
attribute AND the updated IMPLEMENTS description text — single test,
two coupled assertions, both must pass.

`crates/cfdb-core/src/schema/descriptors.rs` — `Provenance::Extractor`
doc-comment is extended in S1 to clarify it covers synthesized stubs
whose source evidence (the impl-block trait qname, etc.) was itself
AST-walked. Suggested addition: "Includes synthesized stubs (RFC-039)
whose existence is inferred from edge dst qnames that the AST walker
already extracted — the stub's evidence is structural even if the
stub node itself has no single source-line definition."

No SchemaVersion bump — adding an optional string-enum attribute to an
existing node label is non-breaking per cfdb-core §5 schema discipline.

### 3.5. Determinism

G1 (canonical-dump byte stability) holds because:
- `external_qnames` is `BTreeSet<String>` → sorted iteration.
- Synthesized nodes get the same `item:<qname>` id formula → no
  collision with workspace items (we already check `emitted_item_qnames`
  before synthesizing).
- Emission happens at a deterministic point in `extract_workspace`
  (after the RETURNS/TYPE_OF resolver passes, before `Emitter::finish`).

CI check `ci/determinism-check.sh` is the enforcement.

## 4. Invariants

- **Determinism (G1):** Two extracts of an unchanged tree produce
  byte-identical `cfdb dump` output, including synthesized externals.
- **Recall (extractor ≡ rustdoc-json):** Synthesized externals are NOT
  in rustdoc ground truth (rustdoc only documents the workspace). The
  recall corpus comparison must filter `WHERE NOT item.external` when
  enumerating items, OR `cfdb-recall` adds a flag to skip externals.
  This is a non-breaking corpus extension — see slice S3 below.
- **Backward compat:** Pre-RFC-039 keyspaces have zero externals.
  Reading them with new code: `external` attr is absent → defaults to
  `false`. Reading new keyspaces with old code: extra `external`
  attribute is ignored (cfdb-core props are open-set on read).
  No SchemaVersion bump.
- **No-ratchet:** No metric thresholds change. Edge-liveness check goes
  from "IMPLEMENTS empty on cfdb-self" to "IMPLEMENTS > 0 on cfdb-self"
  — strictly improves, no allowlist file.
- **Cross-dogfood (RFC-033):** graph-specs-rust at pinned SHA must show
  zero rule findings post-RFC-039. The synthesized external nodes are
  new graph data; if any graph-specs ban rule matches against
  `:Item{kind="trait"}` or similar, externals could surface unexpected
  rows. The slice S2 PR runs cross-dogfood and any rule row blocks
  merge — exit 30 contract.

## 5. Architect lenses

### 5.0. R1 verdict summary (closed)

| Lens | R1 verdict | Status |
|---|---|---|
| Clean architecture (`clean-arch`) | RATIFY w/ 2 RFC-text amendments | addressed in R2 (§3.2 naming rationale, §3.4 Provenance::Extractor doc note, §7 S1 forward test) |
| Domain-driven design (`ddd-specialist`) | REQUEST CHANGES (1 blocking) | addressed in R2 (§3.2 typed enum `ItemSource`, §3.2 bounded_context note, §3.4 IMPLEMENTS description in golden, §7 S2 fallback test) |
| SOLID + components (`solid-architect`) | RATIFY w/ 2 advisory | addressed in R2 (§3.3 explicit Emitter access, §7 S2 split into 3 named tests) |
| Rust systems (`rust-systems`) | REQUEST CHANGES (2 blocking) | addressed in R2 (§3.3 Emitter access prerequisite, §3.2 emit-when-non-default rule, §8 OQ2 extended to IMPLEMENTS generic paths) |

R2 changes (this revision):
- `external: bool` → `source: ItemSource` typed enum (mirrors RFC-038
  `ContextSource`); enum lives in `cfdb-core/src/item_source.rs`;
  wire-form `"workspace"` / `"external"`; emit-when-non-default rule.
- Synthesis algorithm §3.3 now declares the Emitter field-visibility
  promotion explicitly (`pub(crate) edges`) and pins the kind-priority
  rule as a monotone aggregation table.
- §3.4 schema describer update now lists the IMPLEMENTS edge
  description rewording as a coupled assertion in S1's golden test.
- §3.2 explicitly notes foreign stubs carry no `bounded_context` and
  defers to RFC-038 absence-handling for queries that group by
  context.
- §7 S1 adds a forward dogfood test (fresh re-extract shows
  `:Item.source` in describer output); S2 splits the unit row into
  three named tests + adds the fallback-imprecision documentation
  test; updates Provenance::Extractor doc-comment as part of S1's
  scope.
- §8 OQ2 extended to cover IMPLEMENTS generic-parameterized trait
  paths, not only RETURNS/TYPE_OF.

### 5.1. Clean Architecture

*R1: RATIFY (with 2 RFC-text amendments — addressed in R2 above).*

Questions for this lens:
- Does extractor synthesizing nodes that aren't in source AST violate
  the extractor's "structural-only" charter (Provenance::Extractor)?
  Argument for: foreign-item presence IS observable in source — it's
  the dst of an `impl Trait for T` block we already walk.
- Does the synthesis pass belong inside `cfdb-extractor` or in a new
  `cfdb-foreign-stubs` crate sitting between extractor and petgraph?
  Argument for keeping it in `cfdb-extractor`: it's a post-walk
  resolution pass, exactly like the existing RETURNS/TYPE_OF resolvers
  in `crates/cfdb-extractor/src/resolver.rs`. Argument against: the
  synthesis is logically "fill in graph endpoints" which is closer to
  ingest-side concern.

### 5.2. Domain-Driven Design

*R1: REQUEST CHANGES (1 blocking — typed enum). R2: addressed.*

Questions for this lens:
- The `:Item` label is the domain root for "thing in the source code".
  Is `:Item{external=true}` a homonym, or a faithful extension of the
  concept? Argument for faithful: a foreign trait IS an Item, just one
  whose definition is outside our walk — we've already been emitting
  `:Item{kind="impl_block"}` for synthetic impl blocks.
- Bounded context: foreign items have no `bounded_context` attribute by
  design (we don't know it). Does this break queries that group by
  context? They already handle missing `bounded_context` (RFC-038).
- The `name` last-segment rule: is it correct that `Display` and
  `cfdb_cli::Display` (if such a workspace type existed) would have
  identical `name` but different `qname` + `external` flag? Yes —
  consistent with current behavior.

### 5.3. SOLID + Component Principles

*R1: RATIFY (with 2 advisory items — addressed in R2 above).*

Questions for this lens:
- Single Responsibility: the synthesis pass takes `Emitter` (read
  emitted edges, mutate emitted nodes). Same shape as the existing
  RETURNS/TYPE_OF resolvers — fits the established pattern.
- Open/Closed: adding the `external` attr extends `:Item` without
  changing existing call sites. Old consumers ignore the attr.
- Stable abstractions: `cfdb-core` is the most stable crate; this RFC
  adds one optional attr to its node descriptor and updates the
  IMPLEMENTS edge description. No instability introduced.
- Crate granularity: keep synthesis in `cfdb-extractor` (no new crate).

### 5.4. Rust Systems

*R1: REQUEST CHANGES (2 blocking — Emitter access + emit-when-non-default). R2: addressed.*

Questions for this lens:
- `BTreeSet<String>` per workspace: O(N log N) for N foreign-qname
  references. Empirically N is small (hundreds, not millions for
  cfdb-self). Acceptable.
- The kind-priority rule (`IMPLEMENTS > IMPLEMENTS_FOR > others`) is
  driven by edge label — needs to be a deterministic walk over
  emitted edges. Use a stable iteration order: walk `Emitter::edges`
  in insertion order (the order edges were emitted, which is itself
  deterministic per G1).
- Memory: synthesized nodes are small (~100 bytes each, ~hundreds of
  them) → negligible.
- Feature flags: not needed — synthesis is always on.
- `syn` dependency: not needed — synthesis works on already-extracted
  qname strings, no additional AST parsing.

## 6. Non-goals

- **Not in scope:** synthesizing foreign-item attribute richness
  (`visibility`, `is_deprecated`, `module`, `file_path`, `span_line`).
  These are unknown for externals; emitting placeholder values would
  pollute the graph and break downstream filters.
- **Not in scope:** HIR-resolved foreign-type kinds beyond the
  edge-label-driven heuristic. A future RFC could plug `cfdb-hir-extractor`
  in for foreign-type kind inference; this RFC produces structurally
  correct stubs without it.
- **Not in scope:** Reverse direction — extracting stdlib via rustdoc
  to populate genuine `:Item` nodes for `std::fmt::Display`. Pure local
  synthesis is enough for edge-liveness.
- **Not in scope:** changing the IMPLEMENTS edge description's
  `from`/`to` shape — both endpoints stay `:Item`.

## 7. Issue decomposition

Vertical slices, filed once this RFC is RATIFIED. Each slice carries
the prescribed `Tests:` block; architects fill the `Cross dogfood` row
in the council pass.

### S1 — `ItemSource` enum + `:Item.source` schema attribute (cfdb-core)

Adds the `ItemSource` enum + `parse_or_workspace` reader helper to
`cfdb-core`, the `source` attribute to the `:Item` node descriptor,
the IMPLEMENTS edge-description rewording, and the
`Provenance::Extractor` doc-comment clarification. Extends the
schema_describe golden test. No extractor emission yet — this is the
schema-and-types landing.

```
Tests:
  - Unit: (a) ItemSource::as_wire_str returns "workspace"/"external";
    FromStr round-trips both variants; FromStr rejects unknown with
    a typed error; parse_or_workspace returns Workspace on absent /
    None / non-string PropValue. (b) schema_describe() returns :Item
    with the `source` attribute (provenance=Extractor, type_hint=
    "string", description matches §3.2); attribute lands in canonical
    sort order; round-trip test_describe_round_trips_through_serde
    passes byte-stable. (c) IMPLEMENTS edge descriptor description
    matches the §3.4 rewording verbatim — coupled assertion in the
    same schema_describe golden so a partial S1 cannot regress one
    field while passing the other.
  - Self dogfood (cfdb on cfdb): a fresh re-extract on cfdb's tree
    + `cfdb describe --keyspace cfdb` shows :Item with the new
    `source` attribute in the rendered descriptor (forward test —
    confirms the schema landing surfaces in the CLI output even pre-
    S2 emission). Backward-compat read: opening the existing
    pre-S1 keyspace continues to succeed (absent `source` defaults
    to Workspace via parse_or_workspace).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): N/A —
    schema describe is not part of the ban-rules contract; only the
    schema_version bump (none in this slice) would matter.
  - Target dogfood: N/A — schema-and-types only.
```

### S2 — Extractor foreign-item synthesis (cfdb-extractor)

Promotes `Emitter::edges` to `pub(crate)` (per §3.3 prerequisite),
adds the synthesis pass as Step 5 of `extract_workspace`, and emits
foreign stubs. After this slice merges, IMPLEMENTS edge count on
cfdb-self goes from 0 to >0; same for the silent drops in
IMPLEMENTS_FOR / RETURNS / TYPE_OF.

```
Tests:
  - Unit (3 named tests, one per documented behavior — solid-architect R1):
    * synthesis_emits_one_stub_per_foreign_qname: fixture with
      `impl Display for LocalT` (Display foreign) emits exactly one
      :Item{source="external", kind="trait", qname="std::fmt::Display",
      name="Display", crate="std"}.
    * synthesis_deduplicates_repeated_foreign_qname: two impl blocks
      `impl Display for A` + `impl Display for B` synthesize ONE
      Display stub (BTreeMap dedup).
    * synthesis_kind_priority_implements_beats_implements_for: a
      qname referenced first as IMPLEMENTS_FOR dst (kind="struct"
      fallback) and later as IMPLEMENTS dst (kind="trait")
      resolves to kind="trait" (priority 0 wins over priority 1);
      reverse insertion order resolves identically (monotone aggregation).
  - Unit (1 named test — ddd-specialist R1 fallback documentation):
    * synthesis_returns_typeof_only_qname_falls_back_to_struct: a
      foreign qname referenced ONLY by RETURNS or TYPE_OF (no
      IMPLEMENTS / IMPLEMENTS_FOR) gets kind="struct" — documents
      the imprecision case (foreign qname might actually be a
      stdlib trait like `dyn Iterator`); HIR-resolved kind is
      deferred per §6 non-goals.
  - Unit (1 named test — emit-when-non-default rule, rust-systems R1):
    * synthesis_omits_source_prop_for_workspace_items: workspace
      :Item nodes do NOT carry a "source" prop in their props map
      (absence is the canonical Workspace representation); only
      synthesized externals carry source="external".
  - Self dogfood (cfdb on cfdb): IMPLEMENTS count >= 50 (rough lower
    bound — cfdb has dozens of `impl Display`, `impl FromStr`,
    `impl Error`, `impl Serialize`); count of `:Item{source="external"}`
    is > 0 and strictly less than total `:Item` count; every external
    Item has qname containing `::` (no synthesized externals for
    primitive types — those don't appear as edge dst); two consecutive
    `cfdb extract` runs on the unchanged tree produce byte-identical
    canonical dumps including all synthesized stubs (G1 determinism).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule
    findings — externals as new graph data must not match any
    existing ban rule. Exit 30 on any rule row blocks merge per
    RFC-033 §4 contract.
  - Target dogfood: report on PR body — IMPLEMENTS edge count and
    `:Item{source="external"}` node count for cfdb-on-qbot-core at
    qbot-core's pinned SHA, for reviewer sanity-check.
```

### S3 — Recall-corpus filter (cfdb-recall)

Updates `cfdb-recall` to skip `:Item{source="external"}` when comparing
against rustdoc ground truth. Without this, every synthesized external
becomes a recall false-positive ("rustdoc didn't see this item!").

```
Tests:
  - Unit (positive/negative pair, single test boundary):
    * recall_filters_external_items: corpus walker on a fixture with
      one synthesized external + one workspace item reports only the
      workspace item; the external is silently skipped.
    * recall_without_filter_surfaces_externals_as_false_positives:
      counter-test running the comparison WITHOUT the filter on the
      same fixture surfaces the external as a missing-from-rustdoc
      delta — pins the regression direction so a future refactor
      that drops the filter flag is caught.
  - Self dogfood (cfdb on cfdb): cfdb-recall on cfdb's own keyspace
    reports zero false-positives post-S2-merge (RFC-035 §G2 invariant
    holds — the filter restores recall parity that S2 would otherwise
    break).
  - Cross dogfood: N/A — cfdb-recall is not run on companion repos.
  - Target dogfood: N/A — recall is internal.
```

### S4 — Schema describer edge-liveness expectation

Updates RFC-037 §3.7 edge-liveness check to expect IMPLEMENTS > 0 on
cfdb-self post-RFC-039. Removes the temporary skip if any.

```
Tests:
  - Unit: edge-liveness assertion in CI updated.
  - Self dogfood: ci/edge-liveness-check.sh exits 0 with IMPLEMENTS > 0.
  - Cross dogfood: N/A.
  - Target dogfood: N/A.
```

## 8. Open questions

- **OQ1 — Should EQUIVALENT_TO (#307) folded into this RFC?** EQUIVALENT_TO
  is also dead-on-arrival, but its cause is different (no producer at
  all, vs IMPLEMENTS where producer exists but ingest drops). Suggest
  keeping #307 as a separate RFC.
- **OQ2 — Generic-parameterized qname normalization.** For RETURNS /
  TYPE_OF, when the edge points to `Result<T, E>`, `Vec<T>`,
  `Option<T>`, the rendered qname today may include the type
  parameters or not (depends on `render_type_inner`). For IMPLEMENTS,
  generic-parameterized trait paths (e.g. `Iterator<Item = Foo>`,
  `Iterator<Item = Bar>`) are passed through as written by source —
  two impls of `Iterator` over different associated-type bindings
  would synthesize two distinct foreign stubs for what is
  semantically the same foreign trait `Iterator` (rust-systems R1
  finding extended). The synthesized stub uses the qname as-is in
  S2; normalization (e.g. stripping generic instantiation parameters
  to keep only the base path) is a follow-up. Surface metric to
  watch post-S2: how many `:Item{source="external"}` nodes have
  `<` in their qname — if non-trivial, file the normalization issue.
- **OQ3 — Should `:Item.source` be promoted to a typed enum on the
  reader API?** Today `parse_or_workspace` returns `ItemSource`, but
  the prop is stored as `PropValue::String`. Consumers walking
  `Node.props` directly would see a string. RFC-038 has the same
  shape for `:Context.source` and exposes a typed reader; consider
  the same pattern for `:Item.source` once we see real consumer
  patterns post-S2.

## 9. Signals of success

Post-S2 merge:
- `IMPLEMENTS` edge count on cfdb-self goes from 0 to >50.
- RFC-037 §3.7 edge-liveness check on PR CI passes for IMPLEMENTS.
- `cfdb describe --keyspace cfdb` shows the `external` attr on `:Item`.
- A query like `MATCH (i:Item)-[:IMPLEMENTS]->(t:Item) WHERE t.qname = "std::fmt::Display" RETURN i.qname` returns the list of types in cfdb that implement `Display`.

## 10. Landing trail

| Slice | Issue | PR | Status |
|---|---|---|---|
| S1 — `ItemSource` enum + schema attr | [#311](https://agency.lab:3000/yg/cfdb/issues/311) | — | open, ready (no blockers) |
| S2 — extractor synthesis | [#312](https://agency.lab:3000/yg/cfdb/issues/312) | — | open, blocked by S1 |
| S3 — `cfdb-recall` filter | [#313](https://agency.lab:3000/yg/cfdb/issues/313) | — | open, blocked by S2 |
| S4 — edge-liveness expectation | [#314](https://agency.lab:3000/yg/cfdb/issues/314) | — | open, blocked by S2 |

R1 verdict tally (2026-04-26): 2 RATIFY (clean-arch, solid-architect)
+ 2 REQUEST CHANGES (ddd-specialist, rust-systems).
R2 verdict tally (2026-04-26): both REQUEST-CHANGES lenses ratified
their R1 findings as addressed → RATIFIED 4/4. R1 RATIFY verdicts
were not re-rendered (the design changes for R2 did not introduce
new concerns in their charters; clean-arch's `external` → `source`
naming concern was independently addressed by ddd's typed-enum
blocker).
