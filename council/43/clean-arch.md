# Clean-Arch Verdict — Issue #43 Decomposition

**Verdict:** REQUEST CHANGES
**Reviewer:** clean-arch agent
**Date:** 2026-04-20

---

## Q1 — Composition root

The composition root is already correctly located: `crates/cfdb-cli/src/compose.rs` is the single place in the CLI that constructs a `PetgraphStore` and wires adapters (compose.rs:1–7, self-described: "single place in cfdb-cli that knows which concrete StoreBackend is wired in"). Handler modules — including `enrich.rs` — must not call `PetgraphStore::new()` directly; they must go through `compose::load_store` or `compose::empty_store`.

The current enrichment dispatch in `cfdb-cli/src/enrich.rs` (lines 21–33) already obeys this: it calls `compose::load_store`, then dispatches via `EnrichVerb`. This pattern MUST be preserved for all 5 passes.

**Dependency direction is clean today and must remain so:**

```
cfdb-cli  →  cfdb-petgraph  →  cfdb-core  (inner)
cfdb-cli  →  cfdb-core
cfdb-petgraph  ↛  cfdb-cli   (correct: no upward import)
```

Introducing a `cfdb-enrich` crate is a REQUEST CHANGES blocker — see Q2 and Blockers section. All enrichment pass implementations belong on `PetgraphStore` inside `cfdb-petgraph`, with `EnrichBackend` staying in `cfdb-core` as the port. No new crate is warranted: the composition root (cfdb-cli/compose.rs) does not change, only the concrete methods on `PetgraphStore` gain real implementations replacing the Phase A stubs.

---

## Q2 — Trait purity and 5-pass count drift

**Current state:** `EnrichBackend` in `crates/cfdb-core/src/enrich.rs` (lines 76–108) declares 4 methods: `enrich_docs`, `enrich_metrics`, `enrich_history`, `enrich_concepts`. RFC §A2.2 (addendum, lines 196–204) specifies 5 passes: `git_history`, `rfc_docs`, `deprecation`, `bounded_context`, `reachability`. There is a 4-vs-5 mismatch and a vocabulary drift.

**Decision: Option (c) with a targeted rename — replace the 4 generic methods with 5 named-pass methods, each implementing exactly one RFC pass.**

Rationale against the alternatives:

- Option (a) "add methods, keep 4" leaves `enrich_metrics` in place with no corresponding RFC pass. `enrich_docs` conflates `git_history` (subprocess I/O via git2) and `rfc_docs` (filesystem read of `.concept-graph/`), two passes with different I/O profiles and different failure modes. Conflation violates SRP at the port level.
- Option (b) "fold deprecation into concepts" introduces a second ISP violation: `enrich_bounded_context` (deterministic, declarative, crate-prefix heuristic) and `enrich_deprecation` (syn AST attribute walk) have orthogonal inputs and different read boundaries. Folding them forces a re-run of bounded-context logic whenever deprecation data is refreshed. The determinism invariant (RFC §12 G1, store.rs:53) makes this ordering fragile.
- The RFC's BLOCK-1 (addendum line 194) is explicit: enrichment passes materialize signals independently before the Cypher stage joins them. "Independently" means each pass is a pure function over its own input domain. Conflating passes at the port level is an architectural regression against BLOCK-1.

**Concrete port change required:**

`EnrichBackend` in `cfdb-core/src/enrich.rs` must expose 5 methods matching the RFC pass names:

```
enrich_git_history(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>
enrich_rfc_docs(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>
enrich_deprecation(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>
enrich_bounded_context(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>
enrich_reachability(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>
```

Each retains a default stub returning `EnrichReport::not_implemented(verb)` so any future second backend (LadybugStore, etc.) compiles without implementing all 5 immediately.

The CLI `EnrichVerb` enum in `cfdb-cli/src/enrich.rs` (lines 14–19) must be updated to match the 5-pass vocabulary — `Docs`, `Metrics`, `History`, `Concepts` become `GitHistory`, `RfcDocs`, `Deprecation`, `BoundedContext`, `Reachability`.

**Port purity check:** The `EnrichBackend` signature is currently clean — only `cfdb_core` types appear: `&Keyspace` (crate::schema), `Result<EnrichReport, StoreError>` (both in cfdb-core). No sqlx, tokio, reqwest, git2, or petgraph types appear in the trait signatures. This MUST remain true after the rename. The git2 crate and petgraph internals belong inside `PetgraphStore::enrich_git_history` impl, never in the trait signature.

---

## Q3 — Determinism invariant

RFC §12 G1 (store.rs:53–55): "same input facts + same schema version → byte-identical canonical dump." G2 (line 56): "`execute` is read-only. No query may mutate the graph."

The enrichment passes on `EnrichBackend` take `&mut self`, meaning they mutate graph state. This is intentional and correct: enrichment is an additive-write phase, canonicalization is a read phase. The determinism invariant applies to the final canonical dump after enrichment completes, not to enrichment in progress.

**Read-boundary constraint that IS load-bearing for decomposition:** RFC §A2.2 (addendum line 194) states "Stage 1 enrichment passes materialize signals into the graph AS NEW EDGES/ATTRIBUTES, THEN a Cypher query joins on the enriched graph." The words "THEN" enforce a hard ordering: no pass may read enriched attributes written by another pass during the same pipeline run, except `enrich_reachability` which explicitly depends on the prior extraction being complete (it BFS-walks existing `CALLS*` edges, not the enrichment outputs of other passes in this batch).

The only inter-pass dependency in the RFC's table (addendum lines 198–204) is: `enrich_reachability` requires that `:EntryPoint` nodes and `CALLS` edges already exist — these are produced by the extraction stage (cfdb-extractor, not enrichment). No enrichment pass depends on another enrichment pass's output in the same pipeline run.

**Ordering constraint for decomposition:** `enrich_reachability` MUST run last (or at least after extraction is complete). The other 4 passes (`git_history`, `rfc_docs`, `deprecation`, `bounded_context`) are mutually independent and can be implemented and tested in any order. This independence enables parallel child issues.

**Determinism in individual passes:** `enrich_git_history` uses `git2` (addendum line 200 — "uses git2 crate to avoid subprocess overhead"). Git log output is deterministic against a fixed commit history, but the absolute timestamp `git_age_days` is calendar-relative. If this attribute is stored as an integer age computed at enrichment time (not as a commit timestamp), two runs on the same repo on different days will produce different values. This is a design question the implementer must resolve: store as `last_commit_unix_ts` (fully deterministic, reproducible) or as `git_age_days` (requires a fixed reference date for determinism). This council's lens recommends `last_commit_unix_ts` to preserve G1.

---

## Q4 — Reachability pass host

**Decision: behind the `EnrichBackend` port in `cfdb-petgraph`, NOT as a direct petgraph eval operation called by the CLI.**

The RFC §A2.2 table (addendum line 204) locates `enrich_reachability` in the "enricher" layer and explicitly distinguishes it from the "extractor" layer used by the other 4 passes. It is still a Stage 1 pass (materializes signals) not a Stage 2 query.

The argument for tight petgraph coupling is real: BFS over a `StableDiGraph` is efficient with direct `petgraph::visit` traversal; going through the Cypher evaluator (`StoreBackend::execute`) for a BFS query adds parse overhead and requires the Cypher evaluator to handle variable-length traversal correctly at scale.

However, implementing BFS inside `PetgraphStore::enrich_reachability` using internal `petgraph::visit` APIs is NOT a violation of port separation. The `EnrichBackend::enrich_reachability` method signature remains clean (only `&Keyspace` and `Result<EnrichReport, StoreError>`). The BFS logic lives inside the impl block, behind the port boundary. External callers — the CLI, future skills — call `backend.enrich_reachability(&ks)` and see only `EnrichReport`. The internal graph traversal is an implementation detail of `PetgraphStore`.

A new `cfdb-reachability` crate or a `cfdb-enrich` crate that exposes graph internals would violate the port boundary in the other direction: those crates would need to reach into `PetgraphStore`'s internal `KeyspaceState` and its `StableDiGraph`, coupling the adapter internals to a new outer crate. That is a dependency-rule violation.

**Verdict on reachability host:** implement `enrich_reachability` as a method on `PetgraphStore` inside `crates/cfdb-petgraph/src/` using internal petgraph traversal. The port (`EnrichBackend::enrich_reachability`) stays in `cfdb-core/src/enrich.rs`. The composition root (`compose.rs`) does not change.

---

## Q5 — Proposed slice decomposition

7 child issues. Issues 43-A through 43-E are the 5 passes. 43-F renames the port trait to align with the RFC vocabulary (mechanical prerequisite). 43-G adds the CLI `cfdb enrich` verb expansion.

### Issue 43-F — Rename port methods to RFC pass vocabulary (PREREQUISITE — must land first)

**Scope:** Mechanical rename of `EnrichBackend` methods (4 → 5, rename + add) in `cfdb-core/src/enrich.rs`. Update `EnrichVerb` enum in `cfdb-cli/src/enrich.rs`. Update `PetgraphStore`'s `impl EnrichBackend` in `cfdb-petgraph/src/lib.rs` (stub overrides, if any, are just rename). Update all callers.

**Depends on:** nothing (prerequisite for all other slices).

**Parallelizable with:** nothing (blocks 43-A through 43-E).

**Tests:**
- Unit: existing enrich.rs tests pass byte-identically after rename (mechanical invariant). Add one `not_implemented` round-trip test per new verb name.
- Self dogfood: `cfdb enrich-git-history --db .cfdb/db --keyspace cfdb` must return a well-formed JSON `EnrichReport` with `ran: false` (still stub), no panic.
- Cross dogfood: none — rationale: rename only, no behavior change, cross-fixture already at zero violations.
- Target dogfood: none — rationale: mechanical rename.

---

### Issue 43-A — Implement `enrich_deprecation` pass

**Scope:** Implement `PetgraphStore::enrich_deprecation` in `cfdb-petgraph`. Walk all `:Item` nodes; for each, set `item.props["is_deprecated"] = PropValue::Bool(...)` and `item.props["deprecation_since"] = PropValue::Str(...)` (or Null). Source: `#[deprecated]` attribute already extracted by cfdb-extractor at syn AST walk time. This pass reads existing node props (specifically a `deprecated_attr` intermediate prop if the extractor stores it) and materializes the two final attributes. No new I/O.

**Depends on:** 43-F (port rename).

**Parallelizable with:** 43-B, 43-C, 43-D after 43-F lands.

**Tests:**
- Unit: fixture graph with one `:Item` node carrying a `deprecated_attr` prop → after `enrich_deprecation`, verify `is_deprecated = true` and `deprecation_since` is populated. Fixture with no deprecated attr → `is_deprecated = false`, `deprecation_since = null`.
- Self dogfood: `cfdb enrich-deprecation --db .cfdb/db --keyspace cfdb` on cfdb's own tree. Assert `attrs_written > 0` and `ran = true`. Query for deprecated items: `MATCH (i:Item) WHERE i.is_deprecated = true RETURN i.qname` — verify known deprecated items appear (cfdb itself has at least one deprecated re-export from RFC-031 transition period, if any; otherwise assert `ran = true` is sufficient).
- Cross dogfood: run against graph-specs-rust at pinned SHA. Assert zero new ban rule violations (deprecation attributes are additive, no rule targets them yet).
- Target dogfood: report count of deprecated items in qbot-core — reviewer sanity-check only.

---

### Issue 43-B — Implement `enrich_bounded_context` pass

**Scope:** Implement `PetgraphStore::enrich_bounded_context`. For each `:Item` node, derive `bounded_context` from crate-prefix heuristic (strip `domain-` / `ports-` / `adapters-` / `inmemory-` / `postgres-` prefix — RFC §A3.2 addendum lines 288–292). Check `.cfdb/concepts/*.toml` overrides if present. Write `item.props["bounded_context"]` and add `(:Crate)-[:BELONGS_TO]->(:Context)` edges. Also requires creating `:Context {name, canonical_crate, owning_rfc}` nodes per RATIFIED.md B.1.3.

**Depends on:** 43-F (port rename). Schema bump (B.1.2 and B.1.3 from RATIFIED.md) must be reflected in `cfdb-core::NodeLabel` to accept `:Context` label — this may be a sub-task within this issue or a dependency on a separate schema issue.

**Parallelizable with:** 43-A, 43-C, 43-D after 43-F lands.

**Tests:**
- Unit: fixture with a `domain-trading` crate → after pass, `:Item` nodes carry `bounded_context = "trading"`. Fixture with a `.cfdb/concepts/messenger.toml` override → items in listed crates carry `bounded_context = "messenger"`.
- Self dogfood: run against cfdb's own tree. cfdb crates don't follow the qbot prefix convention, but cfdb-core, cfdb-petgraph, cfdb-query etc. should map to distinct contexts by a cfdb-specific `.cfdb/concepts/` toml. Assert all `:Item` nodes have a non-null `bounded_context` after the pass.
- Cross dogfood: run against graph-specs-rust at pinned SHA. Assert no new ban rule violations.
- Target dogfood: run against qbot-core. Report count of distinct bounded contexts discovered — reviewer sanity check that the heuristic matches the known qbot context map.

---

### Issue 43-C — Implement `enrich_rfc_docs` pass

**Scope:** Implement `PetgraphStore::enrich_rfc_docs`. Read `.concept-graph/` and `docs/rfc/*.md` files once at enrichment time. For each `:Item` node whose name appears as a keyword in an RFC doc, emit a `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` edge. Create `:RfcDoc {title, path}` nodes as needed. This pass performs filesystem I/O (read RFC markdown files); I/O is scoped to the workspace path passed to the enricher.

**Workspace path threading:** The `EnrichBackend` trait signature currently only takes `&Keyspace`. To locate RFC files, `enrich_rfc_docs` needs a workspace path. Options: (a) store the workspace path in `PetgraphStore` at construction time in `compose::load_store`; (b) extend `EnrichBackend::enrich_rfc_docs` to accept an additional `&Path` argument. Option (a) is cleaner from a port-purity standpoint (avoids making the path a concern of all callers); option (b) is more explicit. This is a design decision for the implementer; option (a) is preferred by this lens to keep the port signature minimal.

**Depends on:** 43-F (port rename).

**Parallelizable with:** 43-A, 43-B, 43-D after 43-F lands.

**Tests:**
- Unit: fixture store + fixture RFC markdown file containing a known item name → after pass, `MATCH (i:Item)-[:REFERENCED_BY]->(r:RfcDoc) RETURN i.qname, r.path` returns the expected row.
- Self dogfood: run against cfdb's own tree. Assert `edges_written > 0` (cfdb's own RFC docs reference at least `StoreBackend`, `EnrichBackend`, etc.). Query: `MATCH (i:Item)-[:REFERENCED_BY]->(r:RfcDoc) RETURN count(*)` — assert > 0.
- Cross dogfood: run against graph-specs-rust at pinned SHA. Assert zero new ban rule violations.
- Target dogfood: report how many qbot-core items are referenced by at least one RFC doc — reviewer sanity-check.

---

### Issue 43-D — Implement `enrich_git_history` pass

**Scope:** Implement `PetgraphStore::enrich_git_history`. For each `:Item` node, locate its defining file path (from `item.props["file"]` or equivalent). Walk git log for that file using `git2` crate (not subprocess) to derive `git_last_commit_unix_ts` (i64 epoch seconds — see determinism note in Q3), `git_last_author`, `git_commit_count`. Write these as item props. Items in virtual or generated files (no git history) receive null for these attrs.

**Determinism note (Q3):** store `git_last_commit_unix_ts` (epoch seconds), not `git_age_days`. The classifier Cypher (addendum line 216: `abs(a.git_age_days - b.git_age_days) AS age_delta`) computes age_delta at query time from the timestamp — the computation belongs in the Cypher, not baked into the graph as a calendar-relative integer.

**Depends on:** 43-F (port rename).

**Parallelizable with:** 43-A, 43-B, 43-C after 43-F lands.

**Tests:**
- Unit: fixture store with one `:Item` node pointing to a real file in a temp git repo with a known commit. After pass: `git_last_commit_unix_ts` matches the commit timestamp. `git_commit_count = 1`.
- Self dogfood: run against cfdb's own tree. Assert `attrs_written > 0`. Query: `MATCH (i:Item) WHERE i.git_last_commit_unix_ts IS NOT NULL RETURN count(*)` — assert ≥ 80% of items have history (cfdb has no untracked files in normal operation).
- Cross dogfood: run against graph-specs-rust at pinned SHA. Assert zero new ban rule violations.
- Target dogfood: report the 10 items in qbot-core with the highest `git_commit_count` — reviewer sanity-check for churn signal quality.

---

### Issue 43-E — Implement `enrich_reachability` pass

**Scope:** Implement `PetgraphStore::enrich_reachability`. Starting from all `:EntryPoint` nodes (must exist in graph — produced by cfdb-hir-extractor or the `cfdb extract --features hir` path per issue #86). BFS over `CALLS*` edges. For each visited `:Item` node: set `item.props["reachable_from_entry"] = PropValue::Bool(true)`. Items not visited by any BFS receive `reachable_from_entry = false`. Count distinct `:EntryPoint` origins reaching each item; write as `item.props["reachable_entry_count"] = PropValue::Int(n)`.

**Dependency on cfdb-hir-extractor:** `:EntryPoint` nodes are emitted by the HIR pipeline (`cfdb extract --features hir` / issue #86). Without them, this pass must degrade gracefully: if no `:EntryPoint` nodes exist in the keyspace, `enrich_reachability` returns an `EnrichReport` with `ran = false` and a warning: "no :EntryPoint nodes found — run `cfdb extract --features hir` first". It must not panic or set all items to `reachable_from_entry = false` silently, as that would be an incorrect fact.

**Depends on:** 43-F (port rename). Operationally depends on cfdb-hir-extractor (#86 already merged per git log).

**Parallelizable with:** none that depend on reachability output; parallelizable with 43-A through 43-D (they don't depend on each other).

**Tests:**
- Unit: fixture store with one `:EntryPoint` node `E`, two `:Item` nodes `A` and `B`, edge `(E)-[:CALLS]->(A)`. After pass: `A.reachable_from_entry = true`, `B.reachable_from_entry = false`, `A.reachable_entry_count = 1`. Test degraded path: keyspace with no `:EntryPoint` → `ran = false`, warning present.
- Self dogfood: run `cfdb extract --features hir --workspace . --db .cfdb/db --keyspace cfdb` then `cfdb enrich-reachability --db .cfdb/db --keyspace cfdb`. Assert `ran = true`. Query: `MATCH (i:Item) WHERE i.reachable_from_entry = false RETURN count(*)` — report count (expected to be non-zero for internal helpers, zero for all public-path items).
- Cross dogfood: run against graph-specs-rust at pinned SHA (with HIR extract). Assert zero new ban rule violations.
- Target dogfood: report percentage of qbot-core items with `reachable_from_entry = true` — reviewer sanity-check that the signal makes sense against a known architecture.

---

### Issue 43-G — Expand `cfdb enrich` CLI to 5 subcommands

**Scope:** Update `cfdb-cli/src/enrich.rs` — rename and add `EnrichVerb` variants. Add new CLI subcommands: `cfdb enrich-git-history`, `cfdb enrich-rfc-docs`, `cfdb enrich-deprecation`, `cfdb enrich-bounded-context`, `cfdb enrich-reachability`. Remove `cfdb enrich-docs`, `cfdb enrich-metrics`, `cfdb enrich-history`, `cfdb enrich-concepts` (or retain as deprecated aliases emitting a warning — implementer decision). Update `tests/wire_form_*.rs` to reflect the new verb count.

**Depends on:** 43-F (port rename). May be developed alongside 43-A through 43-E but must land after 43-F.

**Parallelizable with:** 43-A through 43-E (the CLI wiring is independent of the pass implementations).

**Tests:**
- Unit: `EnrichVerb` exhaustiveness — all 5 variants dispatch to the correct `EnrichBackend` method.
- Self dogfood: `cfdb enrich-deprecation --db .cfdb/db --keyspace cfdb` returns a valid JSON `EnrichReport`. Repeat for all 5 verbs.
- Cross dogfood: none — rationale: CLI verb shape change is not observed by ban rules.
- Target dogfood: none — rationale: CLI change only.

---

## Blockers to RATIFY

**B1 (BLOCKING — port trait must be renamed before any pass can ship).** The 4-method `EnrichBackend` in `cfdb-core/src/enrich.rs` does not match the 5 RFC passes from §A2.2. Shipping any concrete pass implementation against the current 4-method port creates a vocabulary split-brain between the trait and the RFC. Issue 43-F is a mandatory prerequisite.

**B2 (BLOCKING — determinism risk on `git_age_days`).** If `enrich_git_history` stores `git_age_days` as computed at enrichment time, two runs on different calendar days produce different canonical dumps, violating G1 (store.rs:53). The issue body must mandate storing `git_last_commit_unix_ts` (epoch) and computing age in Cypher at query time.

**B3 (ADVISORY — reachability degraded path must be specified).** If no `:EntryPoint` nodes exist (pre-HIR keyspace), `enrich_reachability` must return a non-fatal `EnrichReport` with `ran = false` and a clear warning, not silently mark all items unreachable. The issue body for 43-E must specify this degraded behavior explicitly.

**B4 (ADVISORY — workspace path threading for `enrich_rfc_docs`).** The `EnrichBackend` trait takes only `&Keyspace`. Reading RFC markdown files requires a workspace path. The design decision (store path in `PetgraphStore` at load time vs. extend trait signature) must be made before 43-C starts; both options preserve port purity but have different ergonomic tradeoffs. This council recommends storing the path in `PetgraphStore` (no trait signature change) so the port remains minimal.

---

## References (file:line for every factual claim)

- `EnrichBackend` 4 methods: `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/enrich.rs:76–108`
- `EnrichReport::not_implemented` stub: `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/enrich.rs:48–59`
- `impl EnrichBackend for PetgraphStore {}` (empty, stubs): `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-petgraph/src/lib.rs:139–143`
- `StoreBackend` G1/G2 determinism invariants: `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/store.rs:53–62`
- Composition root declaration: `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-cli/src/compose.rs:1–7`
- `compose::load_store` factory: `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-cli/src/compose.rs:58–67`
- CLI `EnrichVerb` enum and dispatch: `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-cli/src/enrich.rs:14–33`
- RFC §A2.2 BLOCK-1 two-stage pipeline + 5-pass table: `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/docs/RFC-cfdb-v0.2-addendum-draft.md:192–204`
- RFC §A2.2 classifier Cypher (reads `git_age_days`): `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/docs/RFC-cfdb-v0.2-addendum-draft.md:216`
- RFC §A3.2 bounded context prefix convention: `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/docs/RFC-cfdb-v0.2-addendum-draft.md:288–292`
- RATIFIED.md B.1 schema bumps (bounded_context attr, Context node, BELONGS_TO edge): `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/council/RATIFIED.md:284–292`
- RATIFIED.md A.14 `list_items_matching` 16th verb (informational, not blocking #43): `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/council/RATIFIED.md:164–188`
- `cfdb-petgraph/Cargo.toml` — no git2 dependency yet (confirming 43-D must add it): `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-petgraph/Cargo.toml`
- `cfdb-cli/Cargo.toml` — HIR feature gate (confirming `--features hir` is opt-in): `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-cli/Cargo.toml`
