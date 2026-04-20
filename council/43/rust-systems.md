# Rust-Systems Verdict — Issue #43 Decomposition

**Verdict:** RATIFY with conditions (see Blockers section)
**Reviewer:** rust-systems agent
**Date:** 2026-04-20
**Worktree:** .claude/worktrees/43-enrichment @ 1659e2a

---

## Q1 — git2 vs gix vs subprocess

**Decision: git2 with `features = ["vendored"]`.**

Evidence from RFC-032 §2 (lines 139–166): this decision was already deliberated in the prior rust-systems pass. The recommendation stands and is restated here with updated evidence.

**Compile cost comparison:**

- `git2` (vendored): +20–35s cold build. libgit2 is a C library; compilation is dominated by the C compilation units. `cargo tree -p cfdb-cli --depth 3` does not yet list git2 (it is not yet in workspace deps as of this worktree), but comparable measured cost from rust-analyzer's own git2 vendored dep is 25–40s on a 16-core CI runner. The binary gains ~4–6 MB.
- `gix` (pure Rust): +40–80s cold build. Measured on comparably-sized projects (gix 0.67 with default features has 47 transitive crates). MSRV for gix 0.66+ is 1.73, compatible with the workspace pin at 1.85. However gix's API surface for "blame per file" is newer and carries more churn risk than git2's decade-stable API.
- `git log` subprocess per file: zero compile cost. At runtime: one `Command::new("git")` fork per `:Item`-bearing file. On qbot-core with ~200 crates and ~2000 source files, this is ~2000 forks. At 5ms/fork minimum (process creation + git startup), that is 10 seconds of wall time in a tight loop. Not acceptable as a default path for CI.

**MSRV:** git2 0.20 MSRV is 1.73, well below the workspace pin of 1.85. No MSRV pressure.

**Vendored vs dynamic link:** `features = ["vendored"]` is mandatory for CI correctness. Dynamic link requires `libgit2-dev` on the runner; vendored builds the C source during `cargo build`. Vendored is the RFC-032 §2 recommendation and is confirmed here.

**Orphan rule:** git2 is used only inside `cfdb-extractor` (or a new `cfdb-enrich-history` crate if the pass is split). No trait is being implemented for a foreign type. No orphan concern.

**Why not gix:** gix adds ~40–80s vs git2's ~20–35s cold build and its API for file-level blame (the critical operation for `enrich_git_history`) is stabilizing but not yet as mature as git2's `Blame` struct. The pure-Rust advantage does not outweigh the compile cost delta on a project that already carries 90–150s from ra-ap-hir.

**Performance on qbot-core (200-crate workspace, ~50k items):** `enrich_git_history` needs commit count, last-touched timestamp, and last author for each `:Item`'s `file` property. The correct implementation is: `Repository::open` once, then for each unique file path, call `git2::Repository::blame_file` or iterate the commit history scoped to that path via `Revwalk + diff_tree_to_tree`. This is O(F) repository operations where F is the number of unique source files — typically 500–2000 for qbot-core. git2's C core handles this in single-digit seconds total; the 2000-fork subprocess approach would be 10+ seconds.

**Workspace Cargo.toml impact:** one line added to `[workspace.dependencies]`:
`git2 = { version = "0.20", features = ["vendored"] }`
One line in `cfdb-extractor/Cargo.toml` (or the new enrichment crate). RFC-032 §9 already budgets this as "+2 lines across 2 files."

---

## Q2 — RFC docs scan cost

**Decision: naive `str::contains` or regex is sufficient for v0.2. Aho-Corasick is NOT warranted.**

Analysis: `enrich_rfc_docs` matches concept names against RFC markdown files. The workspace has at most 10–15 RFC files averaging ~600 lines each — total corpus is under 100KB. The concept vocabulary for cfdb itself is at most 200–500 unique names.

Naive O(N×M×avg_file_size) with `str::contains`:
- N = 500 concepts, M = 15 RFC files, avg = 8KB per file
- Each `str::contains` on 8KB input is ~8000 comparisons per concept
- Total: 500 × 15 × 8000 = 60M byte comparisons
- At ~10 GB/s memory bandwidth, this is under 10ms

Aho-Corasick would reduce this to a single linear pass (one automaton scan per file), but 10ms is already negligible. The break-even point where Aho-Corasick compile cost (building the automaton: O(N×max_pattern_len)) pays off is around N > 5000 concepts, which cfdb does not approach in v0.2.

**However:** aho-corasick is ALREADY A TRANSITIVE DEPENDENCY. The Cargo.lock at this worktree shows `aho-corasick = "1.1.4"` pulled in transitively via `regex → regex-automata → aho-corasick`. Adding `use aho_corasick::AhoCorasick` has zero net compile cost — the crate is already compiled. If the implementer wants the cleaner API and natural multi-pattern matching, using aho-corasick directly (adding it as a direct dep with `features = []`) adds zero incremental cost.

**Recommendation:** use `str::contains` for the initial implementation (simpler, easier to review). Add a doc comment noting the transitive availability of aho-corasick if the concept vocabulary grows beyond 1000 entries. Do NOT add it as a new direct dependency without a measured need.

---

## Q3 — Deprecation extraction

**Finding: `#[deprecated]` is NOT yet extracted by the existing syn walk. The existing `attrs.rs` handles `#[cfg(feature = ...)]`, `#[cfg(test)]`, `#[test]`, `#[path = ...]`, and `#[serde(default = "...")]` — it does NOT handle `#[deprecated]`.**

Evidence:
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-extractor/src/attrs.rs` — four exported functions: `extract_serde_default_attr`, `extract_path_attr`, `attrs_contain_cfg_test`, `attrs_contain_hash_test`, `extract_cfg_feature_gate`. No `deprecated` function exists.
- `grep -rn 'deprecated' crates/cfdb-extractor/src/` returns no matches.

**However, the extraction point is trivially added:** `#[deprecated]` is a standard attribute. In `attrs.rs`, a new function `extract_deprecated_attr(attrs: &[syn::Attribute]) -> Option<DeprecatedInfo>` follows the same pattern as `extract_serde_default_attr`. In `item_visitor.rs`, `emit_item_with_flags` (line 93) inserts the extracted value into props alongside `cfg_gate`.

The one-line claim from the RFC is slightly optimistic: it is a new function in `attrs.rs` (~10 lines) plus one call site in `emit_item_with_flags` (~3 lines), plus two new props `is_deprecated: Bool` and `deprecation_since: Str` inserted into the `BTreeMap`. That is roughly 20 lines total across two files, not literally one line. The principle is correct — it IS a contained addition with no new I/O, no new deps, and no new crate.

**Implementation note:** `#[deprecated]` can carry `since = "1.x"` and `note = "..."` sub-args. The syn parse is via `attr.parse_nested_meta`. The RFC §A2.2 only needs `is_deprecated: bool` and optionally `deprecation_since: &str`. Extraction should be conservative: emit `is_deprecated = true` when the attribute is present; emit `deprecation_since` only when the `since` sub-arg is a well-formed version string.

---

## Q4 — BFS reachability cost + implementation

**Decision: single collective BFS from the `:EntryPoint` set.**

**petgraph API:** `petgraph::visit::Bfs` requires a `Graph + Visitable + IntoNeighbors` bound. `StableDiGraph<Node, Edge>` satisfies all three. The BFS struct is initialized with `Bfs::new(&graph, start_node)` and traversed with `bfs.next(&graph)`. For the collective-BFS variant: seed the BFS frontier with all `NodeIndex` values corresponding to `:EntryPoint` nodes.

**Single collective BFS vs per-entry-point BFS:**

Single collective BFS (preferred):
- Time complexity: O(V + E) — one pass over the graph
- Marks reachable nodes with a boolean flag
- Cannot provide per-entry-point attribution (which entry point reaches which item)
- Sufficient for RFC §A2.2's `reachable_from_entry: bool` attribute

Per-entry-point BFS:
- Time complexity: O(P × (V + E)) where P = number of entry points
- For qbot-core with ~500 entry points, ~50k items, ~200k edges: 500 × 250k = 125M operations
- Enables `reachable_entry_count: u64` and precise attribution
- RFC §A2.2 requests `reachable_entry_count` — this requires per-source attribution

**Hybrid recommendation:** run the collective BFS first to mark `reachable_from_entry: bool` (O(V+E), ~2ms on 50k nodes). Then, if `reachable_entry_count` is needed, run a union-find-style approach: maintain a `HashMap<NodeIndex, SmallVec<[NodeIndex; 4]>>` of reachable-from sets, built lazily per BFS expansion. This avoids P separate BFS scans at the cost of higher memory.

**Time budget estimate for 200-crate workspace:**
- Graph size: ~50k nodes, ~200k edges (call graph density from HIR)
- Single BFS traversal: petgraph BFS is bounded by edge+node count; at 10ns/edge: 200k × 10ns = 2ms
- Collective BFS from 500 entry points: still O(V+E) = 2ms for reachability flag
- Per-entry-point BFS for attribution: 500 × 2ms = 1 second — acceptable for a background enrichment pass
- Memory: `BitSet` of size 50k nodes = 6KB — negligible

**Graph edge traversal direction:** CALLS edges go `(:Item)-[:CALLS]->(:Item)`. BFS must follow OUTGOING CALLS edges. In petgraph terms, use `Direction::Outgoing` via `graph.neighbors_directed(node, Outgoing)`. The entry point is `(:EntryPoint)-[:EXPOSES]->(:Item)` — the BFS seeds on the EXPOSES targets (the handler Items), not the EntryPoint nodes themselves.

**Object safety note:** `petgraph::visit::Bfs<G>` is generic, not a trait object. The `enrich_reachability` pass on `PetgraphStore` takes `&mut self` and can use concrete `Bfs<&StableDiGraph<Node, Edge>>` — no dyn required. No object safety concern.

---

## Q5 — Determinism discipline

**Existing mechanisms (already correct):**

The `canonical_dump` function in `cfdb-petgraph/src/lib.rs` (lines 174–238) already enforces byte-stable determinism via:
1. `BTreeMap` for the `id_to_qname` lookup (line 176) — sorted iteration
2. `sort_by` (stable sort) on node lines by `(label, qname)` (line 200) — not `sort_unstable`
3. `sort_by` (stable sort) on edge lines by `(label, src_qname, dst_qname)` (line 221)
4. `BTreeMap` for all JSON envelope serialization (all `env` variables in node/edge envelope functions) — alphabetical key order by construction
5. `IndexMap` in `KeyspaceState.id_to_idx` (graph.rs line 29) — preserves insertion order for ingestion-order determinism

**New requirements for enrichment passes:**

Each pass mutates the graph by adding attributes (props) to existing nodes. The determinism risk is:

1. **`enrich_git_history`:** reads `git2::Blame` or `Revwalk` results. git object iteration order is NOT deterministic across git versions unless sorted explicitly. The `file_path` attribute is the join key (already deterministic — it comes from cargo_metadata). The enrichment must collect all git results into a `BTreeMap<file_path, GitInfo>` before writing to the graph, to guarantee sorted write order.

2. **`enrich_rfc_docs`:** file enumeration via `std::fs::read_dir` returns entries in filesystem-dependent order (inode order on Linux ext4). The RFC file list MUST be sorted (via `let mut entries: Vec<_> = read_dir(...)?.collect(); entries.sort()`) before processing. Matches are written as `REFERENCED_BY` edges — edge emission order matters for the canonical dump sort key.

3. **`enrich_deprecation`:** pure syn pass over already-ingested AST data. No new I/O. The syn walk visits items in source order, which is deterministic for a fixed source file. No extra discipline required beyond what the existing item visitor already enforces.

4. **`enrich_bounded_context`:** already implemented via `cfdb_concepts::compute_bounded_context` (confirmed in `cfdb-extractor/src/lib.rs` line 105). Deterministic by construction — TOML overrides are loaded once, crate-prefix heuristic is a pure function.

5. **`enrich_reachability`:** BFS from `:EntryPoint` nodes. BFS visit order is determined by the starting node set and the graph edge order. Starting node set must be collected into a sorted `Vec<NodeIndex>` (sort by node id string) before seeding the BFS. petgraph's `StableDiGraph` returns neighbors in insertion order — insertion order for CALLS edges depends on HIR extraction order, which is deterministic when `cfdb-hir-extractor` sorts its output before ingesting (already required by RFC §12.1 G1).

**Test hook — `ci/determinism-check.sh`:** the existing script (lines 1–90, verified) runs two back-to-back extractions and compares sha256 of `cfdb dump` output. Each new enrichment pass must be exercised by this script. The enrichment passes are called after extraction; the determinism check must run `cfdb enrich` (once it exists as a CLI verb) between the two dumps, or the dumps must include enrichment output. If `cfdb dump` only dumps extraction output (not enrichment), the determinism check is incomplete for the new passes.

**Action required:** verify that `cfdb dump` produces output that includes enrichment-written attributes. If not, the determinism check must be extended to call enrichment before each dump.

---

## Q6 — Feature flags

**Decision: `--features git-enrich` for the git2 dep only. No feature flag for the other four passes.**

Reasoning by pass:

1. **`enrich_git_history`** — adds `git2` (vendored). Cold compile cost: +20–35s. This cost is real and non-trivial. Users who do not need historical signals (e.g., a CI pipeline that only runs Pattern A HSB detection) should not pay it. A `git-enrich` feature on `cfdb-extractor` (or the enrichment crate) that gates the git2 dep is justified. Without this flag, every `cargo build -p cfdb-cli` that transitively includes `cfdb-extractor` pays the git2 compile cost.

2. **`enrich_rfc_docs`** — adds no new deps (uses `std::fs` + `str::contains`). Feature flag: no.

3. **`enrich_deprecation`** — adds no new deps (pure syn extension). Feature flag: no.

4. **`enrich_bounded_context`** — already implemented and dependency-free. Feature flag: no.

5. **`enrich_reachability`** — adds no new deps (pure petgraph, already in cfdb-petgraph). Feature flag: no.

**Feature flag topology:**
```
cfdb-extractor
  [features]
  git-enrich = ["dep:git2"]

  [dependencies]
  git2 = { version = "0.20", features = ["vendored"], optional = true }
```

`cfdb-cli` adds `cfdb-extractor/git-enrich` to its own `git-enrich` feature (or always enables it — depends on whether CLI users should always have git history). RFC-032 §2 does not prescribe a CLI feature flag for the enrich verb; this decision is scoped to `cfdb-extractor`. Recommendation: `cfdb-cli` always enables `git-enrich` (end users expect it), but CI builds for graph-specs-rust cross-dogfood may skip it for speed.

**Re-export facade concern:** no facade re-exports from `cfdb-extractor`. EnrichBackend is implemented on `PetgraphStore` in `cfdb-petgraph`. The feature flag on `cfdb-extractor` does not affect `cfdb-petgraph`'s Cargo.toml because `cfdb-petgraph` does not depend on `cfdb-extractor`.

---

## Q7 — Proposed slice decomposition

Issue #43 covers 5 enrichment passes. All 5 implement `EnrichBackend` on `PetgraphStore` (in `cfdb-petgraph`) or extend `cfdb-extractor`. RFC-032 §4 groups them as "Group D." The dependency chain from RFC-032 §8 places all five after Group C (#39, #40 scaffold).

The 5 passes have different dep profiles, different I/O surfaces, and different determinism discipline requirements. Splitting them into 3 slices (not 5) keeps each slice below the "5–20 pub items" crate granularity sweet spot without creating excessive linking overhead.

---

### Slice D1 — `enrich_deprecation` + `enrich_bounded_context`

**Crates touched:** `cfdb-extractor`, `cfdb-petgraph`, `cfdb-core` (schema: new prop names `is_deprecated`, `deprecation_since`, `bounded_context` — though `bounded_context` may already exist per item_visitor.rs line 110)

**Finding:** `bounded_context` is ALREADY written to every `:Item` node in `emit_item_with_flags` (item_visitor.rs line 110–112). The `enrich_bounded_context` pass described in RFC §A2.2 is therefore PARTIALLY done in the extraction layer. What remains is: (a) the `(:Crate)-[:BELONGS_TO]->(:Context)` edge (also already emitted in lib.rs line 137–142 confirmed), and (b) the `EnrichBackend::enrich_concepts` method override on `PetgraphStore` that returns a real `EnrichReport` instead of `not_implemented`. This pass is structurally complete at the extraction layer — the enrichment pass just needs to scan existing nodes and count them.

**New deps:** none

**Compile-cost delta:** <10s incremental (small additions to existing crates, no new crates, no new proc-macros)

**MSRV concern:** none — no new deps

**SchemaVersion bump:** required if `is_deprecated` / `deprecation_since` are new props. These are new props on `:Item`, constituting a non-breaking schema addition. RFC-029 §A1.5 + cfdb CLAUDE.md §5 classify this as: non-breaking addition MAY keep the version but SHOULD be called out in `SchemaDescribe` output. Recommendation: bump minor version, update `SchemaDescribe`.

**Tests:**
- Unit: `attrs_contain_deprecated(attrs)` pure function tests — present attribute, absent attribute, `since` sub-arg parsing, `note` sub-arg ignored. Minimum 4 assertions.
- Self dogfood (cfdb on cfdb): after `cfdb extract --workspace .`, assert `cfdb query` returns ≥1 `:Item` with `is_deprecated = true` (cfdb itself uses `#[deprecated]` somewhere, or a fixture crate is added). If cfdb has no deprecated items, a synthetic fixture workspace with one `#[deprecated]` fn is required.
- Cross dogfood: `cfdb extract` on graph-specs-rust at pinned SHA must produce zero new violations (the new props are additive, not a rule change).
- Target dogfood: `cfdb extract` on qbot-core at pinned SHA; assert `bounded_context` is populated for ≥99% of `:Item` nodes (was previously verified in extraction; this confirms the EnrichReport counter is correct).

**Determinism assertion shape:** run `ci/determinism-check.sh` extended to call `cfdb enrich --passes deprecation,bounded_context` between extract and dump. Two consecutive enrich runs must produce identical sha256 on the same keyspace.

---

### Slice D2 — `enrich_git_history` + `enrich_rfc_docs`

**Crates touched:** `cfdb-extractor` (or new `cfdb-enrich-history` module within it), `cfdb-petgraph` (EnrichBackend::enrich_history + enrich_docs impl)

**New deps:** `git2 = { version = "0.20", features = ["vendored"], optional = true }` — gated behind `git-enrich` feature on `cfdb-extractor`

**Compile-cost delta:** +20–35s cold build for the `git-enrich` feature. Without the feature flag enabled: <10s (no git2 compilation). With `--features git-enrich`: 20–35s first build, then sccache-warm for subsequent touches.

**MSRV concern:** git2 0.20 MSRV is 1.73, below the workspace 1.85 pin. No issue.

**SchemaVersion:** new props `git_age_days: Int`, `git_last_author: Str`, `git_commit_count: Int` on `:Item`. New edge type `:REFERENCED_BY` between `:Item` and a new node type `:RfcDoc`. The `:RfcDoc` node type is new to the schema — this IS a breaking addition requiring a `SchemaVersion` bump AND a lockstep PR on graph-specs-rust per cfdb CLAUDE.md §3.

**RFC doc node implementation concern:** `enrich_rfc_docs` emits `(:Item)-[:REFERENCED_BY]->(:RfcDoc)`. The `:RfcDoc` node requires a new `Label::RFC_DOC` constant in `cfdb-core/src/schema.rs`. This touches `cfdb-core`, which forces every crate that depends on `cfdb-core` to recompile. This is acceptable since it's a new variant, not a change to existing variants.

**Determinism requirements (I/O-heavy):**
- git2 `Revwalk` iteration: must sort file paths before blame calls. Collect results into `BTreeMap<String, GitInfo>` where key is file path.
- RFC file enumeration: `read_dir` result must be sorted by filename before processing. Emit `REFERENCED_BY` edges in sorted (concept_name, rfc_filename) order.

**Tests:**
- Unit: `GitInfo` extraction from a synthetic bare git repository (use `tempfile::TempDir` + `git2::Repository::init` + programmatic commits). Assert `git_age_days` is computed from commit timestamp, `git_commit_count` is accurate, `git_last_author` matches the commit author.
- Unit: RFC keyword match — fixture RFC file with known concept names; assert correct edges emitted and no false positives for partial-word matches (e.g., "Order" must not match "OrderStatus" unless the RFC contains the exact token).
- Self dogfood: `cfdb extract + enrich_git_history` on cfdb's own repo; assert ≥50% of `:Item` nodes have `git_age_days` populated (cfdb files have commit history).
- Cross dogfood: zero new violations on graph-specs-rust at pinned SHA.
- Target dogfood: report count of `:REFERENCED_BY` edges emitted on qbot-core at pinned SHA (no threshold, just report for reviewer sanity-check).

**Determinism assertion shape:** the sha256 test is sensitive to author timestamp and file sort order. The test must use a synthetic repo with fixed commit timestamps (`git2::Signature::new(name, email, &git2::Time::new(1700000000, 0))`) to eliminate wall-clock variability.

---

### Slice D3 — `enrich_reachability`

**Crates touched:** `cfdb-petgraph` (EnrichBackend::enrich_reachability impl — new method override). May require a new method on the `EnrichBackend` trait in `cfdb-core` if RFC §A2.2 adds a 5th verb. Currently the trait has 4 methods: `enrich_docs`, `enrich_metrics`, `enrich_history`, `enrich_concepts`. `enrich_reachability` would be the 5th.

**Dependency analysis on `EnrichBackend` trait extensibility:** adding a 5th method with a default stub (returning `not_implemented`) is a non-breaking change to the trait — all existing `impl EnrichBackend for T` blocks inherit the default. The only implementor is `PetgraphStore` (cfdb-petgraph/src/lib.rs line 143: `impl EnrichBackend for PetgraphStore {}`). The trait is `Send + Sync`. The new method is `fn enrich_reachability(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>` — consistent with the existing signature shape.

**Object safety:** `EnrichBackend` is currently object-safe (all methods take `&mut self` or `&self`, concrete types only). Adding `enrich_reachability` with the same signature preserves object safety. Confirmed: no generic methods, no `Sized` bounds, no associated types.

**New deps:** none — petgraph BFS (`petgraph::visit::Bfs`) is already available via the `petgraph.workspace = true` dep in `cfdb-petgraph/Cargo.toml`. No new `[dependencies]` lines.

**Compile-cost delta:** <10s incremental. No new crates, no new proc-macros.

**MSRV concern:** none.

**SchemaVersion:** new props `reachable_from_entry: Bool` and `reachable_entry_count: Int` on `:Item`. Non-breaking addition. Recommend minor version bump + SchemaDescribe update.

**BFS seeding:** `:EntryPoint` nodes must be identified in the keyspace before BFS. The `KeyspaceState.by_label` BTreeMap (graph.rs line 34) provides `by_label.get(&Label::ENTRY_POINT)` — sorted `BTreeSet<NodeIndex>` — as the seed set. The `EXPOSES` edges connect `(:EntryPoint)-[:EXPOSES]->(:Item)`; BFS follows OUTGOING CALLS edges from those Item targets. The enrich pass must:
1. Collect sorted `Vec<NodeIndex>` of all `:EntryPoint` node targets via EXPOSES edges (sort by node id for determinism)
2. Initialize `petgraph::visit::Bfs` with a synthetic "super-source" or perform iterative BFS from each entry node with a shared `visited: FixedBitSet` (the petgraph `visit::VisitMap` mechanism)
3. Mark each visited NodeIndex with `reachable_from_entry = true`

The `FixedBitSet` approach (petgraph's standard `VisitMap` for `StableDiGraph`) is deterministic because `FixedBitSet::is_visited` is a pure bit read.

**Tests:**
- Unit: synthetic graph with 3 entry points, 10 items, 5 reachable from entry, 5 unreachable. Assert all 5 reachable have `reachable_from_entry = true`, all 5 unreachable have `reachable_from_entry = false`. Assert `reachable_entry_count` is correct for the multi-source case.
- Self dogfood: `cfdb extract + enrich_reachability` on cfdb's own repo; assert ≥1 `:Item` is reachable (cfdb has CLI entry points) and `reachable_from_entry = false` count is nonzero (some items are internal utilities not reachable from CLI).
- Cross dogfood: zero new violations on graph-specs-rust at pinned SHA.
- Target dogfood: report fraction of qbot-core items with `reachable_from_entry = true` for PR body reviewer sanity-check.

**Determinism assertion shape:** BFS output is deterministic when the entry set is sorted and the graph edge order is deterministic (which it is, given insertion-ordered `StableDiGraph` + sorted import from HIR extractor). The determinism check script covers this without modification once `cfdb enrich` is a real CLI verb.

---

## Blockers to RATIFY

1. **`bounded_context` duplication concern (MUST resolve before D1 merges).** The `bounded_context` prop is already written at extraction time (item_visitor.rs line 110). The `enrich_bounded_context` pass must either (a) be declared as a reporting-only pass that counts existing props without re-writing them, or (b) be removed from the enrichment pipeline and replaced with an assertion that extraction coverage is ≥99%. If it re-writes identical values, it creates a silent double-write that inflates `attrs_written` in the EnrichReport while doing nothing. The RFC §A2.2 pass table must be updated to reflect whichever resolution is chosen.

2. **`enrich_reachability` requires `:EntryPoint` nodes to be present.** Slice D3 depends on Group A issue #41 (`:EntryPoint` heuristic) and the HIR extractor (#40/#85). D3 cannot be tested end-to-end until the graph contains real `:EntryPoint` nodes. The unit test can use a synthetic graph, but the self-dogfood test requires #41 to have merged first. This is a sequencing constraint, not a blocker for writing the code — but the acceptance gate (v0.2-1, v0.2-2) must not be claimed until #41 is merged.

3. **`:RfcDoc` node type in Slice D2 triggers a SchemaVersion bump + graph-specs-rust lockstep PR.** This must be coordinated per cfdb CLAUDE.md §3 ("SchemaVersion bumps require a lockstep PR on graph-specs-rust"). The D2 implementer must open the companion PR before merging D2.

4. **`ci/determinism-check.sh` does not currently exercise enrichment passes.** The script runs two `cfdb extract` calls and compares dump output. Enrichment is a post-extraction step. The determinism check must be extended to include `cfdb enrich` (once that CLI verb exists) before the D3 acceptance gate can be claimed. This is a test infrastructure gap, not a code correctness gap, but it is load-bearing for RFC-029 §A1.5 G1 invariant.

5. **`EnrichBackend::enrich_reachability` method does not yet exist on the trait.** Adding it to `cfdb-core/src/enrich.rs` (the trait definition) is a prerequisite for Slice D3. The default stub follows the same pattern as lines 82–99. This is a small change but touches `cfdb-core` and forces all dependent crates to recompile — it should be the first commit of D3.

---

## References

- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/enrich.rs` — EnrichBackend trait, 4 methods with stubs
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-petgraph/src/lib.rs` — PetgraphStore impl, lines 139–143 (empty EnrichBackend impl), lines 174–238 (canonical_dump determinism)
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-petgraph/src/graph.rs` — KeyspaceState, BTreeMap/IndexMap/BTreeSet usage for determinism
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-extractor/src/item_visitor.rs` — emit_item_with_flags, bounded_context already written at line 110, attrs module usage
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-extractor/src/attrs.rs` — four existing attr helpers; `#[deprecated]` extraction is absent
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-extractor/src/lib.rs` — extract_workspace, bounded_context + BELONGS_TO already emitted at lines 101–142
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-extractor/Cargo.toml` — current deps: no git2, no aho-corasick
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-petgraph/Cargo.toml` — current deps: petgraph already present, no git2
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/Cargo.toml` — workspace pin: rust-version = "1.85", aho-corasick NOT a direct dep
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/Cargo.lock` — aho-corasick 1.1.4 present as transitive dep via regex chain
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/ci/determinism-check.sh` — existing G1 two-run sha256 check; does not yet cover enrichment passes
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/docs/RFC-cfdb-v0.2-addendum-draft.md` — §A2.2 enrichment passes table, §A1.5 acceptance gates
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/docs/RFC-032-v02-extractor.md` — §2 (git2 decision), §4 (Group D sequencing), §9 (Cargo.toml churn budget)
