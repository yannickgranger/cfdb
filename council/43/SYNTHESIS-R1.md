# Synthesis R1 — Issue #43 Enrichment Framework Decomposition

**Date:** 2026-04-20
**Worktree:** `.claude/worktrees/43-enrichment @ 1659e2a`
**Council round:** 1
**Input verdicts:** `council/43/{clean-arch,ddd,solid,rust-systems}.md`

## Overall verdict

**REQUEST CHANGES** — 3 of 4 lenses. Rust-systems alone RATIFIED with 5 conditions.
All 4 agree on the substance; the conflicts are reconcilable and resolved below.

## Consensus (all 4 lenses agree)

1. **Prerequisite slice is mandatory.** No implementation slice can land before the trait vocab is aligned to the RFC (clean-arch 43-F, SOLID 43-0, DDD Slice A — same intent, different labels).
2. **`enrich_reachability` must be added to `EnrichBackend`.** 5th method currently missing (`cfdb-core/src/enrich.rs:76–108`).
3. **`enrich_metrics` is out of scope for #43.** It has no counterpart in RFC §A2.2; it is a quality-metrics concern orthogonal to the debt-cause classifier pipeline. Keep the Phase A stub; defer real impl to a separate issue.
4. **`git2` must be feature-gated.** +20–35s cold build is unacceptable for users who don't need git-history signals (SOLID Blocker-2, rust-systems Q6).
5. **`:RfcDoc` and `REFERENCED_BY` are NEW schema.** Neither is reserved in `labels.rs`; both require a `SchemaVersion` patch bump + lockstep PR on `graph-specs-rust` per cfdb CLAUDE.md §3.
6. **Reachability depends on `:EntryPoint` nodes.** `#99` (remaining EntryPoint kinds) must produce meaningful fixtures; `#86` already ships `cli_command` + `mcp_tool` kinds.
7. **Determinism discipline:** BTreeMap for node collection, sorted iteration, stable commit-timestamp keys.

## Resolved conflicts

### C1 — Where do pass impls live?

**Winner: clean-arch.** Impls live on `PetgraphStore` inside `cfdb-petgraph`; the port (`EnrichBackend`) stays in `cfdb-core`; no new `cfdb-enrich` crate.

**Rationale:** A new crate would need to reach into `PetgraphStore`'s internal `KeyspaceState` + `StableDiGraph` — a dependency-rule inversion. SOLID's SRP concern ("`cfdb-petgraph`'s SRP is graph storage, not enrichment policy") is answered by: enrichment passes ARE a graph-mutation concern; they belong where the graph lives. Clean-arch Q1 + Q4 cite the reasoning; compose.rs:1–7 is already the composition root.

**Escape hatch:** If a pass grows heavy (rust-systems flagged git2 at +20–35s), its impl body can be split into a sub-module (`cfdb-petgraph/src/enrich/git_history.rs`). Module split ≠ crate split. CCP-clean.

### C2 — 5 passes or 6?

**Winner: DDD.** Six passes. The RFC §A2.2 table has a gap: it omits `:Concept` node materialization which issues #101/#102 block on.

**The 6 passes:**

| # | Method | Writes | Notes |
|---|---|---|---|
| 1 | `enrich_git_history` | `:Item.git_last_commit_unix_ts`, `git_last_author`, `git_commit_count` | renamed from `enrich_history`; stores epoch, not days (C4) |
| 2 | `enrich_rfc_docs` | `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` + `:RfcDoc {path, title}` | renamed from `enrich_docs`; scope narrowed (C3) |
| 3 | `enrich_deprecation` | `:Item.is_deprecated`, `:Item.deprecation_since` | NEW method; provenance TBD (C5) |
| 4 | `enrich_bounded_context` | `:Item.bounded_context` (re-enrichment only) | NEW method; may be no-op (C6) |
| 5 | `enrich_concepts` | `:Concept {name, assigned_by}` + `LABELED_AS` + `CANONICAL_FOR` | renamed scope; blocks #101/#102 |
| 6 | `enrich_reachability` | `:Item.reachable_from_entry`, `:Item.reachable_entry_count` | NEW method; BFS from `:EntryPoint` |

`enrich_metrics` retained as Phase A stub, deferred.

### C3 — RFC amendment required?

**Yes.** DDD identified 5 blockers that require RFC updates:
1. `enrich_docs` → `enrich_rfc_docs` scope narrowing (drops rustdoc enrichment)
2. Add 6th row `enrich_concepts` to §A2.2 pass table
3. Declare `SchemaVersion` patch bump policy (batched vs per-slice)
4. Clarify `enrich_deprecation` provenance (extractor-time vs enrichment-time — RFC says "reuses existing AST walk" implying extractor)
5. Name the v0.2-9 gate as load-bearing invariant: Stage 2 classifier (issue #48) must NOT deploy until `enrich_bounded_context` hits ≥95% accuracy

**The amendment lands inside Slice 43-A** (not a separate RFC cycle). Updating the `-draft.md` file IS the amendment; the RFC-first rule is satisfied because the file exists and is being refined before ratification.

### C4 — `git_age_days` vs `git_last_commit_unix_ts`

**Winner: clean-arch.** Store `git_last_commit_unix_ts` (i64 epoch seconds). Compute `age_delta` in the classifier Cypher at query time.

**Rationale:** `git_age_days` computed at enrich time is calendar-relative; two runs on different days produce different canonical dumps, violating G1 determinism (`cfdb-core/src/store.rs:53–55`). RFC addendum line 216 (`age_delta = abs(a.git_age_days - b.git_age_days)`) can be rewritten as `abs(a.ts - b.ts) / 86400` trivially.

### C5 — Deprecation: extractor-time or enrichment-time?

**Unresolved by council round 1. Route to RFC amendment decision in Slice 43-A.**

DDD + rust-systems both lean extractor-time ("no new I/O; reuses existing AST walk" per RFC §A2.2). If extractor-time: `enrich_deprecation` is a no-op on `PetgraphStore` and the real work is in `cfdb-extractor/src/attrs.rs` + `item_visitor.rs`. If enrichment-time: pass does graph walk + attr write.

**Recommendation for Slice 43-A:** adopt extractor-time. Keep `enrich_deprecation` on the trait as a `not_implemented` stub for symmetry with the other passes (so the CLI verb exists and returns `ran: false`). Move real implementation into an extractor extension slice (43-C').

### C6 — `enrich_bounded_context` may be a no-op

DDD Q2 + rust-systems Blocker-1 both note: `bounded_context` is ALREADY written at extraction time (`cfdb-extractor/src/lib.rs:105–142`). The Phase D pass serves workspaces where `.cfdb/concepts/*.toml` changed between extractions — it re-reads TOML and patches.

**Slice 43-E scope:** "re-enrichment only" — pass walks `:Crate` nodes, checks current `bounded_context` against fresh TOML read, patches mismatches. If extraction is fresh, pass returns `attrs_written = 0, ran = true`. The v0.2-9 ≥95% accuracy gate still applies because it asserts the final graph state, regardless of which stage wrote the attribute.

## Final slice list (7 slices)

### Slice 43-A — RFC amendment + trait rename/additions + schema reservations (PREREQUISITE)

**Scope:**
- Amend `docs/RFC-cfdb-v0.2-addendum-draft.md` §A2.2: 6-row pass table, scope narrowing, schema version bump policy, deprecation provenance decision, v0.2-9 classifier-gate invariant
- `cfdb-core/src/enrich.rs`: rename `enrich_history` → `enrich_git_history`, `enrich_docs` → `enrich_rfc_docs`; add stubs for `enrich_bounded_context`, `enrich_deprecation`, `enrich_reachability`; keep `enrich_concepts` (re-scoped); keep `enrich_metrics` stub (deferred, out of scope)
- `cfdb-core/src/schema/labels.rs`: add `Label::RFC_DOC` + `EdgeLabel::REFERENCED_BY`
- `cfdb-core/src/schema/describe.rs`: descriptors for `:RfcDoc`, `REFERENCED_BY`, new `:Item` attrs
- `cfdb-cli/src/enrich.rs`: rename `EnrichVerb` variants; add new CLI subcommands
- `crates/cfdb-cli/tests/wire_form_*.rs`: update verb list
- Store workspace path on `PetgraphStore` at construction (clean-arch B4 resolution)
- **NO SchemaVersion bump in this slice** — stubs return `ran: false`; bump lands with first impl slice that writes real data
- Cross-dogfood: zero violations expected (mechanical rename + stubs)

**Depends on:** none
**Blocks:** 43-B through 43-G
**Tests:** Unit (new stub round-trip) + Self dogfood (5 new CLI verbs each return `ran: false` JSON) + Cross/Target: none (mechanical)

---

### Slice 43-B — `enrich_git_history` impl

**Scope:** `PetgraphStore::enrich_git_history`; walk `:Item` nodes; for each, resolve `file` attr; use `git2` (feature `git-enrich` on `cfdb-petgraph` OR `cfdb-extractor` per RFC-032 precedent); store `git_last_commit_unix_ts` (i64), `git_last_author` (str), `git_commit_count` (i64). SchemaVersion bump to V0_2_1. Lockstep graph-specs-rust PR.

**Depends on:** 43-A
**Parallel with:** 43-C, 43-D, 43-E, 43-F, 43-G (after 43-A lands)
**Tests:** Unit (git fixture), Self dogfood (≥80% of items have non-null ts on cfdb tree), Cross dogfood (schema bump + zero violations), Target dogfood (top-10 churn items in qbot-core PR body)

---

### Slice 43-C — `enrich_deprecation` extractor extension

**Scope:** Route per 43-A RFC decision. If extractor-time: new `extract_deprecated_attr` in `cfdb-extractor/src/attrs.rs` + call sites in `item_visitor.rs` `emit_item_with_flags`; `PetgraphStore::enrich_deprecation` stays a no-op. If enrichment-time: real impl in pass. Either way: writes `:Item.is_deprecated`, `:Item.deprecation_since`.

**Depends on:** 43-A
**Parallel with:** 43-B, 43-D, 43-E, 43-F, 43-G
**Tests:** Unit (3 `#[deprecated]` variant forms), Self dogfood (0 deprecated items in cfdb — negative-case regression), Cross dogfood (schema bump), Target dogfood (deprecated count in qbot-core)

---

### Slice 43-D — `enrich_rfc_docs` impl

**Scope:** `PetgraphStore::enrich_rfc_docs`; read `.concept-graph/*.md` + `docs/rfc/*.md` at pass time via stored workspace path; `str::contains` scan (rust-systems Q2: `aho-corasick` is already transitively available but naive is <10ms for <500 concepts); emit `:RfcDoc` nodes + `REFERENCED_BY` edges.

**Depends on:** 43-A
**Parallel with:** 43-B, 43-C, 43-E, 43-F, 43-G
**Tests:** Unit (synthetic RFC with known item name), Self dogfood (cfdb's own `StoreBackend`/`EnrichBackend` appear via `REFERENCED_BY` edges; `edges_written > 0`), Cross dogfood (schema bump), Target dogfood (item-RFC-reference count)

---

### Slice 43-E — `enrich_bounded_context` re-enrichment + v0.2-9 gate

**Scope:** `PetgraphStore::enrich_bounded_context`; re-read `.cfdb/concepts/*.toml`; patch `:Item.bounded_context` where TOML has changed since extraction. If no TOML changes: `attrs_written = 0, ran = true`. **v0.2-9 ≥95% accuracy gate BLOCKS merge.** Declared load-bearing for classifier Stage 2 (#48).

**Depends on:** 43-A
**Parallel with:** 43-B, 43-C, 43-D, 43-G (but NOT 43-F — see below)
**Tests:** Unit (TOML override fixture), Self dogfood (all cfdb items have non-null `bounded_context`), Cross dogfood (zero violations), Target dogfood (v0.2-9 accuracy report — ≥95% on ground-truth crates `domain-strategy` / `ports-trading` / `qbot-mcp` — PR body)

---

### Slice 43-F — `enrich_concepts` (`:Concept` materialization — blocks #101/#102)

**Scope:** `PetgraphStore::enrich_concepts`; for each `.cfdb/concepts/<name>.toml`, emit one `:Concept {name, assigned_by: "manual"}` node; emit `LABELED_AS` edges to items in the context's crates; emit `CANONICAL_FOR` edges to items declared canonical in TOML. No SchemaVersion bump (labels already reserved: `labels.rs:44,97–99`, `describe.rs:138–145`).

**Depends on:** 43-A + 43-E (needs accurate `bounded_context` to map crate → concept)
**Parallel with:** 43-B, 43-C, 43-D, 43-G
**Blocks:** #101 (T1 concept-unwired), #102 (T3 concept-multi-crate)
**Tests:** Unit (synthetic TOML → :Concept + LABELED_AS count), Self dogfood (cfdb has no `.cfdb/concepts/*.toml` for itself → 0 concepts, `ran = true`), Cross dogfood (companion concept count), Target dogfood (qbot-core concept + LABELED_AS counts as #101 prereq metric)

---

### Slice 43-G — `enrich_reachability` + degraded path

**Scope:** `PetgraphStore::enrich_reachability`; `petgraph::visit::Bfs` with collectively-seeded frontier (all `:EntryPoint` targets via `EXPOSES`); single sweep for `reachable_from_entry`, per-entry-point sweep for `reachable_entry_count`; `FixedBitSet` visit map for O(V+E). **Degraded path:** if zero `:EntryPoint` nodes, return `ran = false` + warning. **Seed set sorted by node id** for determinism.

**Depends on:** 43-A; operationally depends on `:EntryPoint` coverage (#86 ships 2 kinds; #99 adds 3 more for v0.2-1 ≥95% recall)
**Parallel with:** 43-B, 43-C, 43-D, 43-E, 43-F
**Tests:** Unit (3-node fixture + degraded-empty case), Self dogfood (extract with `--features hir`, then enrich-reachability, count unreachable), Cross dogfood (schema bump), Target dogfood (qbot-core unreachable % with HIR caveat)

---

## Dependency graph

```
43-A (RFC amend + trait rename + schema reservations) — BLOCKS all
 ├─ 43-B (enrich_git_history)           ── git2 + feature-gate + SchemaVersion bump
 ├─ 43-C (enrich_deprecation)            ── extractor-time (per 43-A RFC decision)
 ├─ 43-D (enrich_rfc_docs)               ── :RfcDoc + REFERENCED_BY schema
 ├─ 43-E (enrich_bounded_context)        ── v0.2-9 ≥95% gate BLOCKS merge
 │   └─ 43-F (enrich_concepts)           ── :Concept materialization; blocks #101/#102
 └─ 43-G (enrich_reachability)           ── degraded path for empty :EntryPoint set
```

## Invariants (landed in RFC amendment in 43-A)

- **I1.** `cfdb-core` remains dep-free of `git2`, `petgraph`, `syn`, `cargo_metadata`. Port traits only.
- **I2.** `EnrichBackend` signatures use only `cfdb-core` types (`&Keyspace`, `Result<EnrichReport, StoreError>`).
- **I3.** Each pass is a pure function over prior graph state; no inter-pass enrichment dependency. Reachability depends on extraction (`:EntryPoint` + `CALLS`), not on other enrichment.
- **I4.** All pass writes go through BTreeMap/sorted collection before canonical dump; G1 byte-stability preserved.
- **I5.** `git_last_commit_unix_ts` stored as i64 epoch; age computed in Cypher (not baked).
- **I6.** Classifier Stage 2 (issue #48) BLOCKED until `enrich_bounded_context` v0.2-9 ≥95% gate passes.
- **I7.** SchemaVersion bumps coordinate lockstep graph-specs-rust PRs per cfdb CLAUDE.md §3.

## Blockers resolved / remaining

| Source | Blocker | Resolution |
|---|---|---|
| clean-arch B1 | port vocab split-brain | 43-A mandatory prereq ✅ |
| clean-arch B2 | `git_age_days` determinism | 43-B stores `git_last_commit_unix_ts` ✅ |
| clean-arch B3 | reachability degraded path | 43-G explicit `ran: false` + warning ✅ |
| clean-arch B4 | workspace path threading | 43-A stores path on `PetgraphStore` ✅ |
| SOLID Blocker-1 | 5th method missing | 43-A adds all 3 missing stubs ✅ |
| SOLID Blocker-2 | git2 feature flag | 43-B gates behind `git-enrich` ✅ |
| DDD Blocker-1 | RFC amendment | 43-A amends draft in-place ✅ |
| DDD Blocker-2 | SchemaVersion policy | 43-A declares per-slice bumps (not batched) ✅ |
| DDD Blocker-3 | `bounded_context` duplication | 43-E scoped as re-enrichment only ✅ |
| DDD Blocker-4 | classifier confidence-gating | 43-A adds I6 invariant to RFC ✅ |
| DDD Blocker-5 | deprecation provenance | 43-A decides extractor-time; 43-C routes accordingly ✅ |
| rust-systems conditions 1–5 | determinism script extension, D3/#41 sequencing | Captured in 43-A + 43-G ✅ |

**Zero unresolved blockers.** All lenses ratifiable after 43-A lands.

## Child issue file map

All 7 slices become forge issues under `agency:yg/cfdb`, linked back to #43 as parent EPIC. Each carries the `Tests:` block per cfdb CLAUDE.md §2.5.
