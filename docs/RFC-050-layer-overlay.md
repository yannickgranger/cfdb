# RFC-050 — Deterministic architectural-layer overlay

- **Status:** **RATIFIED** (council R1 REQUEST CHANGES ×4 → amendments applied → R2 **RATIFY ×4**). Decomposition (50-A, 50-C; 50-B killed) ready to file; not yet filed. **Requires a lockstep `graph-specs-rust` fixture PR** (schema bump). Verdicts: [`council/RFC-047-052/`](../council/RFC-047-052/). The A-vs-B dichotomy was dissolved: tier is an **extract-time `:Crate.crate_tier`** fact (see §3). (Borrowed candidate **C4** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
- **Issue:** none yet (filed only after ratification).
- **Schema impact:** **yes** — a single new `:Crate.crate_tier` attribute (`Provenance::Extractor`), **minor `SchemaVersion` bump**. `:Item.layer` (former 50-B) is **dropped** (denormalisation — items reach their crate's tier via `IN_CRATE`). No new edge label. cfdb does **not** model inter-crate Cargo dependencies as edges and this RFC does **not** add one — tiers are computed at extract time from each package's declared `[dependencies]` (§3).
- **Companion:** **required** — lockstep `.cfdb/cross-fixture.toml` bump on `graph-specs-rust` (RFC-033 §4) because the schema surface changes.
- **Origin:** `Understand-Anything` `layer-detector.ts` + `architecture-analyzer` (directory/path → architectural layer).

---

## 1. Problem

cfdb groups code by **ownership** (`:Context` = which bounded context owns a crate) but has no notion of **architectural role / tier** — where a unit sits in the dependency stack. The discovery in [`studies/003 §2`](../studies/003-cfdb-understand-discovery.md) showed cfdb's own architecture is a clean tiered DAG: `cfdb-core` (foundation, Zone of Pain) → mid-tier crates → `cfdb-cli` (composition root). That tiering is real, queryable architecture — "show me everything in the foundation tier", "does any tier-1 crate call up into tier-3" (a layering violation) — but cfdb cannot express it.

`Understand-Anything` assigns every file to one of ~10 architectural layers via directory patterns. The borrow is the *layer-as-overlay* idea, but adapted to what is deterministic for a Rust workspace: the **dependency tier**, not generic web-role names.

## 2. Scope

**Ships:** a deterministic **tier overlay** — every `:Crate` gets a single `crate_tier` attribute (its topological longest-path depth in the workspace normal-`[dependencies]` DAG), emitted at extract time. Plus a layering-violation query (50-C) enabled by the overlay. (The draft's per-`:Item` `layer` inheritance — former 50-B — is **dropped**; items reach their crate's tier via `IN_CRATE`. §3.4/§5/§7.)

**Does not ship (v1):** heuristic web-role labelling (`api`/`service`/`data`/`ui`) from path patterns — it is lower-confidence and culturally web-centric; deferred to a v2 if a consumer wants role names rather than tiers (§6).

## 3. Design

### 3.1 Resolution (council-converged) — extract-time `:Crate.crate_tier`, no edge, no verb
cfdb's vocabulary has `IN_CRATE` (node→crate) and `BELONGS_TO` (crate→context) but **no `crate → crate` dependency edge** (verified `crates/cfdb-core/src/schema/labels.rs` — no `DEPENDS_ON`). The draft posed this as a choice between (A) an enrich-time `Cargo.toml` read and (B) materialising a `DEPENDS_ON` edge. **The council dissolved the dichotomy:** `crate_tier` is a pure topological function of the Cargo `[dependencies]` DAG — a *structural fact* in the same class as `:Crate` itself — so it is emitted **at extract time** on the `:Crate` node, with `Provenance::Extractor` (whose descriptor already reads "walked from the syn AST *or `cargo_metadata`*" — `crates/cfdb-core/src/schema/descriptors.rs:26-31`), co-located with the existing `:Crate`/`:Context`/`BELONGS_TO` emission in `emit_crate_and_walk_targets` (`crates/cfdb-extractor/src/lib.rs:267-289`).

This is strictly better than both drafted options: (i) one additive attribute, zero edges (so it satisfies the DDD "no vocab-ahead-of-pull" concern that argues against B); (ii) it adds **no enrich verb and extends none** (so the closed-at-7 verb ceiling is never pressured — see §3.3); (iii) `cfdb-core` declares no DAG/manifest type, only the `crate_tier` string attribute. Materialising a `DEPENDS_ON` crate edge (the old B) is a genuinely separate capability ("who depends on crate X") with no current consumer — **deferred to its own RFC** (§6), and it is Main-Sequence-neutral (it adds data cfdb *produces*, not a dependency in cfdb's source).

**DAG sourcing (precise, council-verified).** The lone `cargo_metadata` call sets `.no_deps()` (`crates/cfdb-extractor/src/lib.rs:158`), so `metadata.resolve` is **not** populated — there is no resolved DAG in-process (do not claim otherwise). But each `Package.dependencies` (the manifest `[dependencies]`) **is** populated under `.no_deps()`. So: iterate `metadata.workspace_packages()`, read each `package.dependencies`, filter to `kind == Normal` **and** target ∈ workspace-member crate names (intra-workspace edges only), and compute the tier. The `kind == Normal` filter does double duty — it scopes to manifest deps *and* prevents the verified dev-dep false cycle (`cfdb-hir-extractor` dev-deps `cfdb-cli`, which under an all-kinds DAG would form `cfdb-cli → cfdb-hir-extractor →(dev)→ cfdb-cli` and false-trip the cycle check on cfdb-self). Keeps `.no_deps()`; no second `exec()`, no transitive resolve.

### 3.2 Tier computation (deterministic) — normal deps only
Topological **longest-path** depth over the intra-workspace **normal-`[dependencies]`** DAG: leaves with no in-workspace normal deps = tier 0; a crate's tier = `1 + max(crate_tier of its in-workspace normal deps)`. Longest-path (not shortest) is the correct "rank" — a crate is one above its *deepest* dependency; shortest-path would hide real depth. `cfdb-core` is unambiguously tier 0 (zero in-workspace deps; `studies/003 §2`) — a crate's tier is a function of what it depends *on*, not what depends on it.

**Dev/build deps are excluded — required, not optional.** cfdb-self has a real dev-dep back-edge: `cfdb-cli` normal-deps `cfdb-hir-extractor` (`crates/cfdb-cli/Cargo.toml:70`, optional) while `cfdb-hir-extractor` dev-deps `cfdb-cli` (`crates/cfdb-hir-extractor/Cargo.toml:44,49`). An all-kinds DAG therefore **cycles on cfdb-self**, and since a normal-deps cycle is a hard error (below), tier computation would hard-error on cfdb itself and make 50-A's own self-dogfood unreachable. Filtering to `kind == Normal` removes the back-edge. A cycle in the *normal-deps* graph (should one ever exist) is a hard error, not a silent default. Fully deterministic from the manifests — no heuristics, no LLM.

### 3.3 Where the overlay lives — extract time, NOT an enrich verb (council-corrected)
The draft proposed extending `enrich_bounded_context` to also emit `tier` (to dodge the closed-at-7 ceiling). **The council rejects this** on two verified grounds: (1) `enrich_bounded_context` reads only `.cfdb/concepts/*.toml` and patches `:Item.bounded_context` (`crates/cfdb-petgraph/src/enrich/bounded_context.rs:1-2,60`) — it has no Cargo-DAG access, so bolting tier onto it forces a second input domain + second reason-to-change into a pass whose whole point is being the single concept-ownership resolver (an SRP/CCP god-pass; the verb already carries a scope-*narrowing* scar from a prior conflation, `crates/cfdb-core/src/enrich.rs:157-162`); and (2) it is a *re-enrichment* pass that no-ops on a fresh extract, so tier structurally cannot originate there. An 8th verb is also wrong (breaches the ceiling; `cfdb-core.md:215` blesses "extend via schema + Cypher, not new trait methods"). **`crate_tier` is therefore emitted at extract time on `:Crate` (§3.1) — no enrich verb, no extension, ceiling untouched.** This is the Q2 verb-ceiling rule applied: prefer an extract-time `Provenance::Extractor` attribute when the fact is a pure function of data the extractor already loads.

### 3.4 Split-brain test (mandatory for a CREATE) — PASSES (ddd, four-lens converged)
`:Context` answers *who owns this* (ownership, derived from crate-name prefix stripping with zero DAG knowledge — `crates/cfdb-extractor/src/overlay.rs:32-49`); `crate_tier` answers *what role it plays in the stack* (Cargo-DAG depth). They are **orthogonal and independently variable**: `cfdb-core` (tier 0) and `cfdb-cli` (tier 3) sit in the *same* `cfdb` context (`studies/003 §2`), and one tier can hold crates from multiple contexts. There is no existing canonical resolver for "tier" (grep clean in `crates/cfdb-core/src/schema/`) — `enrich_bounded_context` resolves ownership, not depth. The concept is genuinely independent → CREATE justified, as a `:Crate.crate_tier` attribute.

## 4. Invariants

- **Determinism (`G1`).** `crate_tier` is a pure topological function of the Cargo manifests, computed in the same deterministic extract pass as every other `:Crate` fact; byte-stable re-extract preserved. It is **not** toolchain-scoped, so it correctly stays **inside** the `G1` canonical dump (and is recall-gated — unlike the LLM attributes of RFC-052).
- **Additive schema (`G4`).** One new optional `crate_tier` attribute on `:Crate` under a minor `SchemaVersion` bump; a lower-minor reader correctly refuses a higher-minor graph per `can_read`. **Lockstep `graph-specs-rust` `.cfdb/cross-fixture.toml` bump required** (`CLAUDE.md §3`) — this is the one schema-bumping RFC in the batch.
- **Ground-truth gate (recall extension, `CLAUDE.md §5`).** A new extractor fact MUST extend `cfdb-recall`: the gate asserts `computed crate_tier == topological depth of the normal-`[dependencies]` manifest graph` for every crate — the layering analog of the rustdoc recall gate (50-A).
- **No new verb AND no verb extension.** `crate_tier` is an extract-time `Provenance::Extractor` fact (§3.1/§3.3); the closed-at-7 `EnrichBackend` surface is untouched.

## 5. Architect lenses

> **HARDENED by the RFC-047..052 council** — R1 **REQUEST CHANGES (all four lenses)** → unanimous convergence on extract-time `:Crate.crate_tier`. Flip conditions (all applied): resolution C, `crate_tier` name, kill 50-B, recall row, normal-deps-only DAG.

- **clean-arch — REQUEST CHANGES → RATIFY on the amendments.** §3.3's "extend `enrich_bounded_context`" is a god-pass: that pass's single documented responsibility is re-reading `.cfdb/concepts/*.toml` (`enrich/bounded_context.rs:1-2,60`); it reads no `Cargo.toml` and computes no DAG. Folding tier in adds a second input source + second reason-to-change. Flip: compute at extract time on `:Crate` where `cargo_metadata` already runs (`lib.rs:267-289`); `cfdb-core` declares no DAG/manifest type. The Cargo read stays in the extractor adapter where it already is.
- **ddd (Q5 lead) — REQUEST CHANGES → RATIFY on rename + kill 50-B.** Split-brain test passes (§3.4): `crate_tier` ⟂ `:Context`, CREATE justified. Two blocking naming defects: (1) name it **`crate_tier`**, never `layer`/`tier` — "Layer 1/Layer 2" is live provenance vocabulary (`descriptors.rs:11-16`), so `:Crate.layer` would be one word for two concepts; (2) **kill 50-B** (`:Item.layer`) — tier is a `:Crate`-aggregate property; items reach it via `IN_CRATE`, so a per-item copy is denormalisation. Applied.
- **solid (Q2 lead) — REQUEST CHANGES → RATIFY.** §3.3 is an SRP/CCP god-pass (the verb already bears a scope-narrowing scar, `enrich.rs:157-162`); an 8th verb breaches the ceiling. The SOLID-correct third path (Q2 rule path 1): an extract-time `Provenance::Extractor` attribute, because `tier` is a pure function of data the extractor already loads. Q1/SDP: materialising `DEPENDS_ON` (old B) is Main-Sequence-neutral but a forever concrete edge label in cfdb-core's Zone-of-Pain schema with no consumer — its own deferred RFC. Independently reached the kill-50-B verdict (CRP). Applied.
- **rust-systems (Q1 co-lead) — REQUEST CHANGES → RATIFY.** Longest-path is correct (rank = one above the *deepest* dependency); `cfdb-core` is unambiguously tier 0. The "depended on from every tier" worry is a non-issue — a crate's tier depends on what it depends *on*, not its afferent coupling. **DAG must be normal-`[dependencies]`-only** — verified `cfdb-hir-extractor` dev-deps `cfdb-cli` (`Cargo.toml:44,49`) while `cfdb-cli` normal-deps it (`Cargo.toml:70`), so an all-kinds DAG cycles on cfdb-self and would false-trip the hard-error. Corrected own Phase-A claim: `.no_deps()` (`lib.rs:158`) means the resolved DAG is *not* in-process; source from each `package.dependencies` (`kind==Normal`, workspace-filtered) instead. Applied.

## 6. Non-goals

- Heuristic web-role names (`api`/`service`/`ui`/`data`) from path patterns — deferred (§2); v1 is tier-only.
- Intra-crate module layering (a module's role within its crate) — possible v2; v1 is crate-granular.
- A general `DEPENDS_ON` crate edge — the old resolution (B); **deferred to its own RFC** (council: no current consumer, and a concrete edge label in cfdb-core's Zone-of-Pain schema is a forever commitment — do not fold in here).
- **`crate_tier` is dependency-*depth* only (efferent), NOT instability** `I = Ce/(Ca+Ce)` **or distance-from-main-sequence `D`** (solid). Conflating depth with the afferent/Zone-of-Pain metric would seed a future split-brain; an instability/`D` attribute is a separate attribute + separate RFC if a consumer pulls it.
- Enforcing layering rules (banning up-calls) — that is a *ban-rule* (`.cfdb/queries/*.cypher`) built *on top of* this overlay, not part of it.

## 7. Issue decomposition

### 50-A — Extract-time `:Crate.crate_tier` attribute (+ schema bump + lockstep)
Compute `crate_tier` at extract time on `:Crate` from each `package.dependencies` (`kind==Normal`, workspace-filtered; §3.1/§3.2), `Provenance::Extractor`. Minor `SchemaVersion` bump + lockstep `graph-specs-rust` `.cfdb/cross-fixture.toml` PR. New extractor fact → recall extension (`CLAUDE.md §5`).
```
Tests:
  - Unit: topological longest-path crate_tier of a synthetic 4-crate normal-deps DAG matches hand-computed depths; a normal-deps cycle errors; AND a workspace with normal A→B + dev B→A computes finite tiers and does NOT error (dev/build deps excluded).
  - Self dogfood (cfdb on cfdb): assert :Crate.crate_tier — cfdb-core == 0 and cfdb-cli == max (matches studies/003 §2).
  - Cross dogfood (graph-specs-rust at pinned SHA): schema bump landed lockstep; assert crate_tier computed with zero rule-row regressions → exit 0.
  - Recall (cfdb-recall, CLAUDE.md §5): computed crate_tier == topological depth of the normal-[dependencies] manifest DAG for every crate in the ground-truth workspace.
  - Target dogfood (qbot-core): report the crate_tier histogram of the workspace in PR body.
```
(Four Tests rows + the mandatory recall row — recall is required because this adds a new extractor fact.)

### 50-B — KILLED by the council
`:Item.layer` (crate-tier inheritance onto items) is **dropped** — denormalisation with no consumer; the former 50-C up-call query reaches a crate's `crate_tier` via `IN_CRATE` in one hop (clean-arch/solid CRP + ddd aggregate-boundary). If a measured query-ergonomics need ever appears, denormalise as `:Item.crate_tier` (same term, explicitly a copy), never `:Item.layer`.

### 50-C — Layering-violation query (the payoff)
A canonical query (and candidate ban-rule) for "a lower tier calls up into a higher tier," joining `CALLS` to `:Crate.crate_tier` via `IN_CRATE`.
```
Tests:
  - Unit: query AST matches the up-call pattern.
  - Self dogfood (cfdb on cfdb): assert zero up-calls in cfdb (clean acyclic tiers) — a real architectural assertion about this repo. NOTE: cross-crate CALLS are HIR-resolved (`--features hir`); state the feature requirement so the gate isn't run on a syn-only (recall-incomplete) graph and falsely reported clean.
  - Cross dogfood (graph-specs-rust): assert zero up-call findings on the companion → exit 0.
  - Target dogfood (qbot-core): report any up-call violations found, for reviewer sanity-check.
```
