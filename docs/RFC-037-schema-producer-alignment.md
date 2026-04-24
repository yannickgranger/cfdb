# RFC-037 — Schema-Producer Alignment

**Status:** Shipped (phase closed 2026-04-24 — see §9 Phase Shipped). Published in `cfdb-core::SchemaVersion::V0_3_0` via PRs #224 / #225 / #226 / #228 / #229 / #241 / #249.
**Draft history:** Draft v2 (revised 2026-04-23 per council verdicts in `council/RFC-037-VERDICTS.md`).
**Blocks (resolved):** resumption of #201 (036-1: REGISTERS_PARAM emission) — landed via #226.
**Sibling to:** RFC-036 (cfdb v2). This RFC closes the schema-vs-producer gaps surfaced during #201's `/discover` + `/prescribe` pass.
**Seed audit:** `.discovery/gap-audit-schema-vs-code.md`.
**Council verdicts:** `council/RFC-037-VERDICTS.md` (draft v1 REQUEST CHANGES; draft v2 addressed nine blocking findings B1-B9 and non-blocking N1-N6); closeout review in `council/RFC-037-CLOSEOUT.md`.

---

## 1. Problem

During `/prescribe` on #201 (REGISTERS_PARAM producer), three layers disagreed:

- **RFC-036 §3.1 CP1** asks that `REGISTERS_PARAM` "reuse the existing `:Param` nodes emitted by `HAS_PARAM`" for MCP tool fns, clap `#[arg]` struct fields, and clap `Subcommand` variants.
- **The schema descriptor** (`crates/cfdb-core/src/schema/describe/edges.rs:134-139`) declares `REGISTERS_PARAM` with `from: [:EntryPoint]`, `to: [:Param]` — one strict target type.
- **The extractor** emits `:Param` only from fn args (shipped in #209). Clap `#[arg]` struct fields emit `:Field`; `Subcommand` variants emit no variant-level node at all (`visit_item_enum` at `visits.rs:221-230` does not walk `node.variants`).

The disagreement is not unique to REGISTERS_PARAM. A systematic audit (`.discovery/gap-audit-schema-vs-code.md`) found **7 of 20 declared edges have zero producer** and **1 of 12 declared nodes (`:Variant`) is fully dormant**. Four of these dormants are load-bearing for RFC-036's in-flight downstream consumers (#201, #202, #204, #205).

**Why the gaps survived:** descriptor PRs ship on separate cycles from producer PRs; `graph-specs-rust` cross-dogfood ratifies descriptor existence but not producer fidelity; `cfdb-recall` (vs rustdoc-json) is blind to cfdb-specific vocabulary.

## 2. Scope

**In scope — ships:**

1. **REGISTERS_PARAM producer** (Option A — widen edge targets; §3.1).
2. **RETURNS producer** for syn-resolvable return types on fn items, via a **post-walk resolution pass** (§3.2).
3. **HAS_VARIANT + `:Variant` producer** for enum variants; extends `visit_item_enum` + adds tuple-field handling for both structs and variants (§3.3).
4. **TYPE_OF producer** for `:Field`, `:Param`, `:Variant` → `:Item` links, **via the same post-walk pass as RETURNS** (§3.4).
5. **`:Field` attribute alignment** — emitter ships the descriptor's 5 attributes; `type_qname` removed (§3.5).
6. **Vestigial deletions** — `SUPERTRAIT`, `RECEIVES_ARG` descriptors + constants removed; test-file updates enumerated (§3.6).
7. **Edge-liveness dogfood** — shell harness iterating edge labels with per-label `MATCH ... RETURN count(r)` queries (redesigned to respect the cfdb-query Cypher subset — §3.7).
8. **Canonical node-id helpers in `cfdb-core::qname`** — `field_node_id(parent_qname, field_name)` and `variant_node_id(enum_qname, index)`; both extractors route through them (§3.8).
9. **SchemaVersion bump** — `V0_3_0` (additive producers + breaking descriptor deletions + `:Field` attribute replacement; lockstep PR on graph-specs-rust per CLAUDE.md §3).

**Out of scope — deferred:**

- `IN_MODULE` producer — soft consumer only (#212 `cfdb diff --kinds Module`); file separately if #212 needs it.
- HIR-based `RETURNS` / `TYPE_OF` for cross-crate resolution — v2 is syn-level only; HIR refinement is a follow-up.
- `render_type_inner` / generic-unwrapping — current `type_render.rs:14-21` strips `Vec<T>`/`Option<T>`/`Result<T,E>` to the outer wrapper. TYPE_OF / RETURNS edges on wrapped same-crate types silently do not emit. Documented limitation in §6; follow-up RFC may refine.
- Nested `:EntryPoint{kind:cli_subcommand}` model for Subcommand enums — v0.3.0 uses the pragmatic compression "one REGISTERS_PARAM per variant" (§3.1); long-term model is a follow-up.
- Renaming or splitting `:Param` into `:FnParam` + `:EntryParam` (Option C from the audit). Rejected.
- Attribute-contract verification harness (mechanical descriptor↔emitter check) — systemic concern, future RFC round.

## 3. Design

### 3.1 — REGISTERS_PARAM target widening (Option A)

**Edge descriptor change** (`cfdb-core::schema::describe::edges`):

```rust
EdgeLabelDescriptor {
    label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
    description: "An EntryPoint declares an entry-point-exposed input — \
                  an MCP tool fn param (:Param), a clap `#[arg]` struct \
                  field (:Field), or a clap `Subcommand` variant (:Variant). \
                  Nodes on the target side keep their structural labels; \
                  this edge carries the semantic that the target is \
                  externally-facing.".into(),
    attributes: vec![],
    from: vec![Label::new(Label::ENTRY_POINT)],
    to: vec![
        Label::new(Label::PARAM),
        Label::new(Label::FIELD),
        Label::new(Label::VARIANT),
    ],
},
```

**Rationale for Option A over B/C/D** (from the audit): B muddles the structural reading of `:Param`; C requires a breaking schema split; D half-ships. A preserves single-structural-identity per node and moves the semantic overlay onto the edge, mirroring `LABELED_AS` at `edges.rs:141-147`.

**Producer rules — emitter crate ownership** (closes B9):

| REGISTERS_PARAM path | Emitter crate | Rationale | Target id formula |
|---|---|---|---|
| MCP `#[tool]` fn | `cfdb-extractor` (syn-side) | The syn walker already emits `:Param` via `emit::emit_param` at `emit.rs:289`; the `:EntryPoint` is syn-detected; no HIR crossing needed. | `param_node_id(parent_qname, index)` — `cfdb-core/src/qname.rs:87` |
| Clap `#[derive(Parser)]` struct | `cfdb-hir-extractor` (HIR-side) | `:EntryPoint` detection lives at `entry_point_emitter.rs:127-132`; walking the struct fields for `#[arg]` attrs piggybacks on the same HIR-side clap detection. | `field_node_id(struct_qname, field_name)` — §3.8 |
| Clap `#[derive(Subcommand)]` enum | `cfdb-hir-extractor` (HIR-side) | Same rationale; enum detection at `entry_point_emitter.rs:136-143`. | `variant_node_id(enum_qname, index)` — §3.8 |

**Subcommand: transitional approximation** (closes N1). The per-variant REGISTERS_PARAM is a transitional model. The long-term model is one `:EntryPoint{kind:cli_subcommand}` per variant with its own REGISTERS_PARAM targets from variant fields; that decomposition is a follow-up RFC tracking `cli_subcommand` kind.

### 3.2 — RETURNS producer (syn-level, post-walk resolution)

**Producer rule** (`cfdb-extractor::item_visitor`):

RETURNS cannot be emitted inline during the AST walk because a `fn use_foo() -> Foo` can precede `struct Foo {}` in source order. The walk is depth-first pre-order; the target `:Item`'s qname is not yet in any cache at emission time. The RFC therefore prescribes a **post-walk resolution pass**, paralleling the existing `pending_external_mods` deferral pattern on `ItemVisitor`.

**Implementation:**

1. Add `emitted_item_qnames: HashSet<String>` to `ItemVisitor` (populated inside `emit_item_with_flags` at `crates/cfdb-extractor/src/item_visitor/emit.rs:115` — every `:Item` emission inserts its qname).
2. Add `deferred_returns: Vec<(String, String)>` to `ItemVisitor` — tuples of `(fn_item_node_id, rendered_return_type_string)`.
3. During `visit_item_fn` + `visit_impl_item_fn`: inspect `sig.output`:
   - `syn::ReturnType::Default` → push nothing (unit return).
   - `syn::ReturnType::Type(_, ty)` → push `(item_node_id(fn_qname), render_type_string(&ty))` onto `deferred_returns`.
4. At walk completion (end of `extract_workspace` in `cfdb-extractor/src/lib.rs`, before the final sort): iterate `deferred_returns`; for each `(src, ty_string)`, if `emitted_item_qnames.contains(&ty_string)`, emit a `RETURNS` edge `src → item_node_id(ty_string)`.

**Limitation (closes B2 — documented).** `render_type_string` at `crates/cfdb-extractor/src/type_render.rs:11-58` strips generic arguments: `Vec<MyType>` renders as `"Vec"`. RETURNS on wrapper-wrapped same-crate types silently does not emit. This is the same-day behavior of the existing `emit_field` (which sets `type_qname = render_type_string(&ty)`), so no existing query degrades. A follow-up `render_type_inner` that unwraps `Vec<T>` → `T`, `Option<T>` → `T`, `Result<T,E>` → `T` is noted in §6 non-goals.

**Unresolved case:** if `ty_string` is not in `emitted_item_qnames` (cross-crate, generic-stripped-to-Vec, bare primitive, `impl Trait`), no edge is emitted. Parallel to the existing `INVOKES_AT` unresolved-target policy.

### 3.3 — HAS_VARIANT + `:Variant` producer

**Producer rule** — extend `visits::visit_item_enum`:

1. Emit the enum as `:Item{kind:enum}` (existing behavior at `visits.rs:221-230`).
2. For each variant at index `i`, emit a `:Variant` node with:
   - id: `variant_node_id(enum_qname, i)` (new helper, §3.8).
   - props: `{index, name, parent_qname, payload_kind}` — `payload_kind` is `"unit" | "tuple" | "struct"` matching `syn::Fields`.
3. Emit `HAS_VARIANT` edge from the enum's `:Item` to the variant's `:Variant`.
4. For `syn::Fields::Named` variants (record-style), walk the fields via the new `emit_field_list` helper (step 7 below) and emit `:Field` nodes with `parent_qname = variant_qname`.
5. For `syn::Fields::Unnamed` variants (tuple-style), emit indexed `:Field` nodes with names `_0`, `_1`, .... **Tuple-struct support is added in this same step** — `visit_item_struct` at `visits.rs:186-218` currently only handles `Fields::Named`; the `Fields::Unnamed` branch is new for both structs and variants, routed through the same `emit_field_list` (closes N4).
6. **Widen `HAS_FIELD` descriptor** at `crates/cfdb-core/src/schema/describe/edges.rs:25-30`:
   - `from: vec![Label::new(Label::ITEM), Label::new(Label::VARIANT)]`
   - `description: "A struct Item or enum Variant owns a Field.".into()` (closes B6).
7. **Extract `emit_field_list(src_id: &str, fields: &syn::Fields, parent_qname: &str)`** into `cfdb-extractor/src/item_visitor/emit.rs` (closes B4, B5):
   - Takes an explicit `src_id` so both `:Item{kind:struct}` and `:Variant` can own fields.
   - Iterates `Fields::Named` (record) and `Fields::Unnamed` (tuple) internally.
   - Emits the `:Field` node via `field_node_id` (§3.8).
   - Emits the `HAS_FIELD` edge `src_id → field_id`.
   - Both `visit_item_struct` and `visit_item_enum` call this single helper; the current inline named-field loop at `visits.rs:196-218` migrates to the new site.
8. **Update `:Field.parent_qname` attribute description** at `crates/cfdb-core/src/schema/describe/nodes.rs:174-175`:
   - `"Qualified name of the owning struct or enum variant."` (closes B6).

**Id-collision check:** `field_node_id(variant_qname, name)` produces `field:crate::Enum::Variant.name`; does not collide with struct fields or with existing `variant_node_id` ids.

### 3.4 — TYPE_OF producer (syn-level, post-walk resolution)

**Producer rule** — same post-walk pass as RETURNS:

1. Add `deferred_type_of: Vec<(String, String, &'static str)>` to `ItemVisitor` — tuples of `(source_node_id, rendered_type_string, source_label)` where `source_label ∈ {"Field", "Param", "Variant"}`. The label carries through so the edge reporter knows which source was deferred.
2. During emission in `emit_field` / `emit_param` (and variant payload-type inspection): if a field/param has a type that could resolve to an item, push the source id and `render_type_string(&ty)` onto `deferred_type_of`. (Skip deferring when the type renders to `"?"` or is obviously primitive; this is a cost-savings path, not correctness.)
3. At walk completion: iterate `deferred_type_of`; for each entry, if `emitted_item_qnames.contains(&ty_string)`, emit `TYPE_OF` from the source id to `item_node_id(ty_string)`.

**G1 determinism:** `deferred_*` vectors are walked in source-emission order (which is depth-first pre-order); resolution output is appended to `edges` and subsequently sorted by the existing canonical sort in `extract_workspace` before return. Determinism holds.

**Walk-order test requirement.** Issue D's acceptance criteria include a fixture with forward-declaration: `struct Foo(Bar);` referencing `struct Bar;` declared later. Assert `TYPE_OF` edge emits correctly.

### 3.5 — `:Field` attribute alignment

**Breaking change to producer output.** `emit_field` at `emit.rs:325-345` currently emits `{name, parent_qname, type_qname}` (3 props). After Issue E the emission set is `{index, name, parent_qname, type_normalized, type_path}` (5 props, matching the descriptor at `nodes.rs:154-192`).

**`emit_field_list` signature incorporates the change:** the helper accepts the field's `syn::Field` and `index: usize`; for named fields it uses `ident.to_string()` for `name`; for tuple fields it uses `_{index}`. Both `type_normalized` and `type_path` are currently the same value (both computed via `render_type_string(&f.ty)`); the split becomes meaningful once the `render_type_inner` follow-up (§6) ships.

**Downstream migration note** (closes N5): any Cypher query matching on `:Field.type_qname` returns null after v0.3.0. Grep recipe: `grep -rn 'type_qname' .cfdb/queries/ examples/queries/`. Verified in this repo: zero hits. Cross-dogfood companion PR must also pass this grep against graph-specs-rust fixtures.

### 3.6 — Vestigial deletions

Remove from `cfdb-core::schema::describe::edges`:
- `SUPERTRAIT` descriptor at `edges.rs:79-84`.
- `RECEIVES_ARG` descriptor at `edges.rs:114-124`.

Remove from `cfdb-core::schema::labels::EdgeLabel`:
- `SUPERTRAIT` constant at `labels.rs:91`.
- `RECEIVES_ARG` constant at `labels.rs:94`.

**Test-file updates required** (closes N3):
- `crates/cfdb-core/src/schema/describe/tests.rs:49,53` — remove `"SUPERTRAIT"` and `"RECEIVES_ARG"` from the expected edge label list in `schema_describe_covers_all_edge_labels`.
- `crates/cfdb-query/tests/predicate_schema_refs.rs:50,54` — remove `EdgeLabel::SUPERTRAIT` and `EdgeLabel::RECEIVES_ARG` from `KNOWN_EDGE_LABELS`.

**Schema-version impact:** deletion is breaking for any downstream reader referencing these labels by constant. Since no producer has ever emitted them, no keyspace on disk carries them; the break is purely API-surface. Bump v0.3.0 captures additions + deletions.

### 3.7 — Edge-liveness dogfood (redesigned for the cfdb Cypher subset)

Original draft used `CALL { ... }` correlated subqueries and `type(r)` — both rejected by the cfdb-query parser at `crates/cfdb-query/src/parser/mod.rs:247` (closes B3).

**Redesigned as a shell harness** `.cfdb/ci/edge-liveness.sh`:

```bash
#!/usr/bin/env bash
# Emits one row per edge label that has zero instances in the current keyspace.
# Ships as an informational check in v0.3.0; promoted to blocking in v0.4.0.
set -euo pipefail
DB="${CFDB_DB:-.cfdb/db}"
KS="${CFDB_KEYSPACE:-cfdb}"
CFDB="${CFDB_BIN:-./target/release/cfdb}"

EDGE_LABELS=(
  IN_CRATE IN_MODULE HAS_FIELD HAS_VARIANT HAS_PARAM
  TYPE_OF IMPLEMENTS IMPLEMENTS_FOR RETURNS
  BELONGS_TO CALLS INVOKES_AT
  EXPOSES REGISTERS_PARAM
  LABELED_AS CANONICAL_FOR EQUIVALENT_TO
  REFERENCED_BY
)
MISSING=()
for lbl in "${EDGE_LABELS[@]}"; do
  n=$("$CFDB" query --db "$DB" --keyspace "$KS" \
      "MATCH ()-[r:$lbl]->() RETURN count(r) AS n" \
      | awk 'NR==2 {print $1}')
  if [[ "$n" == "0" ]]; then MISSING+=("$lbl"); fi
done
if (( ${#MISSING[@]} > 0 )); then
  printf 'dormant edge labels (zero instances):\n'
  printf '  - %s\n' "${MISSING[@]}"
  exit 1
fi
```

**Policy:** ships informational (run but not CI-blocking) in v0.3.0. Promoted to blocking in v0.4.0 after one release cycle. Declaring an edge label without a producer becomes a CI violation at that point.

### 3.8 — Canonical node-id helpers in `cfdb-core::qname`

**New additions to `crates/cfdb-core/src/qname.rs`:**

```rust
/// Canonical node id for a `:Field`. Both extractors (syn-based today,
/// HIR-based tomorrow) route through this function so cross-extractor
/// `HAS_FIELD` / `REGISTERS_PARAM` edges target the same node id. Mirrors
/// the #209 resolution of the `:Param` id split-brain.
#[must_use]
pub fn field_node_id(parent_qname: &str, field_name: &str) -> String {
    format!("field:{parent_qname}.{field_name}")
}

/// Canonical node id for a `:Variant`. Index-based to mirror
/// `param_node_id` — positionally stable within a single extract; variant
/// reordering produces a new id (delete + recreate in diffs), which is
/// accepted per the same tradeoff as `param_node_id`.
#[must_use]
pub fn variant_node_id(enum_qname: &str, index: usize) -> String {
    format!("variant:{enum_qname}#{index}")
}
```

**Migration:** `emit::emit_field` at `emit.rs:326` changes from `format!("field:{parent_qname}.{name}")` to `field_node_id(parent_qname, name)`. The variant walker uses `variant_node_id(enum_qname, i)`. The clap-path REGISTERS_PARAM emitter in `cfdb-hir-extractor` uses both — this is the load-bearing seam that prevents a node-id split-brain.

**Closes B8.** Ships as Issue H before Issues B and C.

## 4. Invariants

**G1 (determinism).** All new emissions (including the post-walk RETURNS/TYPE_OF pass) feed into the existing canonical sort in `extract_workspace`. `ci/determinism-check.sh` continues to pass.

**G2 (recall).** `cfdb-recall` vs rustdoc-json unchanged — rustdoc does not model RETURNS, TYPE_OF, REGISTERS_PARAM, or `:Variant`, so these additions are orthogonal to recall.

**G3 (no ratchets).** No `.baseline.*` file, no allowlist, no ceiling. Edge-liveness check is const-thresholded at zero.

**G4 (backward-compat).** SchemaVersion V0_3_0 is a major bump (non-patch). Readers of v0.2.x keyspaces work on v0.2.x data; v0.3.0 data requires v0.3.0-aware decoders. The `can_read` contract at `cfdb-core/src/schema/labels.rs:311-314` handles this.

**G5 (cross-dogfood lockstep).** Paired PR on `yg/graph-specs-rust` bumps `.cfdb/cross-fixture.toml` to this RFC's head SHA; `specs/concepts/cfdb-core.md` and `specs/concepts/cfdb-extractor.md` amendments ship in the same PR (N2 + N6 enumerate sections).

**G6 (no breaking query changes without version guard).** Queries referencing `SUPERTRAIT` / `RECEIVES_ARG` / `:Field.type_qname` error at parse or return null after v0.3.0. Verified: zero hits in `.cfdb/queries/` and `examples/queries/`.

## 5. Architect lenses — ratified via council

The four-lens council (`council/RFC-037-VERDICTS.md`) ratified the design direction and returned REQUEST CHANGES on the implementation prescription with nine blocking findings (B1-B9). This revision (draft v2) addresses every blocking finding with the fix prescribed in the verdict document. Single-lens re-check sufficient if any author-initiated deviation from a prescribed fix.

- **clean-arch** — B7 (Issue D depends on Issue E), B8 (new Issue H), B9 (crate-ownership table). Ratified post-revision.
- **ddd** — B6 (HAS_FIELD + :Field.parent_qname description), N1 (Subcommand transitional note), N2 (spec amendments). Ratified post-revision.
- **solid** — B4 (emit_field src_id), B5 (emit_field_list extraction). Ratified post-revision.
- **rust-systems** — B1 (post-walk pass + known_qnames infrastructure), B2 (generic-stripping limitation), B3 (edge-liveness redesign), N3 (test-file enumeration), N4 (tuple-field ADD not REUSE). Ratified post-revision.

## 6. Non-goals

- Fixing attribute-contract drift systematically (descriptor↔emitter mechanical check). RFC-037 closes one such drift (`:Field`); systemic fix is a separate RFC round.
- Adding `:Statement`, `:Expression`, or finer-grained structural nodes. cfdb remains item-granular.
- HIR-based `RETURNS` / `TYPE_OF` for cross-crate resolution.
- **`render_type_inner` generic unwrapping** — current `type_render.rs:14-21` strips generics; `Vec<T>` → `"Vec"`, `Option<T>` → `"Option"`. TYPE_OF/RETURNS silently do not emit for wrapper-wrapped same-crate types. Follow-up RFC may refine. The HSB Jaccard signal (#204) has a recall loss proportional to wrapper frequency in target workspaces; acceptable for v0.3.0.
- **Nested `:EntryPoint{kind:cli_subcommand}` model for Subcommand enums** — long-term evolution of §3.1's pragmatic compression; follow-up RFC.
- Reviving `SUPERTRAIT` / `RECEIVES_ARG` — deleted, not reserved. Future RFC can re-introduce with a producer.
- `cfdb-recall` corpus extensions for new vocabulary — rustdoc-json lacks ground truth for it.

## 7. Issue decomposition

Vertical slices. Each `Tests:` block follows CLAUDE.md §2.5 RFC-033 template. Dependency chain: **H → A, E → B (depends on H) → C (depends on B) → D (depends on B and E) → F (depends on A-E) → G (depends on A-F)**.

### Issue H — Canonical `field_node_id` + `variant_node_id` in `cfdb-core::qname`

Scope: §3.8. Adds two `#[must_use]` pub fns; migrates `emit::emit_field` to route through `field_node_id`.

**Depends on:** none.
**Blocks:** B, C.

```
Tests:
  - Unit: field_node_id("crate::Foo", "bar") == "field:crate::Foo.bar"; variant_node_id("crate::E", 0) == "variant:crate::E#0"; round-trip via qname_from_node_id extension tests matching param_node_id's at crates/cfdb-core/src/qname.rs:476-483.
  - Self dogfood (cfdb on cfdb): extract with pre-change emitter and post-change emitter; sha256 of canonical dump byte-identical (formula unchanged, only its home moves).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows; no SchemaVersion bump in this issue.
  - Target dogfood: none — rationale: mechanical refactor; no observable graph change.
```

### Issue A — RETURNS producer (syn-level, post-walk)

Scope: §3.2. Adds `emitted_item_qnames: HashSet<String>` and `deferred_returns: Vec<(String, String)>` to `ItemVisitor`. Post-walk pass resolves deferred entries. Generic-stripping limitation documented.

**Depends on:** none.
**Blocks:** F.

```
Tests:
  - Unit: Fixture with `fn foo() -> Bar` where Bar is a walked :Item; assert RETURNS edge. Fixture with `fn use_foo() -> Foo` preceding `struct Foo {}` in source order; assert RETURNS still emits (post-walk resolution). Fixture with `fn baz() -> CrossCrateType`; assert no edge (unresolved). Fixture with `fn v() -> Vec<MyType>`; assert no edge (generic stripped — documented limitation).
  - Self dogfood (cfdb on cfdb): extract cfdb's own workspace; assert at least 50 RETURNS edges emitted (spot-check; accounts for generic-stripping loss). Run spec update to specs/concepts/cfdb-extractor.md documenting the new RETURNS emission behavior.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows; RETURNS edge count is non-zero.
  - Target dogfood (on qbot-core at pinned SHA): report RETURNS edge count + top-10 most-common return types in PR body for reviewer sanity-check.
```

### Issue B — HAS_VARIANT + `:Variant` producer + `emit_field_list` + descriptor updates

Scope: §3.3. Extends `visit_item_enum`; adds `emit_field_list` helper with explicit `src_id`; migrates `visit_item_struct` to use the helper; handles `Fields::Unnamed` for both structs and variants; widens `HAS_FIELD` descriptor `from:` list and description; updates `:Field.parent_qname` attribute description; routes through `variant_node_id` from Issue H.

**Depends on:** H.
**Blocks:** C, D, F.

```
Tests:
  - Unit: Fixture with one unit variant, one tuple variant, one struct variant. Assert :Variant node for each with correct payload_kind. Assert HAS_VARIANT edges. Assert :Field nodes for tuple + struct variant contents via the new emit_field_list. Assert HAS_FIELD src on variant-record-field edges is variant_node_id, NOT item_node_id. Assert tuple struct fixture (`struct Foo(i32, String)`) now emits :Field nodes (new behavior). Assert schema_describe_covers_all_edge_labels test passes with the widened HAS_FIELD descriptor.
  - Self dogfood (cfdb on cfdb): cfdb has many enums (StoreError, Label, EdgeLabel). Assert :Variant count equals hand-computed syn variant count. Assert tuple-struct emissions (if any) are present. Update specs/concepts/cfdb-extractor.md + specs/concepts/cfdb-core.md with new :Variant section + HAS_FIELD widening note.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows; :Variant emissions present.
  - Target dogfood (on qbot-core at pinned SHA): report :Variant count grouped by payload_kind + tuple-struct field count in PR body.
```

### Issue C — REGISTERS_PARAM producer (three paths)

Scope: §3.1. Widens edge descriptor `to:` list. Implements MCP (syn-side) + clap-struct (HIR-side) + Subcommand (HIR-side) emitters with the crate-ownership table from §3.1. Uses `field_node_id` / `variant_node_id` from Issue H. Adds § REGISTERS_PARAM + § RETURNS + § TYPE_OF + § Variant sections to specs/concepts/cfdb-core.md.

**Depends on:** B (for :Variant), H (for field_node_id + variant_node_id).
**Blocks:** F.

```
Tests:
  - Unit: MCP fixture — #[tool] fn with 3 args → 3 REGISTERS_PARAM edges to :Param nodes. Clap struct fixture — 4 #[arg] fields → 4 REGISTERS_PARAM edges to :Field nodes (target ids via field_node_id). Subcommand enum fixture — 3 variants → 3 REGISTERS_PARAM edges to :Variant nodes (target ids via variant_node_id). Assert no new :Param/Field/Variant nodes created — REGISTERS_PARAM reuses existing nodes.
  - Self dogfood (cfdb on cfdb): cfdb-cli uses #[derive(Parser)]. Assert every :EntryPoint{kind:cli_command} has REGISTERS_PARAM count equal to its handler struct's #[arg] field count. Update specs/concepts/cfdb-core.md per N2 (new sections REGISTERS_PARAM, RETURNS, TYPE_OF, Variant).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows.
  - Target dogfood (on qbot-core at pinned SHA): report :EntryPoint count by kind + total REGISTERS_PARAM edge count in PR body. Expected: non-zero MCP + non-zero CLI.
```

### Issue D — TYPE_OF producer (post-walk, shares infrastructure with RETURNS)

Scope: §3.4. Adds `deferred_type_of` vector + post-walk pass. Emits TYPE_OF for `:Field`/`:Param`/`:Variant` whose type resolves to a walked `:Item`. Reuses `emitted_item_qnames` from Issue A.

**Depends on:** B (for :Variant sources), E (for :Field.type_path prop).
**Blocks:** F.

```
Tests:
  - Unit: Fixture with `struct Foo { bar: Bar }` where Bar is a walked :Item; assert one TYPE_OF edge. Fixture with forward declaration (`struct Foo(Bar);` before `struct Bar;`); assert TYPE_OF emits correctly via post-walk pass. Fixture with `struct Baz { q: CrossCrateType }`; assert no TYPE_OF edge. Fixture with `struct V { v: Vec<MyType> }`; assert no edge (generic-stripping limitation — documented).
  - Self dogfood (cfdb on cfdb): TYPE_OF edge count ≥ 200 (cfdb has many typed fields; generic-stripping loss reduces from descriptor-implied upper bound). Update specs/concepts/cfdb-extractor.md with TYPE_OF emission behavior.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows.
  - Target dogfood (on qbot-core at pinned SHA): report TYPE_OF edge count + walk-order test result in PR body.
```

### Issue E — `:Field` attribute alignment

Scope: §3.5. Emits `{index, name, parent_qname, type_normalized, type_path}`; removes `type_qname`. `emit_field_list` signature carries `index` through.

**Depends on:** none (parallel with Issue A).
**Blocks:** D, F.

```
Tests:
  - Unit: Fixture struct with 3 fields; assert each :Field carries {index=0|1|2, name, parent_qname, type_normalized, type_path}. Assert no type_qname prop present. Assert schema_describe test's :Field attribute set matches.
  - Self dogfood (cfdb on cfdb): canonical-dump sha256 changes (attribute set changed). Run determinism-check twice at the new version. Grep `.cfdb/queries/` + `examples/queries/` for `type_qname`; assert zero hits.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows. Companion PR must grep its own fixtures and queries for `type_qname` and migrate (likely zero hits).
  - Target dogfood (on qbot-core at pinned SHA): report attribute-presence ratio in PR body. Flag any Cypher in the downstream consumer that reads type_qname.
```

### Issue F — Vestigial deletions + SchemaVersion v0.3.0 bump

Scope: §3.6. Delete `SUPERTRAIT` + `RECEIVES_ARG` from `labels.rs`, `edges.rs`, and the two test files `tests.rs:49,53` + `predicate_schema_refs.rs:50,54`. Bump `SchemaVersion::CURRENT` to `V0_3_0`.

**Depends on:** A, B, C, D, E.
**Blocks:** G.

```
Tests:
  - Unit: SchemaVersion::CURRENT == V0_3_0; schema_describe() output does not list SUPERTRAIT or RECEIVES_ARG. Both test files compile and pass.
  - Self dogfood (cfdb on cfdb): CI green with lockstep graph-specs-rust PR open.
  - Cross dogfood (cfdb on graph-specs-rust): exit 20 during lockstep window (documented); exit 0 after companion PR merges.
  - Target dogfood: none — rationale: version bump + deletion; no new runtime facts.
```

### Issue G — Edge-liveness informational check

Scope: §3.7. Ships `.cfdb/ci/edge-liveness.sh` (NOT a `.cypher` file). CI invokes it as informational in v0.3.0.

**Depends on:** A, B, C, D, E, F.

```
Tests:
  - Unit: Shell script parses on bash 5; iterates expected edge labels; handles zero-count output correctly. Runs against a fixture keyspace that exercises all edge labels; exits 0.
  - Self dogfood (cfdb on cfdb): script returns zero missing labels (otherwise something shipped broken — a failed dogfood is a real bug to fix).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero missing labels.
  - Target dogfood (on qbot-core at pinned SHA): report any missing labels in PR body. Likely zero after Issues A-F.
```

## 8. Ratification gate

This draft v2 integrates all council blocking findings. No re-council run is required if the author ships each fix per its prescribed shape (file:line traceability back to `council/RFC-037-VERDICTS.md` §2 B1-B9). Single-lens re-check sufficient for any deviation.

Issues are filed from §7 once this draft is committed. The edge-liveness shell harness (§3.7) is the mechanical detector that would have caught RFC-037's own motivating gap; informational in v0.3.0, blocking in v0.4.0. From RFC-038 onward the council's pre-check has teeth.

## 9. Phase Shipped (2026-04-24)

Closes #238. Records the disposition of RFC-037 as published in `cfdb-core::SchemaVersion::V0_3_0` and tagged at `v0.3.0` on `main`.

### 9.1 — Slices merged

| Slice | Issue | PR | Merge | Notes |
|---|---|---|---|---|
| H — qname canonical helpers | #215 | #224 | `27075ac` | `field_node_id` + `variant_node_id` in `cfdb-core::qname` |
| A — RETURNS producer (syn post-walk) | #216 | #224 | `27075ac` | Bundled with H/E into one squash |
| E — `:Field` attribute alignment | #217 | #224 | `27075ac` | `type_qname` removed; 5-attr emission |
| B — `:Variant` + HAS_VARIANT + `emit_field_list` | #218 | #225 | `a7f2644` | Tuple-struct + variant fields routed through shared helper |
| C — REGISTERS_PARAM 3-paths | #219 | #226 | `ccac5fb` | HIR-side emission (MCP + Parser + Subcommand) — see 9.4 |
| D — TYPE_OF producer | #220 | #226 | `ccac5fb` | Shares post-walk infra with RETURNS |
| I/F — vestigial deletions + SchemaVersion V0_3_0 | #221 | #228 | `4588fba` | `SUPERTRAIT` / `RECEIVES_ARG` removed; lockstep graph-specs-rust PR shipped |
| G — edge-liveness informational harness | #222 | #229 | `7ea7643` | `ci/edge-liveness.sh`; blocking in v0.4.0 |
| Follow-up — HIR `fn_name_and_qname` impl target | #227 | #241 | `769a0de` | Unblocks impl-method MCP REGISTERS_PARAM on `:EntryPoint` |
| Follow-up — `render_type_inner` generic unwrap | #239 | #249 | `bae7598` | §6 non-goal (1) promoted to follow-up and shipped |

Release tag `v0.3.0` captured via PRs #243 (release branch) / #244 (develop→main) / #245 (back-merge).

### 9.2 — Measured acceptance (phase-shipped snapshot, 2026-04-24)

All three dogfood paths exercise the shipped v0.3.0 binary (with `--features hir` for HIR-owned REGISTERS_PARAM).

**Self-dogfood — cfdb on cfdb @ `bae7598`** (proof: `.proofs/self-dogfood-238.txt`)

| Metric | Count | Threshold (issue body) | Result |
|---|---|---|---|
| RETURNS | 322 | ≥ 50 (#216) / ≥ 150 post-#239 | ✅ |
| TYPE_OF | 259 | ≥ 200 (#220) / ≥ 220 post-#239 | ✅ |
| REGISTERS_PARAM | 13 | non-zero + every `:EntryPoint{kind:cli_command}` has REGISTERS_PARAM count equal to its `#[arg]` field count (#219) | ✅ |
| HAS_VARIANT / `:Variant` | 179 / 179 | equals hand-computed syn variant count (#218) | ✅ |

**Cross-dogfood — cfdb on graph-specs-rust @ `913f06f`** (proof: `.proofs/cross-dogfood-238.txt`, harness: `ci/cross-dogfood.sh`)

| Metric | Result |
|---|---|
| Ban-rule violations (3 rules) | 0 — `arch-ban-f64-in-domain` / `arch-ban-reqwest-client-new` / `arch-ban-utc-now` |
| `ci/cross-dogfood.sh` exit code | 0 |
| Companion keyspace edges (RETURNS / TYPE_OF / HAS_VARIANT / REGISTERS_PARAM) | 27 / 55 / 30 / 0 (no MCP/Parser derives in graph-specs-rust — expected) |

**Target-dogfood — cfdb on qbot-core @ `6eb494e`** (proof: `.proofs/target-dogfood-238.txt`)

| Metric | Count | Notes |
|---|---|---|
| RETURNS | 5,125 | 4.21× the pre-`render_type_inner` baseline (1,216) |
| TYPE_OF | 2,950 | 1.33× the pre-`render_type_inner` baseline (2,218); wrapper-class boundary (HashMap / RwLock / Mutex not in closed 9-list) documented in #249 |
| REGISTERS_PARAM | 79 | Non-zero MCP + non-zero CLI; exact distribution in `.proofs/target-dogfood-238.txt` |
| HAS_VARIANT / `:Variant` | 2,259 / 2,259 | 102 `:EntryPoint` total |

**Edge-liveness (v0.3.0 informational)** — all four RFC-037 producers emit live edges on both self and target; `ci/edge-liveness.sh` dormant labels on self: `IN_MODULE` (scope-out per §2), `LABELED_AS` / `CANONICAL_FOR` / `EQUIVALENT_TO` / `REFERENCED_BY` (enrichment-path labels — populated by `cfdb classify` / `cfdb enrich`, not by `extract`). No RFC-037 producer is dormant.

### 9.3 — §6 non-goals disposition

Each §6 entry is either (a) filed as its own follow-up issue, or (b) retired with a one-line rationale.

| # | Non-goal | Disposition |
|---|---|---|
| 1 | `render_type_inner` generic unwrapping (`Vec<T>` / `Option<T>` / `Arc<T>` / `Result<T,E>`) | **SHIPPED** — filed as #239, merged via PR #249 at `bae7598`. Closed. |
| 2 | Systemic attribute-contract drift check (descriptor↔emitter mechanical validator) | **TRACKED** — filed as #250 (placeholder for the next anti-drift RFC round). The edge-liveness harness (G) already catches the zero-producer case; attribute-contract drift (wrong attrs on a live edge) is a distinct failure mode awaiting a systemic detector. |
| 3 | `:Statement` / `:Expression` / finer-grained structural nodes | **RETIRED** — cfdb's vocabulary stays item-granular by design (RFC-036 §1 / RFC-cfdb.md). Sub-item granularity is out of scope for the producer contract. |
| 4 | HIR-based RETURNS / TYPE_OF for cross-crate resolution | **RETIRED (reopen-able)** — v0.3.0 is syn-level only. Cross-crate precision is a follow-up if a downstream consumer measures unresolved-edge drop as load-bearing. No open issue; #204 (HSB cluster) is the most-likely trigger for the reopen. |
| 5 | Nested `:EntryPoint{kind:cli_subcommand}` model for Subcommand enums | **RETIRED (reopen-able)** — v0.3.0 uses the pragmatic "one REGISTERS_PARAM per variant" compression (§3.1 transitional note). Long-term model is a follow-up RFC once there is concrete query-side evidence the flattened model loses signal. No open issue. |
| 6 | Reviving `SUPERTRAIT` / `RECEIVES_ARG` | **RETIRED** — deleted, not reserved (I/F at PR #228). A future RFC can reintroduce with a concrete producer; no reservation cost carried. |
| 7 | `cfdb-recall` corpus extensions for new cfdb vocabulary | **RETIRED** — rustdoc-json has no ground truth for cfdb-specific labels (`:Variant`, `REGISTERS_PARAM`, etc.); `cfdb-recall` stays scoped to the rustdoc-aligned subset. Orthogonal to RFC-037's surface. |

### 9.4 — Design decisions ratified in flight (not in draft v2)

Two decisions emerged during execution that are captured here for future-RFC traceability:

- **REGISTERS_PARAM emission ownership moved from syn to HIR.** Draft v2 §3.1 prescribed per-path ownership (MCP → syn, clap-struct + Subcommand → HIR). During #219 integration, `cfdb-petgraph::graph::ingest_one_edge` (`src/graph.rs:204`) was found to drop dangling-src edges; `:EntryPoint` is an HIR-owned node, so syn-side REGISTERS_PARAM emission silently disappeared. All three paths were consolidated on the HIR side. Single-lens re-check per §8 happened inline in #226's PR body; no doc amendment issued at ship time.
- **Last-segment fallback in RETURNS / TYPE_OF resolution.** `render_type_string` produces source-as-written paths (`Foo` or `mymod::Bar`) while `emitted_item_qnames` holds crate-prefixed qnames. The shipped resolver uses exact-match + unique-last-segment fallback; ambiguous last-segments drop silently (parallel to the existing `INVOKES_AT` unresolved-target policy). Documented inline in the extractor source.

### 9.5 — Closeout review

Light 1-round review per the issue body — scope is "is the phase correctly dispositioned?", not re-litigation of the design (already ratified in `council/RFC-037-VERDICTS.md`). Four-lens verdicts recorded in `council/RFC-037-CLOSEOUT.md`; summary:

- **clean-arch** — RATIFY: shipped ownership matches prescribed layering once the HIR-emission adjustment (§9.4) is recorded.
- **ddd** — RATIFY: `:Variant` + `HAS_VARIANT` vocabulary holds; no homonym bleed into `:Item`.
- **solid** — RATIFY: `emit_field_list` extraction + canonical-id helpers closed B4/B5/B8 at ship.
- **rust-systems** — RATIFY: post-walk resolution + `render_type_inner` closed B1/B2 at ship; edge-liveness harness (G) is the mechanical detector that catches this class of drift going forward.

No deviations from the ratified design unresolved at close. RFC-037 transitions from "in-flight" to "shipped, closed" in the catalog.

---

**Author:** team-lead @ `a0-session:2026-04-23-201-paused-for-gap-audit`.
**Seed audit:** `.discovery/gap-audit-schema-vs-code.md`.
**Council verdicts:** `council/RFC-037-VERDICTS.md` (draft) + `council/RFC-037-CLOSEOUT.md` (closeout).
**Closeout session:** `a0-session:2026-04-24-rfc-037-closeout` (via #238).
