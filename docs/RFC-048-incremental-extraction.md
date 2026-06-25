# RFC-048 — Incremental extraction — **profile-gated (parsing is likely not the bottleneck)**

- **Status:** DRAFT — **REFRAMED 2026-06-25 after the UA discovery** ([`studies/003`](../studies/003-cfdb-understand-discovery.md)). The original premise (skip re-parsing unchanged files) is suspect: a generic Tree-sitter pass parsed cfdb's entire **352-file code tree in 0.575 s**, so parsing is almost certainly *not* what makes `extract` slow. This RFC is now **profile-first** — it does not authorize building any skip logic until a phase profile proves which phase actually dominates. The likely real lever is incremental *enrichment*, not parse-skip. (Borrowed candidate **C2** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
- **Issue:** none yet (filed only after the profile in §7 48-A justifies a downstream slice).
- **Schema impact:** **none on the wire graph** in v1 (any fingerprint cache is an out-of-band sidecar, §3.2).
- **Companion:** none in v1 (no schema surface).
- **Origin:** `Understand-Anything` `fingerprint.ts` + `change-classifier.ts` + `staleness.ts` — and, decisively, the wall-clock evidence from running its Tree-sitter floor on cfdb.

---

## 1. Problem (reframed by the discovery)

`extract` re-runs the whole pipeline every run, and on a `very-large` tree that hurts. The **original** RFC assumed re-parsing was the dominant cost and proposed skipping it for unchanged files. **The discovery falsifies that assumption's foundation:** `Understand-Anything`'s Tree-sitter floor parsed cfdb's entire 352-file code tree, emitting 2197 functions, in **0.575 s** ([`studies/003 §1`](../studies/003-cfdb-understand-discovery.md)). `syn` is heavier than Tree-sitter, but not minutes-heavier.

What actually consumes `extract` wall-clock is the **global, whole-graph work** that runs *after* parsing:
- `CALLS` resolution (cross-crate best-effort symbol matching),
- reachability BFS over `CALLS*` (`reachable_from_*`),
- `dup_cluster_id` sha256 clustering,
- git-history enrichment (per-item shellouts),
- the `cargo +nightly rustdoc` recall gate — the heaviest, tens of seconds to minutes.

Skipping re-parse of unchanged files optimises the **cheap** phase. So the first question is **not** "how do we skip parsing" but **"where does `extract` actually spend its seconds"** — and the whole build is gated on that answer.

## 2. Scope

**v1 ships a PROFILE, not an optimisation.** Instrument `extract` to attribute wall-clock per phase (walk, parse, `CALLS`-resolve, reachability, each `enrich_*`, recall) on a real target (cfdb-self and one large downstream target). That measurement decides everything downstream:
- **If parsing is material** → the original fingerprint-based parse-skip (now 48-C) is justified.
- **If the global passes dominate** (the expectation) → the real work is **incremental enrichment**: recompute reachability / dup-clusters / recall only for the subgraph touched by changed files, reusing prior results elsewhere — strictly harder, because these facts are global and `G1` must still hold.
- **If the rustdoc recall shellout dominates** → the honest answer may be that "incremental extraction" is the wrong frame entirely, and the higher-value RFC is *caching the rustdoc JSON* (its own RFC, §6).

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
- **The profile must not perturb the graph.** Timings are out-of-band (§3.1).
- **`G5`** snapshots immutable; **deterministic ordering** independent of which phase was reused.
- **Opt-in.** Default extraction unchanged; deterministic gates keep guarding it.

## 5. Architect lenses

> **DRAFT — for next-session hardening.** The discovery answered the original existential question with evidence, so the lenses are re-pointed:
- **rust-systems (lead):** given parsing is ~sub-second, quantify the per-phase breakdown and judge whether incremental *enrichment* (incremental reachability BFS, incremental recall) is even feasible under `G1` — or whether the breakdown will show the rustdoc recall shellout dominates, making "cache rustdoc JSON" the real RFC and this one moot.
- **clean-arch / solid:** the cache/instrumentation is infrastructure — confirm it sits in the extractor/enrichment adapter, never `cfdb-core`; the `StoreBackend`/`EnrichBackend` ports must not learn about caches or timers.
- **ddd:** unchanged — fingerprint/staleness is a build mechanism, not a schema concept; keep it out of the vocabulary.

## 6. Non-goals

- Building **any** skip logic before the profile (§2) — this is the whole point of the reframe.
- Caching the `cargo rustdoc` recall output — if the profile fingers recall as the bottleneck, that is its **own** (likely higher-value) RFC, not this one.
- Incremental *enrichment* implementation in v1 — it is only *scoped* here; it is filed (48-B) only if 48-A's profile justifies it.
- Watch-mode / long-running daemon; cross-machine cache.

## 7. Issue decomposition

### 48-A — Profile `extract` (the gate) — **DO THIS FIRST; the only unconditional slice**
Instrument and report per-phase wall-clock on cfdb-self + one large target.
```
Tests:
  - Unit: phase timers sum to the measured total within tolerance.
  - Self dogfood (cfdb on cfdb): emit the phase breakdown for cfdb; record which phase dominates.
  - Cross dogfood (graph-specs-rust): none — rationale: instrumentation only, no schema/ban surface.
  - Target dogfood (qbot-core): THE headline deliverable — report where extract's seconds go in the PR body. This number decides whether 48-B / 48-C are worth filing at all.
```

### 48-B — (CONDITIONAL on 48-A) Incremental enrichment
Only if the profile shows the global passes dominate. Scope reachability / dup / recall recompute to the changed subgraph.
```
Tests:
  - Unit: changed-subgraph closure includes exactly the items whose derived facts can change.
  - Self dogfood: incremental enrichment after a one-fn edit recomputes only the expected reachability set.
  - Cross dogfood: none — rationale: sidecar cache only, no schema change.
  - Target dogfood (qbot-core): wall-clock full vs. incremental-enrich on a single-crate change; report speedup.
```

### 48-C — (CONDITIONAL on 48-A) Fingerprint parse-skip
Only if the profile shows parsing is a material fraction of wall-clock — the original mechanism, now contingent.
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
