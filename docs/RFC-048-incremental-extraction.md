# RFC-048 — Incremental extraction — **profile-gated (parsing is likely not the bottleneck)**

- **Status:** **PARTIALLY RATIFIED** Only 48-A is filed-eligible. REFRAMED 2026-06-25. Profile-first: no skip/incremental logic is authorized until the corrected profile proves which phase dominates.
- **Issue:** none yet (filed only after the profile in §7 48-A justifies a downstream slice).
- **Schema impact:** **none on the wire graph** in v1 (any fingerprint cache is an out-of-band sidecar, §3.2).
- **Companion:** none in v1 (no schema surface).
- **Origin:** `Understand-Anything` `fingerprint.ts` + `change-classifier.ts` + `staleness.ts` — and, decisively, the wall-clock evidence from running its Tree-sitter floor on cfdb.

---

## 1. Problem (reframed by the discovery)

`extract` re-runs the whole pipeline every run, and on a `very-large` tree that hurts. The **original** RFC assumed re-parsing was the dominant cost and proposed skipping it for unchanged files. **The discovery falsifies that assumption's foundation:** `Understand-Anything`'s Tree-sitter floor parsed cfdb's entire 352-file code tree, emitting 2197 functions, in **0.575 s** ([`studies/003 §1`](../studies/003-cfdb-understand-discovery.md)). `syn` is heavier than Tree-sitter, but not minutes-heavier.

The reframe blamed "global passes that run after parsing." Reading the pipeline (`crates/cfdb-cli/src/commands/extract.rs:42-110`), **those passes are not in `extract` at all.** `cfdb extract` runs exactly: (a) `cargo metadata` subprocess (`cfdb-extractor/src/lib.rs:156`), (b) the per-file `syn` walk + `resolver::resolve_deferred_*` (`lib.rs:234,243`), (c) `ingest_nodes`/`ingest_edges`, (d) the `ra_ap_load_cargo` HIR load **iff `--hir`** (`extract.rs:103`) — a near-full type-checking compile, almost certainly the dominant cost when enabled, and (e) `save`. The phases the RFC blamed — `enrich_reachability`, `enrich_metrics`/`dup_cluster_id`, git-history — are **separate `cfdb enrich-*` verbs** (`crates/cfdb-petgraph/src/enrich_backend.rs:151,187`), and the `cargo +nightly rustdoc` recall gate is the **separate `cfdb-recall` crate** (`ground_truth.rs:118`), a distinct CI step. None of them runs during `cfdb extract`.

So the real `extract`-internal cost candidates are exactly three: the `cargo metadata` subprocess, the `syn` walk + deferred resolution, and (only under `--hir`) the `ra_ap_load_cargo` compile. The first question remains **"where does `extract` actually spend its seconds"** — but the profile must measure *those* phases, not the enrich/recall phases that live elsewhere. The whole build is gated on that corrected answer.

## 2. Scope

**v1 ships a PROFILE, not an optimisation.** Instrument `extract` to attribute wall-clock across its **actual** phases — `{cargo-metadata, syn-walk, deferred-resolve, ingest, hir-load (if --hir), save}` (§1) — on a real target (cfdb-self and one large downstream target). The enrich/recall passes, which run as *separate verbs/gates* (§1), are profiled in a **separate** optional "enrich/recall profile" slice, not bundled into the extract profile. That measurement decides everything downstream:
- **If the `syn` walk is material** → the original fingerprint-based parse-skip (now 48-C) is justified.
- **If the `--hir` `ra_ap_load_cargo` compile dominates** (the likely answer when `--hir` is on) → the lever is HIR-load reuse / incrementality, a different problem from parse-skip.
- **If a *separately-profiled* enrich pass dominates** → the real work is **per-pass incremental enrichment** (recompute reachability / dup only for the subgraph touched by changed files, reusing prior results) — strictly harder, because these facts are global and `G1` must hold; scoped **per existing pass**, never as a cross-cutting engine (solid CCP).
- **If the `cargo +nightly rustdoc` recall gate dominates** → it is not even in `extract`; the higher-value RFC is *caching the rustdoc JSON* (its own RFC, §6), not "incremental extraction."

**Does not ship:** any parse-skip or enrichment-skip code before the profile justifies it.

## 3. Design

### 3.1 The profile (the gate)
A `--profile` mode (or a one-off instrumented run) emitting a per-phase wall-clock breakdown. This is the v1 deliverable. Timings are reported to stderr/JSON, **never** written into the `G1` graph (so the profile cannot perturb determinism).

### 3.2 Fingerprint (unchanged mechanism, now downstream of the profile)
Per file: `content_hash = sha256(bytes)` + a structural fingerprint = the multiset of `:Item.signature_hash` plus the import set; tiers NONE / COSMETIC / STRUCTURAL as in the original draft. Stored in an out-of-band sidecar next to the keyspace — **no `SchemaVersion` bump**. Its *consumer* is selected by the profile: it may drive parse-skip (48-C) or, more likely, scope incremental enrichment (48-B).

### 3.3 The hard part is enrichment, not parsing
cfdb's expensive facts (`reachable_*`, `dup_cluster_id`, recall) are **global** — functions of the whole graph, not one file. Making them incremental while preserving `G1` means: recompute any derived fact whose inputs touch a changed file, reuse the rest, and **prove the merged result is byte-identical to a full run**. This is where the engineering value — if any survives the profile — actually lives, and it is materially harder than the parse-skip the original draft scoped.

## 4. Invariants

- **`G1` — make-or-break.** Any incremental path (parse OR enrichment) MUST produce a byte-identical canonical dump to a full extract of the same workspace SHA (§7 48-D). If a tier can't guarantee it, that tier falls back to FULL.
- **Fingerprint/change-class is build-cache state, NOT vocabulary (ddd — fence line).** The structural fingerprint and the SKIP/PARTIAL/FULL change-class are persisted build-cache state under `.cfdb/`: **no `Label`, no `EdgeLabel`, no `:Item` attribute, no `SchemaVersion` bump.** They are properties of the build process (like the `G1` canonical-dump invariant and the G6 dump-stability concern), never nodes in the graph. The recall corpus asserts the byte-identical-dump property; the cache key never enters `SchemaDescribe`.
- **The profile must not perturb the graph.** Timings are out-of-band (§3.1).
- **`G5`** snapshots immutable; **deterministic ordering** independent of which phase was reused.
- **Opt-in.** Default extraction unchanged; deterministic gates keep guarding it.

## 5. Architect lenses

- The profile is the right unconditional first slice, but its phase list was factually wrong: `extract` runs none of reachability/dup/git/recall (§1). Flip condition: profile the real phases `{cargo-metadata, syn-walk, deferred-resolve, ingest, hir-load (if --hir), save}`; split enrich/recall into a separate profile. It bundles reachability + dup + recall as one effort, but recall isn't in extract and the three have entirely different incrementality profiles. Incremental enrichment *is* determinism-feasible (all three passes use `BTreeSet`/`BTreeMap` + stable sort, so a byte-identical merge is possible — `reachability.rs:37-42`, `clustering.rs:35,66`, `canonical_dump.rs:46-92`) but value-unproven; re-derive per-pass from the corrected profile, or fold the recall pain into the "cache rustdoc JSON" RFC (§6).
- A profile has no abstraction surface. 48-B as scoped ("recompute reachability/dup/recall in one module") is a CCP violation — three reasons to change behind one door. Flip condition: incrementality is implemented **per existing enrichment pass** (each pass gains an optional changed-subgraph scope parameter), not a new cross-cutting "incremental engine"; the 48-D equivalence proof then runs per pass, keeping each pass's SRP intact.
- The incremental cache is infra and must live in the petgraph adapter (a sidecar next to the keyspace), never on a port: the seven `EnrichBackend` signatures (`enrich.rs:91`) and `StoreBackend` (`store.rs:63`) gain no cache handle, fingerprint type, or path. Reachability is whole-graph BFS (`enrich_backend.rs:151-185`), so 48-D byte-equivalence is the real arbiter.
- Fingerprint/staleness is a build mechanism, never schema vocabulary — enforced by the §4 fence line (no `Label`/`EdgeLabel`/`:Item` attr/`SchemaVersion`). Feasibility deferred to rust-systems; the DDD insistence is only that the cache key never becomes vocabulary.

## 6. Non-goals

- Building **any** skip logic before the profile (§2) — this is the whole point of the reframe.
- Caching the `cargo rustdoc` recall output — if the profile fingers recall as the bottleneck, that is its **own** (likely higher-value) RFC, not this one.
- Incremental *enrichment* implementation in v1 — it is only *scoped* here; it is filed (48-B) only if 48-A's profile justifies it.
- Watch-mode / long-running daemon; cross-machine cache.

## 7. Issue decomposition

### 48-A — Profile `extract` (the gate) — **DO THIS FIRST; the only unconditional slice** — RATIFY (on corrected phases)
Instrument and report per-phase wall-clock over the **real** `extract` phases `{cargo-metadata, syn-walk, deferred-resolve, ingest, hir-load (if --hir), save}` on cfdb-self + one large target. The enrich/recall passes (separate verbs/gates, §1) are a **separate optional profile**, not part of this slice.
```
Tests:
  - Unit: phase timers sum to the measured total within tolerance.
  - Self dogfood (cfdb on cfdb): emit the breakdown over the {cargo-metadata, syn-walk, deferred-resolve, ingest, hir-load (if --hir), save} phases; record which dominates (--hir on vs off).
  - Cross dogfood (graph-specs-rust): none — rationale: instrumentation only, no schema/ban surface.
  - Target dogfood (qbot-core): THE headline deliverable — report where extract's seconds go in the PR body (over the real phases). This number decides whether 48-B / 48-C are worth filing at all.
```

### 48-B — **DEFER** (re-derive per-pass from corrected 48-A) Incremental enrichment
Not filed until 48-A shows a *separately-profiled* enrich pass dominates. When filed, it is scoped **per existing pass** (each enrich pass gains an optional changed-subgraph scope parameter), **never** as a cross-cutting "incremental engine" spanning reachability + dup + recall (solid CCP). recall is not in `extract` and is excluded — if recall is the pain, it belongs to the "cache rustdoc JSON" RFC (§6). Cache is an adapter sidecar; no `EnrichBackend`/`StoreBackend` signature change (clean-arch).
```
Tests:
  - Unit: for the one targeted pass, the changed-subgraph closure includes exactly the items whose derived facts can change.
  - Self dogfood: incremental run of that pass after a one-fn edit recomputes only the expected set; 48-D proves byte-identical merge.
  - Cross dogfood: none — rationale: sidecar cache only, no schema change.
  - Target dogfood (qbot-core): wall-clock full vs. incremental for that pass on a single-crate change; report speedup.
```

### 48-C — **DEFER** (contingent on corrected 48-A) Fingerprint parse-skip
Not filed unless the corrected profile shows the `syn` walk is a material fraction of wall-clock — the original mechanism, now contingent.
```
Tests:
  - Unit: fingerprint tiers a structural vs. cosmetic edit correctly.
  - Self dogfood: extract --incremental re-parses only the expected files (instrumented count).
  - Cross dogfood: none — rationale: no schema change.
  - Target dogfood (qbot-core): parse-phase speedup on a single-file change.
```

### 48-D — `G1` byte-equivalence gate (covers whichever of 48-B / 48-C ships)
A CI gate asserting `incremental == full` canonical dumps over a synthetic change matrix.
```
Tests:
  - Self dogfood: for N synthetic edits, sha256(incremental dump) == sha256(full dump).
  - Cross dogfood (graph-specs-rust at pinned SHA): same equivalence on the companion; mismatch → exit 30.
  - Unit: none — rationale: inherently an end-to-end equivalence property.
  - Target dogfood (qbot-core): one real commit's incremental dump matches its full dump.
```
