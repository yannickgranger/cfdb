# RFC-048 — Incremental extraction (structural-fingerprint reuse)

- **Status:** DRAFT — pending architect hardening + council. (Borrowed candidate **C2** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
- **Issue:** none yet (filed only after ratification).
- **Schema impact:** **none on the wire graph** in v1 (the fingerprint cache is an out-of-band sidecar, not a graph fact — see §3.2). A `:File.content_hash` attribute is an explicit *alternative* considered and **deferred** to keep the wire contract untouched.
- **Companion:** none in v1 (no schema surface).
- **Origin:** `Understand-Anything` `fingerprint.ts` + `change-classifier.ts` + `staleness.ts`.

---

## 1. Problem

cfdb re-extracts the entire workspace on every `extract`. The discovery in [`studies/003`](../studies/003-cfdb-understand-discovery.md) classified cfdb's own tree as **`very-large`** (654 files, 344 Rust, 2197 functions), with two crates — `cfdb-petgraph` (578 fns) and `cfdb-cli` (391 fns) — dominating. A one-line change in a leaf crate pays the full-workspace re-parse cost. This does not scale to the larger downstream targets (qbot-core, agentry) where cfdb is run repeatedly in CI and locally.

The cost is *re-parsing*, not *re-storing*: `extract` re-runs `syn`/tree-sitter over every file even when 99% are byte-identical to the last run.

## 2. Scope

**Ships:** an opt-in incremental mode (`extract --incremental`, off by default) that:
1. Fingerprints every source file (content hash + the structural signature cfdb already computes).
2. Classifies the change set against the prior run (`SKIP` / `PARTIAL` / `FULL`).
3. Re-parses only changed files and conservatively recomputes any cross-file fact touching a changed file.
4. **Produces a canonical dump byte-identical to a full re-extract of the same workspace SHA** — this is the gate, not a nice-to-have (§4, §7 48-C).

**Does not ship:** incremental *enrichment* (the `enrich_*` passes — separate concern), a watch-mode daemon, or any change to the default (full) extraction path.

## 3. Design

### 3.1 Fingerprint (borrowed shape)
Per file: `content_hash = sha256(bytes)` + a **structural fingerprint** = the multiset of `:Item.signature_hash` values cfdb already emits for that file plus its import set. Comparison tiers mirror `Understand-Anything`:
- `content_hash` equal → **NONE** (file is byte-identical; reuse all prior facts verbatim).
- `content_hash` differs, structural fingerprint equal → **COSMETIC** (comment/whitespace; reuse structural facts, but see §3.4 caveat).
- structural fingerprint differs → **STRUCTURAL** (re-parse this file).

### 3.2 Where the fingerprint lives — sidecar cache, not the wire graph
v1 stores the prior `{path → (content_hash, structural_fingerprint)}` map in an **out-of-band cache file** next to the keyspace (e.g. `<db>/<keyspace>.fingerprints`), **not** as a `:File` graph attribute. Rationale: change-detection metadata is not a queryable fact about the code, and keeping it out of the wire graph means **no `SchemaVersion` bump and no `graph-specs-rust` lockstep** — the cheapest on-charter path. (Alternative considered: a `:File.content_hash` attribute under a minor bump; deferred — it leaks a build-cache concern into the wire contract for no consumer benefit.)

### 3.3 Change classification → extraction plan
Borrow the `change-classifier` decision tree, adapted to cfdb's crate structure:
- **SKIP** — no file changed → re-emit the prior snapshot unchanged (still validated by §4 determinism).
- **PARTIAL** — a bounded set of files changed within existing crates → re-parse those files; recompute cross-file edges per §3.4.
- **FULL** — a `Cargo.toml` dependency edge changed, a crate was added/removed, or the changed set exceeds a threshold fraction → fall back to full extraction (correctness over cleverness).

The candidate changed-file set may be narrowed with `git diff --name-only` (same deterministic source as RFC-047 §3.3), but the fingerprint comparison is authoritative — git is only a pre-filter.

### 3.4 The hard part — cross-file facts (correctness over speed)
cfdb's `CALLS`, `IMPLEMENTS`, `TYPE_OF`, reachability, and `dup_cluster_id` are **global** — they depend on a workspace-wide symbol table, not a single file. Incremental MUST therefore:
- Recompute any edge whose **source or target** item lives in a changed (STRUCTURAL) file.
- Treat **COSMETIC** conservatively: if a cosmetic change could alter line numbers that appear in facts (`:CallSite.line`, `:Item.line`), it is **not** cosmetic for cfdb — line attributes make most "cosmetic" edits structural. v1 may collapse COSMETIC into STRUCTURAL for safety and revisit later.
- Recompute global derived facts (`reachable_*`, `dup_cluster_id`) whenever any STRUCTURAL file changed, since they are not file-local.

This conservatism is the price of `G1`. The speedup comes from skipping **parsing** of unchanged files, not from skipping global recomputation.

## 4. Invariants

- **`G1` — the make-or-break.** For any `(workspace SHA, schema major.minor)`, an incremental extract MUST produce a **byte-identical** canonical JSONL dump to a full extract. This is asserted by a dedicated gate (§7 48-C); if it cannot be guaranteed for a tier, that tier falls back to FULL.
- **`G5` — snapshots immutable.** Incremental writes a *new* snapshot; it never rewrites a keyspace in place.
- **Determinism of ordering.** The canonical dump's node/edge ordering MUST be independent of *which* files were re-parsed — ordering is by qname/stable key, never by parse/insertion order.
- **No wire-schema change (v1).** Fingerprints are sidecar (§3.2).
- **Opt-in.** Default extraction is unchanged; `--incremental` is the only entry to this path, so the recall gate and determinism check keep guarding the default.

## 5. Architect lenses

> **DRAFT — to be filled by next-session architect hardening before council.** Pre-seeded focus:
- **clean-arch:** the fingerprint cache is infrastructure — confirm it sits in the extractor adapter, not `cfdb-core`; the `StoreBackend` port must not learn about caches.
- **ddd:** is "fingerprint/staleness" a domain concept or a build-cache mechanism? (Draft: mechanism — keep it out of the schema vocabulary, §3.2.)
- **solid:** SRP between fingerprint computation, change classification, and the extraction planner — three units, not one.
- **rust-systems:** measured speedup vs. the conservative global-recompute floor; is parse-skipping alone worth it given `CALLS`/reachability must still recompute? **This is the candidate's existential question** — quantify before committing.

## 6. Non-goals

- Incremental enrichment (`enrich_*` passes recomputed selectively) — separate RFC if pursued.
- Watch-mode / long-running daemon.
- Cross-machine cache sharing (the sidecar is local; no distributed cache).
- COSMETIC-tier optimisation in v1 (collapsed into STRUCTURAL for safety, §3.4).

## 7. Issue decomposition

### 48-A — Fingerprint + sidecar cache
Compute `(content_hash, structural_fingerprint)` per file; read/write the sidecar.
```
Tests:
  - Unit: fingerprint of a fixed file is stable across runs; differs on a structural edit; (documented) policy for cosmetic edits.
  - Self dogfood (cfdb on cfdb): extract cfdb, touch one comment, re-fingerprint; assert exactly the expected files flip tier.
  - Cross dogfood: none — rationale: no schema/ban surface (sidecar cache only).
  - Target dogfood (qbot-core): report fingerprint-cache size + tier histogram in PR body.
```

### 48-B — Change classifier + selective re-extract (`--incremental`)
SKIP/PARTIAL/FULL planner + re-parse of changed files + conservative cross-file recompute.
```
Tests:
  - Unit: classifier returns FULL on a Cargo dep change / crate add; PARTIAL on a localized edit.
  - Self dogfood: `extract --incremental` after a one-file edit re-parses only the expected files (instrumented count).
  - Cross dogfood: none — rationale: no schema change.
  - Target dogfood (qbot-core): wall-clock full vs. incremental on a single-crate change; report speedup in PR body.
```

### 48-C — `G1` byte-equivalence gate (the recall substitute)
A CI gate asserting `incremental == full` canonical dumps over a matrix of synthetic change sets.
```
Tests:
  - Self dogfood (cfdb on cfdb): for N synthetic edits (add fn, delete fn, edit body, rename, add file, delete file, touch Cargo.toml), assert sha256(incremental dump) == sha256(full dump).
  - Cross dogfood (graph-specs-rust at pinned SHA): run the same equivalence on the companion fixture; any mismatch → exit 30.
  - Unit: none — rationale: this is inherently an end-to-end equivalence property, not a pure-function assertion.
  - Target dogfood (qbot-core): one real commit's incremental dump matches its full dump; report in PR body.
```
