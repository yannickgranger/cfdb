# RFC-056 — `DEPENDS_ON`: the crate-dependency edge

**Status:** DRAFT — awaiting architect review.
**Raised by:** `agency:yg/agentry` #3034 (the engine-extraction arc's one
unmet definition-of-done bullet). Upstream design authority:
`agentry/docs/rfc/RFC-agent-execution-engine.md` §14 Amendment A1 §3
(operator-ruled 2026-07-25), which names the required cfdb capability
precisely; this RFC transcribes it into cfdb's schema vocabulary and
prescribes its tests.

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
- The `DEPENDS_ON` edge label in `cfdb-core`'s schema vocabulary, with its
  `SchemaDescribe` row.
- Extractor emission of `DEPENDS_ON` edges from the already-parsed
  workspace manifests, **unfiltered by dependency kind**.
- The prescribed tests (§5).

Does NOT ship:
- Any ban rule. The boundary fence is the consumer repo's artifact
  (agentry lands it one-rule-per-PR with an observed unplanted red, per its
  A1 §5) — a fence shipped here would couple cfdb to one consumer's
  architecture.
- Any change to `crate_tier` (§4, I1).
- Import/type-path position modeling (`:Use`/`:Path`) or the `src/bin`
  qname-collision fix — independent concerns; the dependency edge reads
  crate manifests, not call sites (A1 §3's recall caveat explicitly does
  not touch it).

## 3. Design

- **Edge:** `(:Crate)-[:DEPENDS_ON]->(:Crate)` — source is the depending
  crate, target is the dependency.
- **Properties:** `kind` ∈ `normal` | `dev` | `build` (from
  `cargo_metadata`'s `DependencyKind`), and `source` (the manifest
  provenance attribute A1 §3 names).
- **Membership:** intra-workspace only — both endpoints are workspace
  members, matching the extractor's existing `:Crate` node set; external
  crates never enter (the same membership rule the tier adjacency applies).
  Self-edges are dropped.
- **Cardinality:** one edge per `(source crate, target crate, kind)` — a
  dependency declared under two kinds yields two edges, one per kind.
- **Unfiltered by kind, by construction:** the dev-kind edge is the primary
  catch (the dev-dependency cycle). cfdb's own tree carries the living
  example: `cfdb-cli --normal--> cfdb-hir-extractor` AND
  `cfdb-hir-extractor --dev--> cfdb-cli` (documented in `crate_tier.rs` as
  the reason the TIER DAG filters to normal). An edge set is not a
  topological ranking and cannot cycle-fault, so no cycle guard applies to
  emission.
- **Determinism:** emission ordering derives from the same
  `BTreeMap`/`BTreeSet` walk the adjacency already uses; `cfdb extract`
  stays byte-stable on an unchanged tree.

## 4. Invariants

- **I1 — `crate_tier` is untouched.** It continues to derive from the
  normal-kind subset, preserving its contract and the documented cycle
  rationale for that filter (RFC-050 §5). The normal-kind projection of the
  `DEPENDS_ON` edge set is definitionally equal to the tier adjacency.
- **I2 — Non-breaking addition.** New edge label, no field changes:
  `SchemaVersion` is unchanged per `CLAUDE.md` §5; no graph-specs-rust
  lockstep PR is required. The addition is called out in `SchemaDescribe`.
- **I3 — Determinism gate holds** (`ci/determinism-check.sh` — byte-stable
  across two extracts).
- **I4 — No behavior change to any existing fact.** Existing node/edge
  emission is byte-identical; the change is purely additive.

## 5. Tests (RFC-033 §3.5 template)

```
Tests:
  - Unit: all-kinds workspace adjacency pure function — kind partitioning
    (normal/dev/build), self-edge drop, non-member drop, per-(source,
    target, kind) cardinality.
  - Self dogfood (cfdb on cfdb): the known real dev back-edge
    cfdb-hir-extractor -[DEPENDS_ON {kind:"dev"}]-> cfdb-cli is present;
    AND the kind="normal" projection of DEPENDS_ON equals the adjacency
    that computes crate_tier (edge set ⟷ tier consistency, I1).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows
    (no rule ships here; the gate proves the addition disturbs nothing).
  - Target dogfood (on qbot-core at pinned SHA): report the DEPENDS_ON
    edge count and per-kind histogram in the PR body for reviewer
    sanity-check.
  - Recall: none — rationale: rustdoc JSON carries no crate-dependency
    ground truth; the workspace manifest IS the ground truth, and the self
    dogfood asserts directly against this repo's own manifests.
```

## 6. Consumer note (non-normative)

After this ships, agentry authors its boundary fence as a normal
one-rule-per-PR ban over `(:Crate)-[:DEPENDS_ON]->(:Crate)` with zero
existing violations and an observed unplanted red first (its A1 §5), and
closes agentry #3034. The dev-kind match is that fence's primary catch.
