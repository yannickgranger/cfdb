# SOLID Verdict — Issue #43 Decomposition

**Verdict:** REQUEST CHANGES (one blocker on ISP — trait must grow to 5 before slices land; one blocker on crate home — `git2` dep discipline must be declared; all other items RATIFY)
**Reviewer:** solid-architect agent
**Date:** 2026-04-20
**Worktree:** .claude/worktrees/43-enrichment @ 1659e2a

---

## Q1 — Crate granularity

**Decision: (a) Single new `cfdb-enrich` crate for all 5 pass implementations, with implementation split point at I/O class boundary documented but NOT reflected in separate crates.**

### Evidence gathered

Current workspace members (Cargo.toml lines 3-13):

```
cfdb-core, cfdb-concepts, cfdb-query, cfdb-petgraph,
cfdb-extractor, cfdb-hir-extractor, cfdb-hir-petgraph-adapter,
cfdb-cli, cfdb-recall
```

`ls crates/ | grep enrich` → empty. No `cfdb-enrich` crate exists today.

The 5 passes and their I/O classes (RFC-cfdb-v0.2-addendum-draft.md lines 199-204):

| Pass | I/O class | Heavy dep |
|---|---|---|
| `enrich_git_history` | subprocess/fs — `git log` per `:Item` file | `git2` |
| `enrich_rfc_docs` | fs read — `.concept-graph/*.md` keyword match | none |
| `enrich_deprecation` | syn AST reuse — `#[deprecated]` extraction | `syn` (already workspace dep) |
| `enrich_bounded_context` | TOML load — `.cfdb/concepts/*.toml` + crate-prefix heuristic | `toml` (already workspace dep), `cfdb-concepts` |
| `enrich_reachability` | petgraph BFS from `:EntryPoint` over `CALLS*` | `petgraph` |

### SRP argument

SRP asks: "what is the single reason to change for each unit?" For each pass the answer is identical: "the definition of signal X and how it is materialized as attributes/edges onto `:Item` nodes changed." That is one reason. All 5 passes share that change axis. They do NOT share change axes with the AST extraction logic (which changes when syn parsing strategy changes) or with the query evaluation logic (which changes when Cypher subset evolves). CCP confirms: group things that change for the same reason. All 5 enrich passes change when enrichment policy changes. Put them together.

Option (b) — putting passes in `cfdb-petgraph` — violates SRP on that crate. `cfdb-petgraph` changes for the reason "the in-memory graph representation changes" (petgraph API, serde format, BTreeMap index). Adding enrich business logic gives it a second change axis: "enrichment policy changes." Two reasons = SRP violation. The existing Phase A empty `impl EnrichBackend for PetgraphStore {}` at `cfdb-petgraph/src/lib.rs:143` is a stub delegation point, not an invitation to put logic there.

Option (d) — 5 micro-crates — is compile-cost waste with no SOLID benefit. CCP says group by shared change axis, not atomize by I/O class. The 5 passes share the same change axis (enrichment policy). Splitting them creates 5 independent release units for what is conceptually one feature set. REP violation: the granule of reuse would be too fine — no downstream consumer will ever want `enrich_deprecation` without `enrich_bounded_context` (the classifier Cypher query in §A2.2 joins on attributes from multiple passes simultaneously).

Option (c) — git/fs/ast in one, petgraph-BFS in another — is tempting because `enrich_reachability` is architecturally distinct (it reads a complete graph, not individual `:Item` files). However: (1) it still shares the change axis with the other 4 passes; (2) CRP says don't split unless consumers actually use the sub-units independently — and no consumer needs reachability without bounded-context. Document the I/O class boundary inside `cfdb-enrich` as a module split (`enrich/git_history.rs`, `enrich/reachability.rs`) rather than a crate split.

### RFC-031 precedent

RFC-031 §2 (`cfdb-core/src/enrich.rs` module docstring, lines 1-8 and RFC-031 lines 61-91) split `EnrichBackend` out of `StoreBackend` across a trait boundary. The analogue here is: the implementations of those trait methods belong in a NEW crate, not back in `cfdb-petgraph` where only the stub delegation lives.

### SDP argument

`cfdb-enrich` instability score I = Ce/(Ca+Ce). It will have:
- Ce = outgoing: depends on `cfdb-core` (stable), `cfdb-concepts` (stable), `cfdb-petgraph` (medium stable), `git2` (external/stable), `syn` (external/stable)
- Ca = incoming: `cfdb-cli` will import it for wiring; nobody else at v0.2

I ≈ 5/(1+5) = 0.83 — highly unstable, which is correct: it is the implementation layer, the least abstract component. SDP says unstable components must depend on stable ones. `cfdb-core` (I ≈ 0, no workspace deps) is fully stable. `cfdb-concepts` (I ≈ 0.5, deps only on `serde`/`thiserror`/`toml`) is relatively stable. Arrow direction: `cfdb-enrich → cfdb-core` and `cfdb-enrich → cfdb-concepts` both point from unstable to stable. SDP satisfied.

**Conclusion: single `cfdb-enrich` crate, with internal module structure mirroring I/O class boundaries.**

---

## Q2 — ISP on `EnrichBackend`

**Decision: Keep as a single trait. Add the 5th method (`enrich_reachability`) before the first implementation slice lands. Do NOT split the trait.**

### Consumer analysis (static grep, current state)

From grep across `crates/`:

| Consumer | Calls which methods | Uses % of 4-method trait |
|---|---|---|
| `cfdb-cli/src/enrich.rs:25-28` | all 4 (via `EnrichVerb` dispatch) | 4/4 = 100% |
| `cfdb-petgraph/src/lib.rs:143` | implements all 4 (inherited stubs) | 4/4 = 100% |

Both current consumers use the full trait. ISP is satisfied today.

### Future consumer analysis (RFC §A2.2 passes table)

The 5th pass `enrich_reachability` (RFC line 204) differs architecturally — it runs after Pattern A/B extraction completes and does petgraph BFS over `CALLS*`. The RFC explicitly labels it "enricher (runs after Pattern A/B extraction completes)" whereas the other 4 are labeled "extractor". This is a sequencing distinction, not a consumer distinction. `cfdb-cli` will invoke ALL 5 passes through the same dispatch surface (the `EnrichVerb` enum at `cfdb-cli/src/enrich.rs:14`). No downstream tool at v0.2 is scoped to consume only a subset of passes.

**ISP split would be premature.** No evidence exists that any consumer will depend on a subset. The RFC does not name any consumer that needs reachability without the other 4 passes or vice versa. Splitting to a `GraphEnrichBackend` vs `ItemEnrichBackend` trait pair now is speculative ISP decomposition — the anti-pattern of over-engineering to a future that has not been identified.

**However:** the 5th method must be added to `EnrichBackend` in `cfdb-core/src/enrich.rs` as a default stub BEFORE the first implementation slice lands. If slice 43a (git history) lands with 4-method trait still in place, the trait evolves out of sync with the RFC table. Blocker.

**Trait utilization at v0.2-9 (projected):**
- `cfdb-cli` enrich handler: 5/5 = 100% — no ISP violation
- `PetgraphStore`: implements 5/5 = 100% — no ISP violation
- Any future `cfdb-query` evaluator that joins on enriched attributes: does NOT call `EnrichBackend` at all — it reads graph attributes directly via `StoreBackend::execute`. Correct separation already achieved by RFC-031 §2.

---

## Q3 — SRP per pass

**Decision: Each pass has ONE responsibility: "materialize signal X as attributes/edges onto the graph for keyspace K." The three sub-steps (source collection, signal extraction, graph mutation) are implementation details within that responsibility, not separate SRPs.**

### Argument

Robert Martin's SRP is stated at module/class level: "a module has one and only one reason to change." The question is: what counts as "one reason"?

For `enrich_git_history`: the module changes if (a) the git log invocation strategy changes (e.g. switch from subprocess to `git2` crate), (b) the attributes materialized change (add `git_author_domain`), or (c) the mutation strategy changes (batch vs per-node). All three are consequences of the SAME external decision: "how cfdb models git history on `:Item` nodes." That is one reason — a domain policy decision about the git history enrichment.

Contrast with a genuine SRP violation: if `enrich_git_history` ALSO wrote a summary report to disk or invoked `enrich_bounded_context` as a side effect. Those would be different reasons.

**Implementable-as-one-unit: YES.** Each pass slice is a self-contained unit of work: define the method signature, write the logic, add the test. No sub-decomposition is needed at the issue level. The internal structure (source → extract → mutate) is a function call sequence, not a responsibility boundary.

**Exception: `enrich_reachability` is architecturally distinct and should be a separate issue slice.** Its "source collection" is not I/O — it is a petgraph BFS traversal of the already-populated graph. It must run AFTER the other passes have written `bounded_context` and `is_deprecated` attributes (the classifier Cypher query needs all attributes present). This sequencing dependency means `enrich_reachability` has a different precondition than the other 4 passes. It should be the last slice in the decomposition order.

---

## Q4 — Stable Abstractions — cfdb-core Zone of Pain

**Discipline confirmed. `cfdb-core` must not gain implementation deps. Current state is clean.**

### `cfdb-core/Cargo.toml` dep census (lines 9-14)

```toml
[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
indexmap.workspace = true
```

Zero workspace crate dependencies. Zero I/O or OS crate dependencies. The dep set is: `{serde, serde_json, thiserror, indexmap}` — pure data/serialization.

### SAP analysis

`cfdb-core` public types (from `grep 'pub trait'` across its `src/`):
- `StoreBackend` (store.rs:62) — abstract
- `EnrichBackend` (enrich.rs:76) — abstract

Two traits, both abstract. All other pub types are data structs/enums (`Node`, `Edge`, `Query`, `EnrichReport`, `StoreError`, etc.). The crate is maximally abstract.

A = abstract_types / total_pub_types. With 2 traits and ~15 data types: A ≈ 2/17 ≈ 0.12.
I = Ce/(Ca+Ce). `cfdb-core` has Ce = 0 (no workspace deps). Ca = 7 (cfdb-concepts, cfdb-query, cfdb-petgraph, cfdb-extractor, cfdb-hir-extractor, cfdb-hir-petgraph-adapter, cfdb-cli all depend on it). I = 0/(7+0) = 0.

D = |A + I - 1| = |0.12 + 0 - 1| = 0.88. **This is deep in the Zone of Pain.**

But this is the CORRECT zone for `cfdb-core` — it is a foundational schema/types hub with no workspace deps. The Zone of Pain is only a pathology when a crate is both abstract AND has many dependents but ALSO has changeable concrete implementations. `cfdb-core` is stable-by-design: its types change only on SchemaVersion bumps (RFC-gated). High Ca + high abstraction + low instability = the intended architecture for a schema hub. This is not a violation; it is a design decision that must be protected.

**The protection rule:** any PR that adds a `git2`, `syn`, `petgraph`, `regex`, or `cargo_metadata` dep to `cfdb-core/Cargo.toml` violates this discipline and must be rejected. The 5 enrichment pass implementations MUST live in `cfdb-enrich` (not `cfdb-core`). The `EnrichBackend` trait definition stays in `cfdb-core` (abstract surface); the implementations go to `cfdb-enrich` (concrete surface).

---

## Q5 — Proposed slice decomposition

### Stability metrics table for proposed crates

| Crate | Ca (afferent) | Ce (efferent) | I=Ce/(Ca+Ce) | A | D=|A+I-1| | Zone |
|---|---|---|---|---|---|---|
| `cfdb-core` (existing) | 7 | 0 | 0.00 | 0.12 | 0.88 | Zone of Pain (intentional — stable schema hub) |
| `cfdb-enrich` (new) | 1 (cfdb-cli) | 4-5 (cfdb-core, cfdb-concepts, cfdb-petgraph, git2, syn) | ≈0.83 | 0 (all concrete) | 0.17 | Zone of Uselessness edge — ACCEPTABLE for impl crate |
| `cfdb-petgraph` (existing, modified) | 2 (cfdb-cli, cfdb-enrich) | 1 (cfdb-core) | ≈0.33 | 0 | 0.67 | Zone of Pain edge — acceptable, concrete adapter |

`cfdb-enrich` I=0.83 is correct for an implementation crate. D=0.17 is close to the main sequence — no zone concern.

### ADP cycle check

Proposed dependency graph:

```
cfdb-cli → cfdb-enrich → cfdb-petgraph → cfdb-core
cfdb-cli → cfdb-petgraph → cfdb-core
cfdb-cli → cfdb-core
cfdb-enrich → cfdb-concepts → (serde, thiserror, toml only)
cfdb-enrich → cfdb-core
```

No cycles. `cfdb-enrich` does NOT depend on `cfdb-cli`. `cfdb-petgraph` does NOT depend on `cfdb-enrich`. ADP satisfied.

**Note:** `cfdb-enrich` will depend on `cfdb-petgraph` because it needs `PetgraphStore` as its receiver (it must mutate the in-memory graph). This adds `cfdb-petgraph` as a Ce for `cfdb-enrich`, which is acceptable — it is a stable direction (petgraph store is more stable than the enrichment policies). Alternative: accept `&mut dyn EnrichTarget` (a minimal graph mutation trait). But this is implementation detail — decision for rust-systems lens.

### CCP grouping validation

All 5 passes share the same domain dependency signature:
- Import `cfdb-core::enrich::{EnrichBackend, EnrichReport}` — trait and return type
- Import `cfdb-core::schema::Keyspace` — parameter type
- Import `cfdb-core::store::StoreError` — error type
- Import `cfdb-core::fact::{Node, PropValue}` — graph mutation primitives

They change together when: (1) `EnrichReport` shape changes, (2) `Keyspace` semantics change, (3) a new attribute kind is added to the graph vocabulary. CCP is satisfied: they belong in one crate.

### CRP violation matrix

| Consumer | `EnrichBackend` methods used | Trait utilization |
|---|---|---|
| `cfdb-cli/src/enrich.rs` | all 5 (after 5th added) | 5/5 = 100% |
| `cfdb-petgraph/src/lib.rs` (stub impl) | inherits all 5 | 5/5 = 100% |
| `cfdb-recall` | 0 — never calls EnrichBackend | N/A — depends on cfdb-extractor, not enrich |
| `cfdb-query` | 0 — reads graph attrs via StoreBackend | N/A |

CRP: no consumer is forced to depend on trait methods it does not use. The CFR threshold (below 25% = violation) is not triggered by any consumer.

### ISP improvement quantification

Before RFC-031 §2 (historical, from RFC-031 lines 80-83):
- `cfdb-cli/src/enrich.rs` utilization of `StoreBackend`: 4/11 = 36%

After RFC-031 §2 (current state, `cfdb-core/src/enrich.rs:76`):
- `cfdb-cli/src/enrich.rs` utilization of `EnrichBackend`: 4/4 = 100%
- `cfdb-cli/src/commands.rs` utilization of `StoreBackend`: 7/7 = 100% (enrich methods removed)

The ISP split already happened and is complete. Issue #43 must not regress it.

### SDP direction check

All proposed dependency arrows point from unstable (high I) to stable (low I):

- `cfdb-enrich` (I≈0.83) → `cfdb-core` (I=0) — CORRECT direction
- `cfdb-enrich` (I≈0.83) → `cfdb-concepts` (I≈0.5) — CORRECT direction
- `cfdb-enrich` (I≈0.83) → `cfdb-petgraph` (I≈0.33) — CORRECT direction (enrich is less stable than petgraph)
- `cfdb-cli` (I≈0.9) → `cfdb-enrich` (I≈0.83) — CORRECT direction (cli is least stable)

No SDP violations in proposed graph.

### Issue slice decomposition

**Prerequisite — Slice 43-0: Extend `EnrichBackend` to 5 methods (BLOCKER)**

- Crate: `cfdb-core` only — `src/enrich.rs`
- Responsibility: add `enrich_reachability` as a 5th default stub returning `EnrichReport::not_implemented("enrich_reachability")`; add `enrich_reachability` variant to `EnrichVerb` enum in `cfdb-cli/src/enrich.rs`
- Consumers: all — this is a mechanical extension of the existing pattern
- Compile-cost: negligible — no new deps, 1 method addition
- Tests:
  - Unit: extend `cfdb-core/src/enrich.rs` tests — `not_implemented_marks_pass_as_unran` pattern for `enrich_reachability`
  - Self dogfood: `cfdb enrich-reachability --db .cfdb/db --keyspace cfdb` must exit 0 and emit JSON with `ran: false`
  - Cross dogfood: zero findings on graph-specs-rust (no ban rule impact)
  - Target dogfood: none — rationale: stub only, no attribute emitted

**Slice 43-1: `enrich_bounded_context` — TOML + crate-prefix heuristic**

- Crate: creates `cfdb-enrich` (new crate); implements `enrich_bounded_context` in `cfdb-enrich/src/bounded_context.rs`
- Responsibility: materialize `:Item.bounded_context` attribute and `(:Crate)-[:BELONGS_TO]->(:Context)` edges via `cfdb-concepts::ContextMap` lookup
- Deps added to new crate: `cfdb-core`, `cfdb-concepts`, `cfdb-petgraph` (to mutate graph)
- Consumers: `cfdb-cli` wired through `PetgraphStore`-backed `EnrichBackend::enrich_bounded_context`
- Compile-cost: creating the new crate adds ~2-5s cold (no heavy deps); `cfdb-concepts` is already compiled as `cfdb-extractor` dep
- Rationale for first: `enrich_reachability` (slice 43-5) joins on `bounded_context` in the classifier — this must land first
- Tests:
  - Unit: fixture workspace with 2 crates, verify `:Item.bounded_context` = crate prefix; override via `.cfdb/concepts/*.toml` overrides `cfdb-concepts`
  - Self dogfood: `cfdb enrich-bounded-context --db .cfdb/db --keyspace cfdb` → assert `cfdb-core` items carry `bounded_context: "cfdb-core"`, `cfdb-cli` items carry `bounded_context: "cfdb-cli"`; count must be ≥ 80% of total `:Item` nodes
  - Cross dogfood: `enrich-bounded-context` on graph-specs-rust at pinned SHA → zero ban-rule findings; attribute coverage ≥ 80%
  - Target dogfood: report `bounded_context` attribute coverage % in PR body

**Slice 43-2: `enrich_deprecation` — syn AST walk**

- Crate: `cfdb-enrich/src/deprecation.rs` (extends crate from slice 43-1)
- Responsibility: materialize `:Item.is_deprecated` (bool) and `:Item.deprecation_since` (Option<String>) from `#[deprecated]` syn attribute
- Deps: `syn` (already workspace dep), `cfdb-core`, `cfdb-extractor` (reuse existing AST walk infrastructure)
- Consumers: classifier Cypher query joins on `is_deprecated` for "unfinished_refactor" class detection
- Compile-cost: `syn` already in dep tree via `cfdb-extractor`; marginal cost ~0s
- Tests:
  - Unit: fixture with `#[deprecated(since = "0.2.0", note = "...")]` struct → assert `is_deprecated: true`, `deprecation_since: "0.2.0"`; fixture with no attribute → assert `is_deprecated: false`
  - Self dogfood: run on cfdb's own tree; assert any intentionally deprecated items carry the attribute (can be 0 if none exist — must produce a count in output)
  - Cross dogfood: zero ban-rule findings on graph-specs-rust
  - Target dogfood: report count of deprecated items in qbot-core (informational, no merge gate)

**Slice 43-3: `enrich_rfc_docs` — filesystem markdown scan**

- Crate: `cfdb-enrich/src/rfc_docs.rs`
- Responsibility: materialize `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` edges by keyword-matching concept names from `:Item` nodes against RFC markdown files in `.concept-graph/` and `docs/rfc/*.md`
- Deps: stdlib `std::fs` only — no new external deps
- Consumers: classifier joins on `has_rfc_ref` for "unfinished_refactor" class
- Compile-cost: stdlib only — zero marginal cost
- Tests:
  - Unit: fixture with a mock RFC directory containing one `.md` file with a keyword matching one `:Item` qname; assert `REFERENCED_BY` edge emitted
  - Self dogfood: run on cfdb's own tree against `docs/RFC-031-audit-cleanup.md`; assert `EnrichBackend` `:Item` carries at least one `REFERENCED_BY` edge to that RFC doc
  - Cross dogfood: zero ban-rule findings
  - Target dogfood: report RFC reference coverage % in PR body

**Slice 43-4: `enrich_git_history` — git2 I/O**

- Crate: `cfdb-enrich/src/git_history.rs`
- Responsibility: materialize `:Item.git_age_days`, `:Item.git_last_author`, `:Item.git_commit_count` by walking git history for each `:Item`'s defining file via `git2` crate
- Deps: `git2` (new dep — **must be added to workspace `[workspace.dependencies]` in same PR**)
- Consumers: classifier joins on `git_age_days` for `age_delta` computation
- Compile-cost: `git2` links libgit2 via `cc` + `pkg-config`. Cold compile cost estimate: 15-25s added to `cfdb-enrich` build (libgit2 is C, ~30k LoC compiled through bindgen/cc). This is acceptable because `git2` is gated inside `cfdb-enrich` and does NOT propagate to `cfdb-core`, `cfdb-cli` default build, or `cfdb-recall`. Verify with `cargo tree -p cfdb-cli --depth 5` after wiring — `git2` must NOT appear there unless explicitly feature-gated.
- SOLID flag: `git2` in `cfdb-enrich` violates CRP if any downstream consumer of `cfdb-enrich` needs zero-git-dep builds. Mitigate with a `git` feature flag: `cfdb-enrich = { path = "../cfdb-enrich", features = ["git"] }` in `cfdb-cli/Cargo.toml`. Default build of `cfdb-enrich` without `git` feature compiles the stub. This preserves the ISP discipline from RFC-031.
- Tests:
  - Unit: git fixture — `git2::Repository::init` in tempdir, commit a file, extract `:Item` nodes from that file, run `enrich_git_history`; assert `git_age_days ≥ 0`, `git_last_author` non-empty, `git_commit_count ≥ 1`
  - Self dogfood: run on cfdb's own workspace git history; assert ≥ 90% of `:Item` nodes carry `git_age_days`; report mean/max age in PR body
  - Cross dogfood: zero ban-rule findings on graph-specs-rust
  - Target dogfood: report git_age_days distribution in qbot-core PR body

**Slice 43-5: `enrich_reachability` — petgraph BFS (LAST — depends on 43-1 through 43-4)**

- Crate: `cfdb-enrich/src/reachability.rs`
- Responsibility: materialize `:Item.reachable_from_entry` (bool) and `:Item.reachable_entry_count` (u32) by BFS from `:EntryPoint` nodes over `CALLS*` edges in the petgraph store
- Deps: `petgraph` (already in `cfdb-petgraph` dep; `cfdb-enrich` already depends on `cfdb-petgraph`)
- Ordering constraint: must run after all other passes AND after Pattern A/B extraction populates `CALLS` edges; the CLI `enrich-reachability` subcommand must document this precondition
- Consumers: classifier joins on `reachable_from_entry` for "unwired" class detection
- Compile-cost: petgraph already compiled — zero marginal cost
- Tests:
  - Unit: construct a `PetgraphStore` with 2 `:Item` nodes and 1 `CALLS` edge, add 1 `:EntryPoint` node; run `enrich_reachability`; assert the reachable item carries `reachable_from_entry: true`, the unreachable item carries `reachable_from_entry: false`
  - Self dogfood: `cfdb enrich-reachability` on cfdb's own keyspace (requires prior `cfdb extract` + HIR extraction for CALLS edges — document this as a precondition; v0.2 initial run may produce low coverage if HIR is not yet default)
  - Cross dogfood: zero ban-rule findings
  - Target dogfood: report reachability % on qbot-core in PR body

---

## Blockers to RATIFY

1. **BLOCKER-1 (ISP / trait completeness):** `cfdb-core/src/enrich.rs` currently declares 4 methods (`enrich_docs`, `enrich_metrics`, `enrich_history`, `enrich_concepts`). The RFC §A2.2 passes table lists 5 passes including `enrich_reachability`. The 5th method MUST be added as a default stub (Slice 43-0) before any implementation slice lands. If implementation slices land with a 4-method trait, the trait evolves out of phase with the RFC contract and forces a breaking change mid-sequence. File Slice 43-0 as a prerequisite issue and block 43-1 through 43-5 on its merge.

2. **BLOCKER-2 (`git2` dep discipline):** The `git2` crate links libgit2 (C library, ~15-25s cold compile cost). If added unconditionally to `cfdb-enrich`, it propagates to every consumer including `cfdb-cli` default builds. The `git` feature flag pattern used by `cfdb-hir-extractor` (cfdb-cli/Cargo.toml lines 30-36) is the established precedent. Slice 43-4 MUST gate `git2` behind a `cfdb-enrich/git` feature. The PR must include a `cargo tree -p cfdb-cli --depth 5` snapshot showing `git2` absent from the default build tree.

Non-blocking observations (REQUEST CHANGES, not REJECT):

3. **`enrich_docs` and `enrich_metrics` are not in the RFC §A2.2 passes table.** The current 4-method trait includes `enrich_docs` and `enrich_metrics` as Phase A stubs. The RFC §A2.2 table lists 5 passes but none named exactly `enrich_docs` or `enrich_metrics`. `enrich_rfc_docs` is the closest match for `enrich_docs`. This naming misalignment should be resolved in Slice 43-0: either rename the stub methods or document the mapping. Do not carry the misalignment into implementation slices.

4. **Slice ordering is a hard dependency.** The decomposition order must be enforced in the issue tracker: 43-0 → 43-1 → 43-2 → 43-3 → 43-4 → 43-5. The classifier Cypher query (RFC §A2.2) joins on attributes from ALL passes simultaneously. Partial enrichment produces meaningless classifier output. Each slice should carry an explicit "Depends-On:" field in its issue body.

---

## References

- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/enrich.rs` — EnrichBackend trait (lines 76-108), EnrichReport (lines 26-65)
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/Cargo.toml` — dep census (lines 9-14)
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-petgraph/src/lib.rs` — stub impl at line 143; StoreBackend impl lines 91-137
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-cli/src/enrich.rs` — consumer analysis (lines 14-33)
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-cli/Cargo.toml` — feature flag precedent (lines 30-36)
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/docs/RFC-031-audit-cleanup.md` — ISP split rationale (lines 61-91), ISP violation quantification (lines 80-87)
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/docs/RFC-cfdb-v0.2-addendum-draft.md` — 5-pass table (lines 198-204), classifier Cypher (lines 208-232)
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/Cargo.toml` — workspace members (lines 3-13)
