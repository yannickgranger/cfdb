# RFC-038 — `:Context.source` discriminator (declared vs heuristic)

Status: **draft, R1 pending**
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
- **No `cfdb-petgraph::enrich_bounded_context` change.** That re-enrichment pass patches `:Item.bounded_context` (a string) when `.cfdb/concepts/*.toml` changes between extracts. It does NOT re-emit `:Context` nodes (per `crates/cfdb-petgraph/src/enrich/bounded_context.rs:18-19`). The new `:Context.source` prop lands at extract time only.

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

### 3.2 `BoundedContext` return type

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

This is enforced in the extractor's per-context accumulator: when a crate resolves to `BoundedContext { name, source: Declared }`, the accumulator marks the context as `Declared`; when a crate resolves to `Heuristic` and the context is not yet marked declared, the accumulator records `Heuristic`. Order of crate visitation does not affect the final source value. Determinism preserved.

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

### 5.1 R1 — pending

Four §2.3 lenses (clean-arch, ddd-specialist, solid-architect, rust-systems) reviewing this draft. Verdicts captured here on return.

### 5.2 R2+ — pending

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

Plumb the source signal from slice 2's `BoundedContext` through the per-context accumulator (§3.3) and into the prop map at `emit_context_node`. Adds the self-dogfood scar.

```
Tests:
  - Unit: per-context aggregation rule (§3.3) — declared+heuristic inputs combine to declared; heuristic+heuristic stays heuristic; visitation order independent.
  - Self dogfood (cfdb on cfdb): every :Context emitted on cfdb's own tree carries source ∈ {"declared","heuristic"}; cfdb's `.cfdb/concepts/*.toml` files determine the expected distribution (asserted explicitly).
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
