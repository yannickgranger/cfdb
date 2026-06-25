# RFC-050 — Deterministic architectural-layer overlay

- **Status:** DRAFT — pending architect hardening + council. (Borrowed candidate **C4** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
- **Issue:** none yet (filed only after ratification).
- **Schema impact:** **yes** — a new `:Crate.tier` attribute (and optionally `:Item.layer`), **minor `SchemaVersion` bump**. A surfaced prerequisite: cfdb does **not** currently model inter-crate Cargo dependencies as edges (§3.1) — this RFC must resolve how tiers are sourced.
- **Companion:** **required** — lockstep `.cfdb/cross-fixture.toml` bump on `graph-specs-rust` (RFC-033 §4) because the schema surface changes.
- **Origin:** `Understand-Anything` `layer-detector.ts` + `architecture-analyzer` (directory/path → architectural layer).

---

## 1. Problem

cfdb groups code by **ownership** (`:Context` = which bounded context owns a crate) but has no notion of **architectural role / tier** — where a unit sits in the dependency stack. The discovery in [`studies/003 §2`](../studies/003-cfdb-understand-discovery.md) showed cfdb's own architecture is a clean tiered DAG: `cfdb-core` (foundation, Zone of Pain) → mid-tier crates → `cfdb-cli` (composition root). That tiering is real, queryable architecture — "show me everything in the foundation tier", "does any tier-1 crate call up into tier-3" (a layering violation) — but cfdb cannot express it.

`Understand-Anything` assigns every file to one of ~10 architectural layers via directory patterns. The borrow is the *layer-as-overlay* idea, but adapted to what is deterministic for a Rust workspace: the **dependency tier**, not generic web-role names.

## 2. Scope

**Ships:** a deterministic **tier overlay** — every `:Crate` gets a `tier` (its topological depth in the workspace dependency DAG), and (optionally) every `:Item` inherits a `layer` from its crate. Plus a layering-violation query enabled by the overlay.

**Does not ship (v1):** heuristic web-role labelling (`api`/`service`/`data`/`ui`) from path patterns — it is lower-confidence and culturally web-centric; deferred to a v2 if a consumer wants role names rather than tiers (§6).

## 3. Design

### 3.1 Prerequisite surfaced by discovery — cfdb has no crate-dependency edge
cfdb's vocabulary (`specs/concepts/cfdb-core.md`) has `IN_CRATE` (node→crate) and `BELONGS_TO` (crate→context) but **no `crate → crate` dependency edge**. The tier of a crate is the topological depth of the Cargo `[dependencies]` DAG — which cfdb does not currently store. Two resolutions, for the architects to choose:
- **(A) Enrich-time, no new edge.** A layering enrichment reads `Cargo.toml` manifests directly, computes tiers, and writes the `tier` attribute — the DAG is consumed but never materialised as graph edges. Smallest schema surface (one attribute). **Draft preference.**
- **(B) Materialise a `DEPENDS_ON` crate edge first.** Richer (enables "who depends on crate X" queries) but a larger schema bump and arguably a separate RFC. Deferred unless the council prefers it.

### 3.2 Tier computation (deterministic)
Topological longest-path depth over the crate dependency DAG: leaves with no in-workspace deps = tier 0; a crate's tier = `1 + max(tier of its in-workspace deps)`. The DAG is acyclic (verified in `studies/003 §2`); a cycle (should one ever exist) is a hard error, not a silent default. Fully deterministic from the manifests — no heuristics, no LLM.

### 3.3 Where the overlay lives — verb-ceiling note
Materialising the overlay fits the `enrich_*` pattern (like `enrich_concepts` materialises `:Concept`). But **`EnrichBackend` is closed at 7 verbs** (`RFC-031 §2`). v1 therefore **extends the existing `enrich_bounded_context` pass** (which already derives crate-level structure) to also emit the `tier` attribute, rather than adding an 8th verb. Whether layer derivation belongs inside bounded-context enrichment or warrants the council blessing a new verb is the central architect question (§5).

### 3.4 Split-brain test (mandatory for a CREATE)
`:Context` answers *who owns this* (ownership); `tier`/`layer` answers *what role it plays in the stack* (architecture). They are **orthogonal**: one context can span multiple tiers; one tier can span multiple contexts. There is no existing canonical resolver for "tier" — `enrich_bounded_context` resolves ownership, not depth. The concept is therefore genuinely independent and CREATE is justified. (If an architect finds `:Context` already implies a tier, this RFC collapses into a `:Context` attribute instead.)

## 4. Invariants

- **Determinism (`G1`).** `tier` is a pure topological function of the Cargo manifests; byte-stable re-extract preserved. The attribute is **not** toolchain-scoped (unlike `test_coverage`), so it stays inside the `G1` canonical dump.
- **Additive schema (`G4`).** New optional `tier` attribute under a minor bump; a lower-minor reader correctly refuses a higher-minor graph per `can_read`. Lockstep `graph-specs-rust` fixture bump (`CLAUDE.md §3`).
- **Ground-truth gate (recall substitute).** Tiers are checkable against the Cargo DAG directly — the gate asserts `computed tier == topological depth of the manifest graph`, the layering analog of the rustdoc recall gate.
- **No new verb without council.** v1 extends `enrich_bounded_context` (§3.3).

## 5. Architect lenses

> **DRAFT — to be filled by next-session architect hardening before council.** Pre-seeded focus:
- **clean-arch:** does tier derivation belong in the bounded-context enrichment, or is that a god-pass? Reading `Cargo.toml` is infra — confirm it sits in the enrichment adapter, not `cfdb-core`.
- **ddd:** the §3.4 split-brain test is the crux — ratify that `tier`/`layer` is independent of `:Context`, or fold it in. Homonym check: `:Layer`/`layer` vs. `Understand-Anything`'s generic web layers (different meaning).
- **solid:** resolution (A) vs. (B) in §3.1 — single attribute vs. a `DEPENDS_ON` edge; SDP/SAP (stable-dependencies) implications of materialising the DAG.
- **rust-systems:** longest-path tier vs. shortest-path; how to present a crate that is reachable at multiple depths (e.g. `cfdb-core` is depended on from every tier).

## 6. Non-goals

- Heuristic web-role names (`api`/`service`/`ui`/`data`) from path patterns — deferred (§2); v1 is tier-only.
- Intra-crate module layering (a module's role within its crate) — possible v2; v1 is crate-granular.
- A general `DEPENDS_ON` crate edge — resolution (B), deferred to its own RFC unless the council pulls it in.
- Enforcing layering rules (banning up-calls) — that is a *ban-rule* (`.cfdb/queries/*.cypher`) built *on top of* this overlay, not part of it.

## 7. Issue decomposition

### 50-A — Crate-tier computation + `:Crate.tier` attribute
Topological tier from the Cargo DAG (resolution A), emitted via extended `enrich_bounded_context`.
```
Tests:
  - Unit: topological tier of a synthetic 4-crate DAG matches hand-computed depths; a cycle errors.
  - Self dogfood (cfdb on cfdb): assert cfdb-core.tier == 0 and cfdb-cli.tier == max (matches studies/003 §2).
  - Cross dogfood (graph-specs-rust at pinned SHA): schema bump landed lockstep; assert tiers computed with zero rule-row regressions → exit 0.
  - Target dogfood (qbot-core): report the tier histogram of the workspace in PR body.
```

### 50-B — `:Item.layer` inheritance (optional)
Propagate crate tier to items as a queryable `layer`.
```
Tests:
  - Unit: an item's layer equals its crate's tier.
  - Self dogfood: a cfdb-core item is layer 0; a cfdb-cli item is top layer.
  - Cross dogfood: lockstep fixture covers the new attribute; exit 0.
  - Target dogfood: none — rationale: derived 1:1 from 50-A, no independent target signal.
```

### 50-C — Layering-violation query (the payoff)
A canonical query (and candidate ban-rule) for "a lower tier calls up into a higher tier."
```
Tests:
  - Unit: query AST matches the up-call pattern.
  - Self dogfood (cfdb on cfdb): assert zero up-calls in cfdb (the DAG is acyclic → clean) — a real architectural assertion about this repo.
  - Cross dogfood (graph-specs-rust): assert zero up-call findings on the companion → exit 0.
  - Target dogfood (qbot-core): report any up-call violations found, for reviewer sanity-check.
```
