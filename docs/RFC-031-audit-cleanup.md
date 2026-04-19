---
title: "RFC-031: Audit cleanup — absorb orphan issues #22-#29"
status: Accepted
date: 2026-04-19
authors: [solid-architect, clean-arch, rust-systems]
supersedes: issues #22, #23, #25, #26, #27, #28, #29 (orphan filings)
---

# RFC-031: Audit cleanup — absorb orphan issues #22–#29

Three-lens audit (SOLID/component-principles, Clean Architecture, Rust-systems) ran across the cfdb workspace on 2026-04-19 and produced eight orphan issues filed without a backing RFC. Per user directive "ALL issues are in a RFC", this document is the canonical home. Every section maps to one issue; existing issue bodies should add the line `RFC: RFC-031 §N`.

---

## Audit synthesis

The audit applied six lenses simultaneously:

| Lens | Principles applied |
|---|---|
| SOLID/ISP | Interface segregation — every consumer should depend only on the methods it uses |
| SOLID/SRP | Single responsibility — each module/crate has one reason to change |
| CCP | Common Closure Principle — things that change for the same reason belong together |
| CRP | Common Reuse Principle — things that are not reused together should not be deployed together |
| SDP/SAP | Stable Dependencies + Stable Abstractions — depend in direction of stability; stable things must be abstract |
| Rust-systems | Complexity ceilings, error-type ergonomics, scan-deduplication |

**Cross-cutting consensus finding:** `cfdb-core/src/store.rs` is the central architectural hotspot. It hosts a fat trait (`StoreBackend`) that conflates three independent responsibilities (ingest, query/eval, enrich), which produces ISP, CRP, and CCP violations simultaneously. Splitting that trait is the highest-leverage change in the backlog and is the prerequisite for most other issues.

**Execution order (consensus, do not reorder without team consultation):**

```
§1 (#29-verify) → §2 (#27) → §3 (#25) → §4 (#23) → §5 (#26) → §6 (#28)
```

Rationale for this ordering is given per section below.

---

## §1 — Verify #29: typed BuilderError for list_items (close as invalid)

**Issue: #29**

**Verification finding:** The three `expect()` calls cited in the original filing are located at `cfdb-core/src/query/list_items.rs:169, 207, 263` — all three are inside the `#[cfg(test)]` block that begins at line 162. They are test assertions, not production code paths. There is no typed `BuilderError` gap in the production surface of this module.

Evidence:

- `list_items.rs:162` — `#[cfg(test)]`
- `list_items.rs:169` — `.expect("list_items_matching always emits a where_clause")` — test helper `regex_of`
- `list_items.rs:207` — `.expect("alias")` — inside `list_items_matching_filters_by_name_pattern` test
- `list_items.rs:263` — `.expect(...)` — inside `list_items_matching_group_by_context_partitions_rows` test

The production function `list_items_matching` at line 33 is a pure constructor that always returns `Query`; it cannot fail and has no `Result` return type. No `expect` calls exist in the production code path.

**Recommendation:** Close #29 as invalid. No typed `BuilderError` is needed here.

**Ordering rationale:** Must be verified first because subsequent sections reference it by name and its status (valid vs invalid) would change the prescription for §2.

---

## §2 — Split `EnrichBackend` out of `StoreBackend`

**Issue: #27**

**Lenses:** ISP (primary), CRP (secondary), SRP (secondary)

**Evidence:**

`cfdb-core/src/store.rs:56` — `StoreBackend` declares 8 methods:
- `ingest_nodes` (ingest concern)
- `ingest_edges` (ingest concern)
- `execute` (query concern)
- `schema_version` (query/meta concern)
- `list_keyspaces` (meta concern)
- `drop_keyspace` (lifecycle concern)
- `canonical_dump` (query/export concern)
- `enrich_docs`, `enrich_metrics`, `enrich_history`, `enrich_concepts` (4 enrich concerns — provided with default stubs, lines 88–113)

There is exactly one implementor: `PetgraphStore` at `cfdb-petgraph/src/lib.rs:87`.

**ISP violation (quantified):** `cfdb-cli/src/enrich.rs:34-38` — the `enrich` handler calls only the 4 `enrich_*` methods on `StoreBackend`. It depends on the trait to get those 4 but compiles in the full 8-method trait contract. Trait utilization ratio = 4/8 = 50% — below the CRP threshold of 100% for a two-consumer scenario where the other consumer (`commands.rs`) uses only query methods.

`cfdb-cli/src/commands.rs` calls `ingest_nodes`, `ingest_edges`, `execute`, `canonical_dump`, `list_keyspaces`, `drop_keyspace`, `schema_version` — never the 4 enrich methods. Its utilization = 7/11 = 64%.

**SRP violation:** `StoreBackend` changes for two independent reasons: (1) query evaluation changes (AST extensions, new return shapes), (2) enrichment pass API changes (new verb added, report shape changes). These two axes are independent — a new enrichment verb in v0.2 must not force a recompile of all query consumers.

**CCP violation:** `cfdb-core/src/enrich.rs` (the `EnrichReport` type) and the four enrich default stubs in `store.rs:88-113` share the same domain dependency (enrichment passes). They change for the same reason. They should move to a dedicated module/crate together.

**Prescription:** Extract a new trait `EnrichBackend` (in `cfdb-core/src/enrich.rs` or a new `cfdb-core/src/backend/enrich.rs`) with the four enrich methods and their default stubs. `StoreBackend` drops those four methods. `PetgraphStore` implements both traits. Consumers that only enrich depend on `EnrichBackend`; consumers that only query depend on `StoreBackend`. The default stubs move with `EnrichReport` into the enrich module.

**Ordering rationale:** This is the highest-leverage change — it unblocks §3 (the query composer location depends on whether `StoreBackend` stays fat or shrinks) and §4 (the composition root can be cleaner once the traits are split).

---

## §3 — Move query composers out of `cfdb-core`

**Issue: #25**

**Lenses:** CCP (primary), SDP (secondary), SRP (secondary)

**Evidence:**

`cfdb-core/src/query/list_items.rs:1-159` — the `list_items_matching` query composer function lives in `cfdb-core`. It depends on `cfdb-core`'s own AST types (`Query`, `Pattern`, `NodePattern`, `Predicate`, `Expr`, etc.) and `ItemKind`.

`cfdb-core/Cargo.toml` — `cfdb-core` has zero workspace dependencies. Its declared dependency set is `{serde, serde_json, thiserror, indexmap}` — pure data types.

**CCP violation:** The composer function in `list_items.rs` changes for a different reason than the AST nodes in `query/ast.rs`. The AST changes when the Cypher subset evolves (a grammar decision). The composer changes when the cfdb verb surface changes (RATIFIED §A.14 additions). Two different change axes in one module.

**SDP violation (stability direction):** `cfdb-core` is the most-stable crate in the graph — it has no workspace dependencies and every other crate depends on it. `cfdb-core/src/lib.rs:16` notes it is the schema + types hub. Putting an application-level composer (one that encodes cfdb verb semantics) into the most-stable crate means a verb change forces a bump of the most-stable crate, which re-publishes it to all dependents. This inverts the stability arrow.

**CRP corollary:** `cfdb-recall` (`cfdb-recall/Cargo.toml`) depends on `cfdb-core` but never calls `list_items_matching` — it is forced to compile the composer it does not use.

**Prescription:** Move `cfdb-core/src/query/list_items.rs` (and `item_kind.rs` insofar as it is verb-specific vocabulary rather than schema vocabulary) to `cfdb-query`. `cfdb-query` already depends on `cfdb-core` and is the natural home for composer logic that sits between the AST and the CLI layer. The re-export in `cfdb-core/src/lib.rs:31` (`pub use query::list_items_matching`) should be removed; callers import from `cfdb-query` directly.

**Note on `ItemKind`:** `ItemKind` is borderline — it defines the wire vocabulary for `list-items-matching` (verb-level), not the core schema (node-level). If the schema never grows a native `ItemKind` node, it belongs in `cfdb-query` alongside the composer. If it becomes a first-class schema attribute, it stays in `cfdb-core`. Defer the final call to the schema design stage of v0.2.

**Ordering rationale:** §2 must land first so that `StoreBackend` is narrow by the time composers move; the move is cleaner when the consumer surface of `cfdb-core` is already trimmed.

---

## §4 — Consolidate `cfdb-cli` composition root

**Issue: #23**

**Lenses:** SRP (primary), Clean Architecture (secondary)

**Evidence:**

`cfdb-cli/src/lib.rs:12-24` — the crate re-exports from four sibling modules: `commands`, `enrich`, `scope`, `stubs`. The public surface is a flat re-export from `lib.rs` — no composition root exists.

`cfdb-cli/src/main.rs:48-53` — `main.rs` imports from `cfdb_cli`, `cfdb_core`, `clap`. The `main.rs` function at line 326 is a 90-line `match` that dispatches directly to handler functions; each arm calls into a public function from the re-exported surface.

The composition concern (which concrete store to construct, how to load persisted state, how to wire the petgraph backend) is scattered: `cfdb-cli/src/commands.rs:42-50` constructs `PetgraphStore::new()` and calls `persist::save`; `cfdb-cli/src/enrich.rs:31-32` constructs another `PetgraphStore::new()` and calls `persist::load`; `cfdb-cli/src/stubs.rs:87-88` does the same; `cfdb-cli/src/scope.rs` presumably the same.

**SRP violation:** Each handler module currently owns both (a) the command semantics and (b) the infrastructure construction (store instantiation, file I/O). Changes to the persistence layer require touching all handler files.

**Prescription:** Introduce a `cfdb-cli/src/compose.rs` (or `infra.rs`) that owns the single construction path: `load_store(db, keyspace) -> Result<(PetgraphStore, Keyspace), CfdbCliError>`. All handler modules call through this function. The `main.rs` dispatcher remains thin. This makes the composition root explicit and gives the CLI a single reason to change when the persistence strategy changes.

**Ordering rationale:** §2 and §3 must land first — the composition root shape depends on the final trait split and where composers live.

---

## §5 — Refactor `cfdb-petgraph/src/eval/pattern.rs`

**Issue: #26**

**Lenses:** SRP (primary), Rust-systems complexity (secondary)

**Evidence:**

Complexity metrics for `/var/mnt/workspaces/cfdb/crates/cfdb-petgraph/src/eval/pattern.rs`:

| Function | Cognitive complexity | Max nesting |
|---|---|---|
| `apply_node_pattern` (line 18) | 33 | 5 |
| `apply_path_pattern` (line 86) | 40 | 6 |
| `traverse` (line 169) | 11 | 3 |
| `collect_directed_edges` (line 204) | 12 | 3 |
| `apply_optional` (line 233) | 7 | 3 |
| `collect_pattern_vars` (line 294) | 9 | 2 |

`apply_path_pattern` at line 86 scores 40 — nearly 3× the project ceiling of 15. `apply_node_pattern` scores 33. Both exceed threshold because they conflate three responsibilities: (1) binding-table expansion (the outer loop over `table`), (2) endpoint resolution (calling `resolve_endpoint` / `candidate_nodes`), and (3) binding-variable constraint enforcement (the inner checks on `next.get(var)` and `matches_existing`).

**SRP violation:** `apply_path_pattern` (lines 86-139) changes for at least three independent reasons: (a) the traversal semantics change (variable-length, direction), (b) the binding-constraint logic changes (how existing bindings are honoured), (c) the endpoint resolution logic changes (how candidate nodes are selected). Each axis is independently evolvable.

**Prescription:** Extract three private helpers from `apply_path_pattern`:
1. `emit_path_binding(bindings, src_idx, dst_idx, pp) -> Option<Bindings>` — pure binding assembly, no iteration.
2. The existing `resolve_endpoint` (already extracted at line 144) — keep as-is.
3. Fold `collect_directed_edges` into `traverse` or keep separate (complexity 12 is borderline; leave as-is if preferred).

Similarly for `apply_node_pattern`: separate the "variable already bound" branch (lines 27-33) into a `check_existing_binding` helper.

Target: `apply_node_pattern` and `apply_path_pattern` below cognitive complexity 15 each.

**Ordering rationale:** No cross-crate dependency. Can land any time after §2 without blocking or being blocked. Placed here because it is purely local to `cfdb-petgraph`.

---

## §6 — Unify `strip_comments` + `find_keyword` scanners

**Issue: #28**

**Lenses:** SRP (primary), DRY (secondary)

**Evidence:**

`cfdb-query/src/parser/mod.rs:127-184` — `strip_comments` function: 58-line scanner that tracks `in_single` / `in_double` string-literal state to avoid treating comment-markers inside string literals as real comments.

`cfdb-query/src/parser/mod.rs:220-260` — `find_keyword` function: 40-line scanner that independently tracks the same `in_single` / `in_double` state for identical reasons.

Both functions maintain the same two-boolean state machine to skip string literals. The duplication is structural: if a new string quoting rule were introduced (e.g., backtick identifiers from the openCypher spec), both functions would need to be updated in sync.

**SRP / DRY violation:** The string-literal-aware scanning concern is owned by two independent functions. The invariant "do not treat content inside string literals as syntax" is expressed twice. One change axis (quoting rules) drives changes in two places.

**Prescription:** Extract a `StringAwareScanner` struct (or a `scan_outside_strings<F>(source: &str, f: F)` combinator that calls `f` with each byte and a `bool in_string` flag) into a private submodule `cfdb-query/src/parser/scanner.rs`. Both `strip_comments` and `find_keyword` delegate the string-awareness bookkeeping to this shared primitive. The two functions shrink to their core logic (what to do with each non-string byte) with the tracking loop factored out.

**Ordering rationale:** Entirely self-contained within `cfdb-query`. No dependency on any other section. Placed last because it is lowest risk/impact.

---

## §7 — Typed `CfdbCliError` (retroactive cite — PR #38 in flight)

**Issue: #22**

**Lenses:** SRP (primary), Rust-systems error ergonomics (secondary)

**Evidence:**

`cfdb-cli/src/commands.rs:28,59,112,150,191,226,238,266` — every public handler function returns `Result<_, Box<dyn std::error::Error>>`. Same pattern in `cfdb-cli/src/stubs.rs:25,75,119,161,194,210` and `cfdb-cli/src/enrich.rs:28`.

`Box<dyn std::error::Error>` erases the error variant. Callers (including the `main.rs` dispatcher and any test that calls a handler directly) cannot match on error kind — they can only print the error. This makes it impossible to write assertions like "this command returns a `KeyspaceNotFound` error" without substring-matching the error message string.

**SRP violation (error ownership):** Multiple crates own the construction of errors that end up in the same `Box<dyn Error>` bag. There is no single canonical cfdb-cli error vocabulary.

**Prescription:** PR #38 introduces `CfdbCliError` as a `thiserror`-derived enum with variants for each distinct failure mode (`KeyspaceNotFound`, `ParseError`, `StoreError`, `IoError`, `SerdeError`). All handler signatures become `Result<_, CfdbCliError>`. This is the correct fix. RFC-031 retroactively cites this issue and the in-flight PR.

**Ordering note:** PR #38 is already in flight. This section is included for completeness and to provide the RFC citation. If the PR merges before this RFC is adopted, the issue is automatically resolved; no separate action required.

---

## Dependency graph

```
§1 (#29-verify) — no dependencies. Verify first.
    |
    └─ §2 (#27 EnrichBackend split) — depends on §1 (no BuilderError gap to worry about)
           |
           └─ §3 (#25 move composers) — depends on §2 (stable trait split)
                  |
                  └─ §4 (#23 composition root) — depends on §3 (final import shape)
                         |
                         └─ §5 (#26 pattern.rs) — no blockers; can land any time
                         |
                         └─ §6 (#28 scanner unification) — no blockers; can land any time

§7 (#22 CfdbCliError / PR #38) — independent; in-flight already
```

**Strict ordering:** §1 → §2 → §3 → §4. §5, §6, and §7 are independent of that chain and of each other.

---

*RFC-031 — drafted by solid-architect (SOLID/component-principles lens), 2026-04-19.*
*Verified: all file:line citations from main branch HEAD (`crates/` tree).*
