# RFC-038 — `:Context.source` discriminator (declared vs heuristic)

Status: **Ratified (R2, 2026-04-25)** — 4/4 RATIFY: clean-arch, ddd-specialist, solid-architect, rust-systems.
Parent trace: deep-audit EPIC #273 → Pattern 2 / cfdb-extractor F-013 → **this RFC**
Companion: closes the contract-drift item paired with F-005 (`span_line` real numbers — closed via PRs #291/#294).

---

## 1. Problem

`:Context` nodes carry no signal for **how** their bounded-context name was derived. Two distinct provenance paths produce indistinguishable nodes today:

1. **Declared:** the context name is named in a `.cfdb/concepts/<name>.toml` file. Author-asserted, high confidence, carries `canonical_crate` / `owning_rfc` metadata.
2. **Heuristic:** the context name is auto-derived by `cfdb_concepts::compute_bounded_context` stripping a well-known prefix (`domain-`, `ports-`, `adapters-`, `application-`, `use-cases-`) from the crate name, or returning the crate name unchanged. No author assertion, lower confidence, no metadata.

The discrimination already exists *internally* in `cfdb_concepts::compute_bounded_context` — it consults the override map first, falls back to the prefix heuristic. The discrimination is **discarded at the API boundary**: the function returns `String`, the caller (extractor) cannot distinguish the two paths.

Consumers of the fact graph cannot today answer "is `:Context{name='trading'}` a context the team explicitly declared in their concept TOML, or did cfdb just guess from a crate-name prefix?" Two consumer use cases concretely affected:

- **Skill-routing decisions.** A skill that triages ban-rule findings by bounded context is entitled to higher confidence on declared contexts than on heuristic ones (a heuristic context is a candidate, not an authority). Today the skill cannot make this distinction.
- **Audit / scope verbs.** `cfdb scope --context <name>` is currently identical regardless of provenance. A scope that intersects only declared contexts (e.g. "show me items in author-asserted contexts only") is unrepresentable.

Audit synthesis at v0.4.0 / SHA `eed55cd` flagged this as Pattern 2 (doc / contract drift, F-013). The discriminator is missing from the schema even though the deriving code already knows the answer.

---

## 2. Scope

### Deliverables

1. **`cfdb_core::ContextSource` enum** — two variants `Declared` and `Heuristic`, with `as_wire_str` / `Display` / `FromStr` (matching the `Visibility` enum pattern in `crates/cfdb-core/src/visibility.rs`). Lives in a new `crates/cfdb-core/src/context_source.rs` module, re-exported from `lib.rs`.
2. **API change in `cfdb_concepts::compute_bounded_context`** — return a `BoundedContext { name: String, source: ContextSource }` struct rather than bare `String`. The `name` field is the same string returned today; the `source` field carries the new signal.
3. **`:Context.source` attribute** — declared in `cfdb-core::schema::describe::nodes::context_node_descriptor` as `(name="source", ty="string", required, prov=Extractor)`. Wire values are exactly `ContextSource::as_wire_str` outputs (`"declared"`, `"heuristic"`).
4. **Extractor wiring** — `cfdb-extractor::lib.rs::emit_context_node` writes `source` to the prop map, sourced from the per-context provenance signal accumulated as crates are walked.
5. **Determinism + recall + dogfood test surface** per RFC-cfdb §2.5 / RFC-033 §3.5 four-row template (§7).

### Non-deliverables

- **No `SchemaVersion` bump.** This is an additive required attribute on an existing node label. v0.4.0 readers loading a keyspace that emits the prop see an extra unknown attribute and ignore it (per the existing forward-compat semantic). Pre-RFC-038 keyspaces loaded by post-RFC-038 readers carry no `source` prop on `:Context` nodes — readers MUST treat absence as `Heuristic` for backward compat OR re-extract the keyspace (recommended path; see §4).
- **No new node label, no new edge label.** Only an attribute on an existing node.
- **No migration of existing keyspaces.** Re-extract is the supported path. Both `cfdb-extractor` (syn) and `cfdb-hir-extractor` (HIR — currently does not emit `:Context`) are unaffected; only the extract-time emission path changes.
- **No graph-specs-rust cross-fixture lockstep PR.** The cross-dogfood ban rules in cfdb's `.cfdb/queries/*.cypher` do not reference `:Context.source`. Cross-fixture finding count is invariant under this change. Verified at PR time.
- **No `cfdb-petgraph::enrich_bounded_context` change.** That re-enrichment pass patches `:Item.bounded_context` (a string) when `.cfdb/concepts/*.toml` changes between extracts. It does NOT re-emit `:Context` nodes (per `crates/cfdb-petgraph/src/enrich/bounded_context.rs:18-19`). The new `:Context.source` prop lands at extract time only. The enrichment pass's `expected_for_crate` memo (`crates/cfdb-petgraph/src/enrich/bounded_context.rs:151`) only consumes `BoundedContext.name`, not `.source` — verified by R1 rust-systems migration count (post-R2 §3.2).

---

## 3. Design

### 3.1 `ContextSource` enum

```rust
// crates/cfdb-core/src/context_source.rs

use std::fmt;
use std::str::FromStr;

/// Provenance discriminator for `:Context` nodes (RFC-038).
///
/// `Declared` contexts are author-asserted in `.cfdb/concepts/<name>.toml`.
/// `Heuristic` contexts are auto-derived from crate-name prefix stripping
/// in `cfdb_concepts::compute_bounded_context`. Wire format is the
/// lower-case variant name; round-trips through `:Context.source` prop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextSource {
    Declared,
    Heuristic,
}

impl ContextSource {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            ContextSource::Declared => "declared",
            ContextSource::Heuristic => "heuristic",
        }
    }
}

impl fmt::Display for ContextSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

impl FromStr for ContextSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "declared" => Ok(ContextSource::Declared),
            "heuristic" => Ok(ContextSource::Heuristic),
            other => Err(format!("unrecognised context source: {other:?}")),
        }
    }
}
```

**Invariant-owner pattern (RFC-035 §3.3 precedent).** The wire-string round-trip is defined by `ContextSource` alone — no other crate is allowed to construct the wire string by hand. `audit-split-brain`'s FromStrBypass check is the existing enforcement.

**Return type of `as_wire_str` — `&'static str` (divergence from `Visibility::as_wire_str`).** R1 rust-systems flagged that `Visibility::as_wire_str` returns `String` (`crates/cfdb-core/src/visibility.rs:47`) while this RFC's `ContextSource::as_wire_str` returns `&'static str`. The divergence is **intentional**: `Visibility` carries a `Restricted(String)` variant whose wire form requires runtime allocation (`format!("pub(in {path})", ...)`). `ContextSource` has no such dynamic variant — the closed two-variant set maps to two `&'static str` literals. Returning `&'static str` is strictly more precise (no allocation, no lifetime concern) for this enum. The inconsistency with `Visibility` is a refactor opportunity for a future PR (Visibility's two static variants `Public` / `Private` could also be `&'static str`-routed with a small enum tweak), out of scope for RFC-038. Documented here so the precedent is clear: closed-set wire enums use `&'static str`; open-set wire enums (variants carrying owned data) use `String`.

### 3.2 `BoundedContext` return type and `cfdb-concepts → cfdb-core` dep arc

**New dependency arc (R1 B1 resolution).** `BoundedContext` lives in `cfdb-concepts` and carries `source: ContextSource` (which lives in `cfdb-core`). This requires adding a workspace-internal dep:

```toml
# crates/cfdb-concepts/Cargo.toml — [dependencies]
cfdb-core = { path = "../cfdb-core" }
```

`cfdb-concepts/Cargo.toml` today carries an explicit "zero heavy deps" comment (lines 10-22 of the current Cargo.toml). The intent of that comment is to keep `cfdb-concepts` out of the `ra-ap-*` / `cargo_metadata` / `syn` heavy-dep transitive closure so that `cfdb-query` (and other lightweight consumers) can depend on it without pulling in 1M+ LoC. **Adding `cfdb-core` does not violate that intent** — `cfdb-core`'s own deps are exactly `serde`, `serde_json`, `thiserror` (verified: `crates/cfdb-core/Cargo.toml`), all of which are already in `cfdb-concepts`'s direct dep list or are negligibly small. No HIR / no syn / no cargo_metadata transitively pulled in.

The arc direction is `cfdb-concepts → cfdb-core` (downward toward maximal stability, no cycle — `cfdb-core` has zero workspace-internal deps). The `Cargo.toml` comment in `cfdb-concepts` is updated to acknowledge `cfdb-core` as the schema-vocabulary-types provider, while preserving the "no heavy deps" intent. Slice 1's deliverable explicitly includes the Cargo.toml edit + comment update.

**Why not relocate `ContextSource` to `cfdb-concepts`?** R1 solid-architect offered an alternative: keep `ContextSource` inside `cfdb-concepts` (avoiding the new arc entirely). Rejected because the `:Item.visibility` precedent already places wire-vocabulary types in `cfdb-core` (`Visibility` enum). `cfdb-core` is the schema-vocabulary authority — every wire string consumed via `PropValue::Str` in the schema describer should round-trip through a typed enum in `cfdb-core`. Relocating `ContextSource` to `cfdb-concepts` would fragment that authority: future schema readers in `cfdb-petgraph` or `cfdb-query` that want to type-check `:Context.source` props would have to depend on `cfdb-concepts` (a higher-level crate) instead of `cfdb-core` (the schema authority). Keeping the precedent consistent is worth the new arc.

**Migration cost (R1 rust-systems audit).** Production callsites of `compute_bounded_context` that need migration: 2.

| Callsite | File | Reads `.source`? |
|---|---|---|
| `cfdb-extractor::emit_crate_and_walk_targets` | `crates/cfdb-extractor/src/lib.rs:190` | Yes (via the per-context accumulator, slice 3) |
| `cfdb-petgraph::enrich/bounded_context.rs::expected_for_crate` | `crates/cfdb-petgraph/src/enrich/bounded_context.rs:151` | No — only `.name` (BTreeMap value type unchanged from `String`) |

Test callsites in `crates/cfdb-concepts/src/lib.rs` (11 occurrences in `#[cfg(test)] mod tests`): all updated to assert on `.name`.

### 3.2.1 `BoundedContext` struct

```rust
// crates/cfdb-concepts/src/lib.rs

/// Bounded-context name with provenance discriminator (RFC-038).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedContext {
    pub name: String,
    pub source: ContextSource,
}

#[must_use]
pub fn compute_bounded_context(
    package_name: &str,
    overrides: &ConceptOverrides,
) -> BoundedContext {
    if let Some(meta) = overrides.lookup(package_name) {
        return BoundedContext {
            name: meta.name.clone(),
            source: ContextSource::Declared,
        };
    }
    for prefix in WELL_KNOWN_PREFIXES {
        if let Some(rest) = package_name.strip_prefix(prefix) {
            if !rest.is_empty() {
                return BoundedContext {
                    name: rest.to_string(),
                    source: ContextSource::Heuristic,
                };
            }
        }
    }
    BoundedContext {
        name: package_name.to_string(),
        source: ContextSource::Heuristic,
    }
}
```

The signature change forces every caller to handle `source` explicitly — there is no path where the discriminator can be silently dropped. This is the central ergonomic argument for changing the function shape rather than adding a parallel `compute_bounded_context_with_source` helper.

### 3.3 Per-context aggregation rule

A `:Context{name='trading'}` node is emitted once even if multiple crates resolve to it. Two crates can in principle reach the same context via different paths — one via override, one via heuristic. The aggregation rule is:

> **A context is `Declared` iff at least one crate resolved to it via an override.** Otherwise `Heuristic`.

Rationale: declarations are author intent; the heuristic is a fallback. If any author has named a context in TOML, that name is meaningful. Mixing one declared crate with N heuristic crates does not demote the context — it stays `Declared`.

**The aggregation rule is already implicitly implemented (R1 DDD discovery).** The extractor's `contexts_seen` accumulator at `crates/cfdb-extractor/src/lib.rs:116` is a `BTreeMap<String, ContextMeta>` pre-seeded with `overrides.declared_contexts()` BEFORE the per-crate loop runs. The per-crate loop at lines 196-203 uses `or_insert_with` — heuristic crates can ONLY insert when the context name is absent from the map; they CANNOT overwrite a pre-seeded declared entry. This means:

- A context whose name appears in any `.cfdb/concepts/*.toml` → in the map BEFORE the loop runs, with the full `ContextMeta` (canonical_crate, owning_rfc).
- A context whose name does NOT appear in TOML → inserted by the first heuristic crate to reach it, with default `ContextMeta`.

The aggregation rule "Declared if any crate via override" is therefore implicit in the pre-seeding: declared contexts are seeded as declared; heuristic contexts cannot promote themselves. Slice 3's job is **not** to re-implement this rule — it's to extend the accumulator's value type to carry the discriminator alongside `ContextMeta`, and propagate it to `:Context.source` at emission time.

**Updated accumulator type (slice 3).**

```rust
// Before (current code, crates/cfdb-extractor/src/lib.rs:116):
let mut contexts_seen: BTreeMap<String, ContextMeta> = ...;

// After (slice 3):
let mut contexts_seen: BTreeMap<String, (ContextMeta, ContextSource)> = ...;
```

The pre-seed at the same site changes from `(name, meta)` pairs to `(name, (meta, ContextSource::Declared))` for declared contexts. The per-crate `or_insert_with` arm changes from inserting `ContextMeta::default()` to inserting `(ContextMeta::default(), ContextSource::Heuristic)`. The emitter at `emit_context_node` (currently around line 260 of the same file) reads the second tuple element and writes `props["source"] = PropValue::Str(source.as_wire_str().to_string())`.

Order independence is preserved: BTreeMap iteration is sorted; the `or_insert_with` operation is order-independent (it inserts only when absent); pre-seeding happens before any per-crate insertion, so no race exists. G1 byte-stable canonical dump under cross-binary extract is verifiable.

### 3.4 Schema declaration

```rust
// crates/cfdb-core/src/schema/describe/nodes.rs::context_node_descriptor

attr(
    "source",
    "string",
    "Provenance discriminator: `\"declared\"` if the context name appears in `.cfdb/concepts/<name>.toml`; `\"heuristic\"` if the name was auto-derived by `cfdb_concepts::compute_bounded_context` via crate-name prefix stripping (RFC-038).",
    Extractor,
),
```

The attribute is **required** (`"string"`, not `"string?"`). Every `:Context` node emitted post-RFC-038 carries a value. Pre-RFC-038 keyspaces carry no `source` prop; consumers reading legacy keyspaces MUST tolerate absence — see §4.

### 3.5 Wire-format and SchemaVersion

- **Wire format change:** `:Context` nodes now carry one additional prop (`source`) with values `"declared"` or `"heuristic"`.
- **SchemaVersion bump:** **NO**. Per RFC-cfdb §2.5: additive non-breaking attribute additions MAY keep the version. The `source` prop is additive (no existing consumer reads it; absence on legacy keyspaces is tolerable per §4); no SchemaVersion bump is required.
- **`SchemaDescribe` output:** the new attr appears in `cfdb describe` output. Existing consumers parse describer output as a list — additional rows do not break parse.
- **Cross-repo lockstep:** none. `agency:yg/graph-specs-rust` does not reference `:Context.source` in its `.cfdb/cross-fixture.toml` rule set; cross-fixture finding count invariant under this change. Verified at PR time per RFC-033 §4.

---

## 4. Invariants

- **Determinism / G1 byte-stable.** Two extracts of the same tree produce byte-identical canonical dumps. The per-context aggregation (§3.3) is order-independent; visitation order does not affect the final `source` value.
- **Recall.** Every `:Context` node emitted post-RFC-038 carries `source`. A self-dogfood scar test asserts coverage = 100% of cfdb's own `:Context` nodes.
- **Backward-compat (legacy keyspaces).** Pre-RFC-038 keyspaces have no `source` prop on `:Context` nodes. Readers that consult the prop MUST treat absence as `ContextSource::Heuristic` (the conservative default — caller cannot prove declaration). Re-extract is the recommended path to upgrade. The legacy-load + missing-prop path is exercised by an integration test (§7 slice 2).
- **Forward-compat (future readers).** Adding `Restricted(...)` or any other variant is a future-RFC concern; v0.1 of this RFC ships exactly two variants. `FromStr` rejects unknown wire strings with a clear error rather than silently coercing.
- **Single resolution point.** Only `cfdb_concepts::compute_bounded_context` decides if a crate's bounded context is `Declared` or `Heuristic`. The extractor accumulates the per-context source signal but does not independently classify. `audit-split-brain` enforces no other code path constructs `ContextSource::Declared` directly outside `cfdb-concepts`.
- **Stable Abstractions Principle.** `cfdb-core` gains one new pure-data type (no I/O, no external deps). `cfdb-concepts` changes its public API. `cfdb-extractor` updates its `:Context` emission. No port surface affected; `StoreBackend` untouched.
- **No-ratchet rule (CLAUDE.md §3 / quality-architecture).** The two wire variants (`"declared"`, `"heuristic"`) are `const &str` in `ContextSource::as_wire_str`. Adding a third variant is a future-RFC change reviewed against the entire ecosystem of consumers.

---

## 5. Council review

### 5.1 R1 (2026-04-25) — REQUEST CHANGES

All four §2.3 lenses reviewed the R1 draft.

| Lens | Verdict | Primary concern |
|---|---|---|
| clean-arch | REQUEST CHANGES | New `cfdb-concepts → cfdb-core` dep arc unacknowledged; `:245` line ref wrong (should be `:190`) |
| ddd-specialist | RATIFY | None blocking — discovered the aggregation rule is already implicitly implemented via pre-seeding |
| solid-architect | RATIFY w/ B1 | Same dep-arc concern; offered alternative (relocate `ContextSource` to `cfdb-concepts`) |
| rust-systems | REQUEST CHANGES | Same dep-arc concern; flagged `as_wire_str` return-type inconsistency with `Visibility::as_wire_str` |

Two BLOCKING items identified, both addressed in this R2 draft:

| # | Item | R2 resolution |
|---|---|---|
| B1 | RFC silent on adding `cfdb-core = { path = "../cfdb-core" }` to `cfdb-concepts/Cargo.toml`; need explicit dep-arc justification | §3.2 — added Cargo.toml deliverable; rejected the relocation alternative; documented why the precedent (Visibility lives in cfdb-core) makes cfdb-core the right home |
| B2 | `ContextSource::as_wire_str -> &'static str` diverges from `Visibility::as_wire_str -> String` | §3.1 — divergence intentional and documented; closed-set vs open-set wire-enum convention captured |

Non-blocking items absorbed:

- clean-arch line-ref correction (`245` → `190`) — fixed in §2 Non-deliverables.
- clean-arch / DDD note on the `contexts_seen` accumulator type change — §3.3 now shows the explicit `BTreeMap<String, (ContextMeta, ContextSource)>` migration.
- DDD note on slice 3 mixed-crate unit test — added to §7 slice 3 prescription.
- DDD discovery: the §3.3 aggregation rule is already implicitly implemented via pre-seeding + `or_insert_with` — captured in §3.3 to simplify slice 3 implementation.

Detailed verdicts retained in the conversation transcript and on the council team's task list (`~/.claude/teams/rfc-038-council/`).

### 5.2 R2 (2026-04-25) — RATIFIED

All four §2.3 lenses RATIFY. Per CLAUDE.md §2.3 the RFC is **ratified**; no override recorded, no dissent.

| Lens | Verdict |
|---|---|
| clean-arch | RATIFY |
| ddd-specialist | RATIFY |
| solid-architect | RATIFY |
| rust-systems | RATIFY |

**NITs flagged for slice implementer attention** (non-blocking, resolve during slice work):

- **clean-arch NIT.** §3.3 says `emit_context_node` is "currently around line 260" — verify in slice 3 against the live `crates/cfdb-extractor/src/lib.rs` at slice-3-PR-time.
- **DDD NIT (carried from R1).** Slice 4's `parse_or_default(prop_value: Option<&PropValue>) -> ContextSource` helper — if it ever becomes public API, rename to `ContextSource::from_prop` to match the domain-vocabulary pattern used by `Visibility`.
- **SOLID NIT (carried from R2).** Open question Q1 ("any crate" vs "canonical_crate only") effectively answered by R1 DDD discovery: pre-seeding implementation already implements "any crate" semantic. The chosen rule is consistent with current code; Q1 is closed.

### 5.3 Post-ratification

Per CLAUDE.md §2.4, the §7 Issue decomposition is now the concrete backlog. Each slice is filed as a forge issue with `Refs: docs/RFC-038-context-source-discriminator.md` and the prescribed `Tests:` block, and worked via `/work-issue-lib`. Open questions Q1/Q2/Q3 in §8 are all resolved by council consensus or R2 absorption.

---

## 6. Non-goals

Restated from §2 for emphasis.

- No `SchemaVersion` bump (additive non-breaking attribute).
- No new node label, no new edge label.
- No migration of existing keyspaces (re-extract is the supported path).
- No graph-specs-rust cross-fixture lockstep PR.
- No `enrich_bounded_context` re-enrichment update — extract-time emission only.
- No third source variant (e.g. `Inferred`, `Imported`); two variants only in v0.1 of this RFC.
- No HIR-extractor change — `cfdb-hir-extractor` does not currently emit `:Context` nodes; if it ever does, it MUST source the discriminator the same way (single resolution point in `cfdb-concepts`).

---

## 7. Issue decomposition (post-ratification)

Vertical slices, each filed with `Refs: docs/RFC-038-context-source-discriminator.md` and a prescribed `Tests:` block per RFC-cfdb §2.5 / RFC-033 §3.5.

### Slice 1 — `cfdb_core::ContextSource` enum + schema attr declaration

Adds the pure type + the schema-describer entry. No behaviour change to runtime code yet (`cfdb-concepts` and `cfdb-extractor` continue to compile against pre-RFC-038 API). This slice unblocks slice 2 by giving it a type to import.

```
Tests:
  - Unit: ContextSource FromStr/Display/as_wire_str round-trip — every variant; reject unknown wire string with a clear error.
  - Self dogfood (cfdb on cfdb): N/A — no runtime behaviour change yet; describer output is exercised by existing schema-describe integration tests (which assert no missing attr declarations).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero finding delta; only describer output changes, no extracted facts change.
  - Target dogfood: N/A.
```

### Slice 2 — `cfdb_concepts::compute_bounded_context` returns `BoundedContext`

Change the public API to return the struct. Update every caller. This is the breaking-but-internal API change (no external consumers — `cfdb-concepts` is a workspace-internal crate). The extractor updates its callsite to read `.name` initially; the source signal is plumbed in slice 3.

```
Tests:
  - Unit: compute_bounded_context returns Declared on overridden inputs, Heuristic on prefix-stripped inputs, Heuristic on unmatched inputs.
  - Self dogfood (cfdb on cfdb): byte-identical canonical dump pre/post — only API shape changed, the emitted `:Item.bounded_context` string values are unchanged.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero finding delta.
  - Target dogfood: N/A.
```

### Slice 3 — extractor wires `source` into `:Context` emission

Plumb the source signal from slice 2's `BoundedContext` through the per-context accumulator (§3.3) and into the prop map at `emit_context_node` (`crates/cfdb-extractor/src/lib.rs`, currently around line 260). Adds the self-dogfood scar. The accumulator value type changes from `BTreeMap<String, ContextMeta>` to `BTreeMap<String, (ContextMeta, ContextSource)>` per §3.3.

```
Tests:
  - Unit: per-context aggregation rule (§3.3) — three explicit cases:
      * declared+heuristic mixed (one TOML-overridden crate + one prefix-heuristic crate map to same context name) → context source = Declared.
      * heuristic+heuristic (two prefix-heuristic crates map to same context name, neither in TOML) → context source = Heuristic.
      * declared+declared (two TOML-overridden crates map to same context name) → context source = Declared.
      * Visitation-order independence: shuffle crate visitation order across runs, assert identical (name, source) tuple set.
  - Self dogfood (cfdb on cfdb): every :Context emitted on cfdb's own tree carries source ∈ {"declared","heuristic"}; cfdb's `.cfdb/concepts/*.toml` files determine the expected distribution (asserted explicitly — e.g. `:Context{name="cfdb"}` is declared, `:Context{name="recall"}` should be declared or heuristic depending on TOML state at slice 3 ship time, asserted on the actual file set).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero finding delta — no rule references `:Context.source`.
  - Target dogfood (qbot-core at pinned SHA): report the count of declared vs heuristic :Context nodes in the PR body for reviewer sanity-check.
```

### Slice 4 — legacy keyspace tolerance + reader-side absence handling

Integration test asserting a pre-RFC-038 keyspace loads cleanly post-RFC-038, and any consumer reading `:Context.source` treats absence as `Heuristic` per §4. If any consumer is shipped that asserts presence, it carries a `// RFC-038: legacy keyspace tolerance` comment.

```
Tests:
  - Unit: ContextSource consumer-side helper `parse_or_default(prop_value: Option<&PropValue>) -> ContextSource` returns Heuristic on None.
  - Self dogfood: round-trip a fixture pre-RFC-038 keyspace through current loader — load succeeds, downstream rules that consult `:Context.source` see consistent Heuristic values.
  - Cross dogfood: N/A — graph-specs-rust does not consult the prop.
  - Target dogfood: N/A.
```

Slices land top-to-bottom in the merge order shown. Slice 1 unblocks slice 2 (type import). Slice 2 unblocks slice 3 (BoundedContext available at the extractor callsite). Slice 4 may land in parallel with slice 3 since it tests reader-side semantics that don't depend on slice 3's emitter changes.

---

## 8. Open questions (R1 draft)

- **Q1 — Aggregation rule edge case.** §3.3 specifies "Declared if at least one crate resolves via override". Alternative: "Declared if THE canonical_crate resolves via override." The RFC picks the simpler "any crate" rule. Council to confirm this is the right semantic. If the canonical-crate version is preferred, slice 3's per-context accumulator gains a check on `meta.canonical_crate`.
- **Q2 — `BoundedContext` vs tuple return type.** §3.2 picks `struct BoundedContext { name, source }`. Tuple `(String, ContextSource)` is shorter but less discoverable. The RFC picks struct for ergonomic clarity. Council to confirm.
- **Q3 — Reader-side default for absent source.** §4 picks `Heuristic` as the conservative default. Alternative: `Declared` (assume best). The RFC picks `Heuristic` (least confidence) to avoid promoting legacy ambiguity to declared status. Council to confirm.

---

## 9. Signals that RFC-038 has succeeded

- Every `:Context` node on every cfdb-extracted tree carries `source ∈ {"declared", "heuristic"}` post-slice-3.
- Self-dogfood scar test asserts 100% coverage on cfdb's own tree.
- `cfdb describe --format json` output shows the new attr in the `:Context` descriptor.
- A skill or scope query that filters `WHERE c.source = "declared"` returns the expected subset on a real keyspace.
- Cross dogfood on graph-specs-rust at pinned SHA: zero finding delta.
- Pre-RFC-038 keyspaces load cleanly under post-RFC-038 readers; absence-handling test passes.

---

## 10. Landing trail (post-ratification)

To be filled as slices merge.

| Slice | Issue | PR | Commit | Subject |
| --- | --- | --- | --- | --- |
| 1/4 | TBD | TBD | TBD | ContextSource enum + schema attr |
| 2/4 | TBD | TBD | TBD | compute_bounded_context returns BoundedContext |
| 3/4 | TBD | TBD | TBD | extractor wires :Context.source |
| 4/4 | TBD | TBD | TBD | legacy keyspace tolerance + reader helper |
