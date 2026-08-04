# RFC-056 — `DEPENDS_ON`: the crate-dependency edge

**Status:** RATIFIED — 2026-08-04 (architect review round 1 folded; operator-ratified).
**Raised by:** `agency:yg/agentry` #3034 (the engine-extraction arc's one
unmet definition-of-done bullet). Upstream design authority:
`agentry/docs/rfc/RFC-agent-execution-engine.md` §14 Amendment A1 §3
(operator-ruled 2026-07-25), which names the required cfdb capability;
this RFC transcribes it into cfdb's schema vocabulary, defines the value
vocabulary A1 §3 left open, and prescribes the tests.

## 1. Problem

agentry's engine extraction established a crate boundary
(`agentry-agent-engine` must never depend on `agentry-role-runtime`) whose
supplementary fence has **no instrument**. The residual vector the fence
exists to catch is the **dev-dependency cycle** — the one dependency shape
the compiler legally admits across that boundary — and cfdb, the instrument
of record, cannot express it: the extractor already parses every workspace
`Cargo.toml` and builds the intra-workspace dependency adjacency to compute
`:Crate.crate_tier`, then **collapses it to an integer and discards the
edges** — having first filtered to `kind == Normal`, which excludes the dev
table that is the fence's whole subject
(`crates/cfdb-extractor/src/crate_tier.rs`). The dependency data is parsed
and thrown away, not absent.

Consumers today have no graph-expressible way to ask "which crate depends
on which, by what dependency kind" — the question every crate-boundary
fence reduces to.

## 2. Scope

Ships:
- The `DEPENDS_ON` edge label in `cfdb-core`'s schema vocabulary: the
  `EdgeLabel` constant, its `SchemaDescribe` descriptor (label + both
  attributes), and the `DEPENDS_ON` bullet in
  `specs/concepts/cfdb-core.md`'s edge list (the precedent every prior
  edge-label addition followed).
- A `SchemaVersion` minor bump (`V0_9_0`) with the paired
  `graph-specs-rust` cross-fixture lockstep PR, per the unbroken V0_1_1 →
  V0_8_0 convention and the RFC-050 precedent (a strictly smaller schema
  surface, same data source, bumped `V0_6_0` and paired the lockstep).
  The lockstep pairs with the *implementing* PR — the one that lands the
  bump — merge order cfdb first per `docs/cross-fixture-bump.md`.
- Extractor emission of `DEPENDS_ON` edges from the already-parsed
  workspace manifests, **unfiltered by dependency kind**.
- The prescribed tests (§7, issue 056-A).

## 3. Design

- **Edge:** `(:Crate)-[:DEPENDS_ON]->(:Crate)` — source endpoint is the
  depending crate, target is the dependency.
- **`kind` property:** `normal` | `dev` | `build`, from
  `cargo_metadata::DependencyKind`. The set is closed: a dependency whose
  kind is not one of the three (the `#[non_exhaustive]` enum's `Unknown`
  arm — unreachable from real Cargo manifests today) is **not emitted**,
  and the unit suite names that arm explicitly.
- **`source` property — vocabulary defined here** (A1 §3 names the
  property but not its values): the **workspace-relative path of the
  manifest that declares the dependency** (e.g.
  `crates/cfdb-cli/Cargo.toml`) — the provenance a boundary-fence reader
  needs to locate the offending declaration. Always non-null for
  workspace-member edges. Deliberately NOT `cargo_metadata`'s
  `Dependency.source` (registry/git id — `None` for every path-declared
  intra-workspace dep, i.e. null on 100% of emitted edges). **Homonym
  callout:** distinct from `:Context.source` (`declared` | `heuristic`, a
  derivation discriminator on the overlay node) — same word, different
  subject; the `SchemaDescribe` attr text carries this distinction.
- **Membership:** intra-workspace only — both endpoints are workspace
  members, matching the extractor's existing `:Crate` node set; external
  crates never enter (the same membership rule the tier adjacency
  applies). Self-edges are dropped.
- **Cardinality:** one edge per `(source crate, target crate, kind)` — a
  dependency declared under two kinds yields two edges, one per kind.
- **Determinism:** emission derives from the same `BTreeMap`/`BTreeSet`
  walk the tier adjacency uses, AND — because `Edge::sort_key()` is
  `(src, dst, label)` and two `DEPENDS_ON` edges may differ only by
  `kind` — the per-pair kind emission order is itself fixed (a stable
  kind ordering), so byte-stability never rests on an unstable tie.
  `cfdb extract` stays byte-stable on an unchanged tree.

## 4. Invariants

- **I1 — `crate_tier` is untouched.** It continues to derive from the
  normal-kind subset, preserving its contract and the cycle rationale for
  that filter (RFC-050 §5). The `kind = "normal"` projection of the
  `DEPENDS_ON` edge set is definitionally equal to the tier adjacency.
- **I2 — Additive bump.** New edge label + two attributes:
  `SchemaVersion` bumps minor to `V0_9_0` with the paired
  graph-specs-rust lockstep (the convention every schema-vocabulary
  change since V0_1_1 has followed; CLAUDE.md §5's "MAY keep" permission
  has never been exercised and this RFC does not make it the first).
- **I3 — Determinism gate holds** (`ci/determinism-check.sh` — byte-stable
  across two extracts, including the same-`(src,dst,label)` kind-tie case
  named in §3).
- **I4 — No behavior change to any existing fact.** Existing node/edge
  emission is byte-identical; the change is purely additive.

## 5. Architect lenses

Round 1 (2026-08-04, read-only architect review against the tree):
design-fit CLEAN (generic `Edge.props` carries attributed edges today —
`IMPLEMENTS.resolver`, `CALLS.resolved` precedents; descriptor machinery
supports the new entry); extractor-fit CLEAN (the all-kinds adjacency is a
mechanical generalization of `normal_workspace_adjacency()` at the
existing `compute_crate_tiers` integration point). Corrections folded:
`source` vocabulary defined (was undefined, homonym-risky); `V0_9_0` +
lockstep restored (draft 1 wrongly claimed no-bump against unbroken
precedent); the self-dogfood example replaced (draft 1 cited a dev
back-edge that does not exist in the manifests — see §8); recall row
restored per the RFC-050 re-derivation pattern; structure aligned to
CLAUDE.md §2.2.

## 6. Non-goals

- **No ban rule ships here.** The boundary fence is the consumer repo's
  artifact (agentry lands it one-rule-per-PR with an observed unplanted
  red, per its A1 §5) — a fence shipped here would couple cfdb to one
  consumer's architecture.
- **No `crate_tier` change** (I1).
- **No import/type-path position modeling** (`:Use`/`:Path`) and no
  `src/bin` qname-collision fix — independent concerns; the dependency
  edge reads crate manifests, not call sites (A1 §3's recall caveat
  explicitly does not touch it).

## 7. Issue decomposition

### 056-A — `DEPENDS_ON` edge: schema vocabulary + extractor emission

One vertical slice: `EdgeLabel::DEPENDS_ON` + `SchemaDescribe` descriptor
(+ attrs with the `:Context.source` distinction in the attr text) +
`specs/concepts/cfdb-core.md` bullet + `V0_9_0` bump + all-kinds adjacency
emission + the boy-scout comment correction (§8). Paired lockstep
`graph-specs-rust` cross-fixture bump PR per CLAUDE.md §3.

```
Tests:
  - Unit: all-kinds workspace adjacency — kind partitioning
    (normal/dev/build, Unknown-arm excluded and named), self-edge drop,
    non-member drop, per-(source, target, kind) cardinality, stable
    kind ordering on a two-kind same-pair fixture.
  - Self dogfood (cfdb on cfdb): the real dev edge
    cfdb-hir-extractor -[DEPENDS_ON {kind:"dev"}]-> cfdb-extractor
    (declared in crates/cfdb-hir-extractor/Cargo.toml [dev-dependencies])
    is present with source naming that manifest; AND the kind="normal"
    projection of the DEPENDS_ON edge set equals the adjacency that
    computes crate_tier (I1).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule
    rows (no rule ships here; the gate proves the addition disturbs
    nothing), via the lockstep-bumped fixture.
  - Target dogfood (on qbot-core at pinned SHA): report the DEPENDS_ON
    edge count and per-kind histogram in the PR body for reviewer
    sanity-check.
  - Recall (per the RFC-050 pattern — rustdoc JSON carries no
    crate-dependency ground truth, so the manifest graph is the ground
    truth): an independent re-derivation of the all-kinds adjacency from
    the raw workspace manifests diffs empty against the emitted
    DEPENDS_ON edge set.
```

## 8. Corrected grounding — the false "dev back-edge" example

Draft 1 (and, independently, `crates/cfdb-extractor/src/crate_tier.rs`'s
module header, sourced from RFC-050 §3.2) cites
`cfdb-hir-extractor --dev--> cfdb-cli` as the tree's living dev back-edge.
**The manifests refute it**: `cfdb-hir-extractor`'s `[dev-dependencies]`
are `tempfile`, `cfdb-extractor` (path), `syn`, `quote`, `toml` — `cfdb-cli`
appears only in a comment; the full path-dependency enumeration finds no
all-kinds cycle anywhere in the current workspace. The real dev edge is
`cfdb-hir-extractor --dev--> cfdb-extractor` (no cycle: `cfdb-extractor`
declares no dependency back). The tier filter's rationale survives on the
general shape, not the false instance. 056-A carries the boy-scout
correction of the `crate_tier.rs` header; RFC-050's text stays as history
with this section as the correction of record.

## 9. Consumer note (non-normative)

After this ships, agentry authors its boundary fence as a normal
one-rule-per-PR ban over `(:Crate)-[:DEPENDS_ON]->(:Crate)` with zero
existing violations and an observed unplanted red first (its A1 §5), and
closes agentry #3034. The dev-kind match is that fence's primary catch.
