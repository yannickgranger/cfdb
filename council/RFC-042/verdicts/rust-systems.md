# RFC-042 verdict — rust-systems

**Verdict:** REQUEST CHANGES
**Author:** rust-systems sub-agent
**Date:** 2026-05-17

## D1. Verdict on the RFC as written

### Finding RS-1 (BLOCKING): `enrich_reachability` is a trait method — the filter parameter cannot be added as specified

RFC §3.3 describes the dual-BFS design (Option A) as follows: "`enrich_reachability` gains an `entry_kind_filter: Option<&BTreeSet<&str>>` parameter and is invoked twice from the orchestrator."

This is incoherent with the existing type structure. `enrich_reachability` is not a free function — it is a method on the `EnrichBackend` trait (`crates/cfdb-core/src/enrich.rs:177`):

```
fn enrich_reachability(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError>
```

Adding `entry_kind_filter: Option<&BTreeSet<&str>>` to this signature changes the trait definition in `cfdb-core`. That is a breaking change to every `EnrichBackend` implementor (the trait has `Send + Sync` bounds; the method is virtual). The concrete override lives at `crates/cfdb-petgraph/src/enrich_backend.rs:151-163`, where `enrich_reachability` calls the internal `reachability::run(state)` with no filter parameter. The RFC text never acknowledges the trait-method boundary or says what the signature change looks like in `cfdb-core/src/enrich.rs`.

The RFC's phrase "invoked twice from the orchestrator" is also ambiguous: is the caller `cfdb-cli/src/enrich.rs` calling `store.enrich_reachability(ks)` twice with different arguments? That would require the trait method to accept the filter. Or is the second invocation an entirely separate internal call inside `PetgraphStore::enrich_reachability` that bypasses the trait? The RFC does not resolve this.

**Required change:** RFC §3.3 must specify the exact signature change in `cfdb-core/src/enrich.rs` and its ripple into every `impl EnrichBackend`. Alternatively, the RFC may specify that `entry_kind_filter` is NOT part of the trait surface and the dual-BFS is driven by a separate `enrich_reachability_production(&mut self, keyspace: &Keyspace)` method on `PetgraphStore` directly (not on `EnrichBackend`). Either resolution is acceptable; the silence is not.

**Proposed edit shape for §3.3:**
> Add a subsection "Trait surface impact": state whether `EnrichBackend::enrich_reachability` gains the filter parameter (breaking change to all impls) or whether the production-filtered variant is a `PetgraphStore`-specific method not on the trait, called directly by the CLI orchestrator. If the filter is added to the trait, list every file that must change: `cfdb-core/src/enrich.rs` (trait def), the `TestBackend` in its own test module (line 267 of `cfdb-core/src/enrich.rs`), and any downstream crate that `impl EnrichBackend for`.

### Finding RS-2 (NON-BLOCKING, requires RFC acknowledgement): attribute-walk segmentation is `ra_ap_syntax`, not `syn`

RFC §3.1 says the probes "walk `fn_ast.attrs()`, extract the last `::`-separated segment of each attribute's meta path." The existing `has_tool_attr` at `registers_param.rs:56-71` confirms the implementation uses `ra_ap_syntax::ast::Attr::meta().and_then(|m| m.path())` and `.syntax().to_string().rsplit("::")`. This is the `ra_ap_syntax` (rust-analyzer syntax tree) path, not `syn`. The RFC §3.1 header says "textual-attribute discipline RFC-037 §3.1" but the module-level doc of `entry_point_emitter.rs` (line 1-26) is explicit that this is HIR-backed. The BRIEF §3.1 asks the rust-systems lens to verify whether the code uses `ra_ap_hir`'s AST or `syn`. Answer: it uses `ra_ap_syntax::ast` (the same as `ra_ap_hir`'s syntax layer) via `attr.meta().and_then(|m| m.path()).syntax().to_string()`. This is a string-textual extraction on the syntax tree, not trait resolution. Consistent with RFC-037 §3.1 contract.

### Finding RS-3 (NON-BLOCKING): `#[cfg(test)]` exclusion is correctly handled by the existing probe shape

RFC §3.1 states `has_test_attr` returns `true` when "any attribute's last path segment is `test`" and adds the caveat "other than `cfg(test)` (cfg is not in the attr path)." This is correct given the probe implementation at `registers_param.rs:56-71`. `#[cfg(test)]` has an outer path segment of `cfg`, not `test` — `rsplit("::")` on `"cfg"` yields `"cfg"` as the last segment. The `#[cfg(test)]` form does NOT carry `test` as the last segment of the attribute path; it carries it as a token-tree argument to `cfg`. The probe is safe.

The case of `#[cfg(test)] fn foo() { ... }` (a fn with a `cfg` attribute AND separately annotated `#[test]`) does not create a false positive: the `cfg` attribute has last-segment `cfg`, the `test` attribute has last-segment `test`. The probe visits all attributes and fires on `test`. This is correct behavior — such a fn should be classified as `kind=test`.

However, the RFC does not document the boundary between attribute-path matching and attribute-argument matching. The note "cfg is not in the attr path" requires the reader to know the rust-analyzer syntax model. A one-sentence clarification in §3.1 would eliminate future maintainer confusion: "the probe reads `attr.meta().path()`, which for `#[cfg(test)]` yields path `cfg` (not `test`) — the `test` inside `cfg(...)` is a token-tree argument, not a path segment." This is non-blocking but the RFC should add it.

### Finding RS-4 (NON-BLOCKING): `#[bench]` + `#[test]` mutual exclusion claim requires qualification

RFC §3.1 item 4 states "rustc lints reject a fn with both". This is accurate for `#[bench]` (unstable libtest harness) with `#[test]` in current rustc. However, the probe is textual and does not compile-check; if malformed or macro-generated input carries both, the `if … else if` dispatch probes `test` before `bench` per the RFC's ordering, so the fn would be classified as `test`. This is a deterministic outcome and the RFC correctly documents the ordering for "stable ordering." Acceptable — no change required.

### Finding RS-5 (NON-BLOCKING): feature-flag split between attribute detection and file-location detection is underspecified

`cfdb-hir-extractor` has no `[features]` section in its `Cargo.toml` (verified: the file has only `[package]`, `[dependencies]`, `[dev-dependencies]`). The HIR-extractor crate is itself accessed only through the `hir` feature on `cfdb-cli` (per the Cargo.toml comment: "NEVER add `cfdb-hir-extractor` as a direct dep of `cfdb-cli` or `cfdb-petgraph`"). This means the entire `cfdb-hir-extractor` runs only when the `hir` feature is active — attribute-based AND file-location-based detection are both gated on `hir`.

RFC §2 ("Does NOT ship: SchemaVersion bump") implies the extractor changes are feature-gated, but §3.1 does not explicitly state that both detection modes share the same `hir` feature gate. The RFC should state: "All new emission (`kind=test`, `kind=bench`) requires `--features hir` on extraction, exactly as `kind=mcp_tool` does today. There is no partial syn-only path for test/bench detection." This prevents a future maintainer from assuming file-location detection could be moved to the syn extractor.

### Finding RS-6 (NON-BLOCKING): dual-BFS vs. single multi-source BFS — the RFC omits a third option without justification

RFC §3.3 ratifies Option A (two sequential BFS runs) over Option B (post-filter at classifier). A third option — a single BFS with a visit-time kind-mask that accumulates two separate reach-count maps in one traversal — is neither mentioned nor dismissed. The single-traversal option would be O(V+E) rather than 2×O(V+E) for the BFS itself (the traversal cost is the same; per-node attribution doubles in one pass rather than two). For qbot-core scale (~85k items, call-graph density unknown), the difference is likely insignificant. The RFC's omission is not a correctness error, but a complete trade-off analysis should mention it. Non-blocking: the BRIEF §3.3 explicitly says "council debates shape, not necessity" and the omitted option is an optimization, not a correctness concern.

### Finding RS-7 (NON-BLOCKING): sort-key includes `kind` values via the `ep_id` format

RFC §4 invariant G1 states "new emission paths inherit the existing sort discipline." Verified: `emit()` at `entry_point_emitter.rs:349` constructs `ep_id = format!("entrypoint:{kind}:{handler_qname}")`. The final sort at line 105 sorts by `a.id.cmp(&b.id)`. Since `kind` is embedded in the id string, `kind=bench` and `kind=test` sort lexicographically below `kind=cli_command` (`b` < `c`) and below `kind=mcp_tool` (`b`/`t` < `m`). This is deterministic across `ra_ap_syntax` versions as long as the attribute walk visits attributes in source order (which the `ast::HasAttrs::attrs()` iterator guarantees per the rust-analyzer invariant on syntax tree ordering). G1 is satisfied.

### Finding RS-8 (NON-BLOCKING): graph-specs-rust existing queries are safe from the new `reachable_from_production_entry` attribute

The BRIEF §7 convener note raises the question of whether any existing `yg/graph-specs-rust/.cfdb/queries/*.cypher` query reads `reachable_from_entry` or `reachable_from_production_entry` attribute names. Verified: all four queries (`arch-ban-unwrap-domain-ports.cypher`, `arch-context-no-application-in-domain.cypher`, `arch-context-no-cross-layer-unwrap.cypher`, `arch-context-no-syn-in-domain.cypher`) match on `(caller:Item)-[:INVOKES_AT]->(cs:CallSite)` patterns and `caller.is_test = false` / `cs.is_test = false` filters. None reads `reachable_from_entry` or any `:EntryPoint` node. The new attribute `reachable_from_production_entry` written by the dual-BFS is inert from graph-specs-rust's perspective. Cross-dogfood regression is a no-op as the RFC §4 SchemaVersion stability invariant predicts.

### Summary judgment

RS-1 is blocking: the RFC cannot be ratified until the trait-surface impact of the `entry_kind_filter` parameter is specified. All other findings are non-blocking documentation gaps or implementation notes. If the RFC author resolves RS-1 by adding a "Trait surface impact" subsection to §3.3, the rust-systems lens will re-review as RATIFY.

---

## D2. Tests prescription

### Slice 042-A — extractor `:EntryPoint{kind=test|bench}` + fixture

- **Unit:** Construct synthetic `ast::Fn` values by parsing Rust source fragments via `ra_ap_syntax::SourceFile::parse(src, Edition::Edition2021)` and extracting the first `ast::Fn` descendant. Inputs:
  - `"#[test] fn f() {}"` → `has_test_attr` returns `true`
  - `"#[tokio::test] fn f() {}"` → `has_test_attr` returns `true` (last segment `test`)
  - `"#[async_std::test] fn f() {}"` → `has_test_attr` returns `true`
  - `"#[given] fn f() {}"` → `has_test_attr` returns `true`
  - `"#[when] fn f() {}"` → `has_test_attr` returns `true`
  - `"#[then] fn f() {}"` → `has_test_attr` returns `true`
  - `"#[bench] fn f(_b: &mut Bencher) {}"` → `has_bench_attr` returns `true`, `has_test_attr` returns `false`
  - `"#[tool] fn f() {}"` → `has_test_attr` returns `false`, `has_bench_attr` returns `false` (ensures no false positive for existing kinds)
  - `"#[cfg(test)] fn f() {}"` → `has_test_attr` returns `false` (the `cfg` attribute's last segment is `cfg`, not `test`)
  - `"fn f() {}"` (no attr) → both return `false`
  - These tests are pure: `has_test_attr` and `has_bench_attr` take `&ast::Fn`, return `bool`, zero I/O. Run in `crates/cfdb-hir-extractor/src/entry_point_emitter/registers_param.rs` unit test module or a sibling `tests/` file.

- **Self dogfood (cfdb on cfdb):** `cfdb extract --workspace . --features hir --db .cfdb/db --keyspace cfdb-042a` then `MATCH (e:EntryPoint{kind:"test"}) RETURN count(e) AS n`. Lower bound N = count of `#[test]` / `#[tokio::test]` / `#[given]`/`#[when]`/`#[then]` annotated fns in cfdb's own `crates/*/{src,tests}/` (grepped with `rg "#\[test\]|#\[tokio::test\]|#\[given\]|#\[when\]|#\[then\]" crates/` before the extract — the grep count is the floor). Assert `n >= grep_count`. Rationale: the dogfood tree is live data; the lower bound is the known attribute-annotated count; any discrepancy surfaces a detection gap.

- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA `913f06f`):** Re-run `ci/cross-dogfood.sh`. All four existing queries read `cs.callee_path` / `caller.crate` / `caller.is_test` — none reads `:EntryPoint` kind or `reachable_from_*` attrs. Assert zero new rows on any existing rule. This is a no-op regression check by design.

- **Target dogfood (qbot-core at pinned SHA):** Report `MATCH (e:EntryPoint) WHERE e.kind IN ["test","bench"] RETURN e.kind, count(e)` in the PR body. Expected to show several hundred `kind=test` entries given the `#[test]` density in qbot-core; bench count expected small but non-zero given `#[bench]` / `benches/` usage. Reviewer sanity-check only; no lower bound enforced at merge time.

### Slice 042-B — scope `--production-only` + dual-BFS + classifier rule

- **Unit:** Test `collect_seeds` (the internal function at `reachability.rs:115`) in isolation. Construct a synthetic `KeyspaceState` with:
  - Two `:EntryPoint` nodes: one with `kind="mcp_tool"`, one with `kind="test"`.
  - Each `EXPOSES`-targets a distinct `:Item` node.
  - Call `collect_seeds` with `entry_kind_filter = None` → assert both `:Item` seeds returned.
  - Call `collect_seeds` with `entry_kind_filter = Some({"mcp_tool", "cli_command", ...})` → assert only the mcp_tool-exposed `:Item` is returned.
  - Note: this test requires RS-1 to be resolved — the `entry_kind_filter` parameter must exist on the callable surface first.
  - Additionally test `accumulate_reach_counts` invariant: under either filter, items not transitively reached via `CALLS*` get `count=0`.

- **Self dogfood (cfdb on cfdb):** `cfdb scope --context cfdb-extract` default mode vs `cfdb scope --context cfdb-extract --production-only`. The difference in `unwired` count must be ≥ 1 (at minimum one item in cfdb's own tree is reachable only from integration test code, not from any production entry point). The exact delta is reported in the PR body. Rationale: if the delta is zero, either the new entry points were not extracted (RS-1 unresolved) or cfdb has no test-only-reached code (unlikely given `crates/cfdb-*/tests/`).

- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA):** Zero regression on existing rules — the `--production-only` flag is opt-in and the new `reachable_from_production_entry` attribute is inert for all existing graph-specs-rust queries (verified in RS-8 above).

- **Target dogfood (qbot-core at pinned SHA):** Report `cfdb scope --context trading` `unwired` count in default mode and in `--production-only` mode. Expected: default count drops ≥ 30% from 2057 (RFC §7 prediction); `--production-only` count remains near 2057. Report both numbers in PR body with a diff table.

### Slice 042-C — empirical close-out on qbot-core

- **Tests:** none — rationale: cross-repo empirical report, not code. (Per RFC §7 and CLAUDE.md §2.5 escape-hatch.)

---

## D3. Dual-dogfood discipline notes

**Self dogfood lower-bound fragility (042-A):** The self-dogfood assertion for 042-A proposes `count(e) >= N` where N is a pre-extract grep count. The grep count is a live lower bound — it grows as cfdb adds tests. The implementer must run the grep IMMEDIATELY before the extract (in the same CI step) to avoid count skew when the tree changes between runs. The CI script for 042-A self-dogfood should be: `GREP_COUNT=$(rg -c "#\[test\]|#\[tokio::test\]|#\[given\]|#\[when\]|#\[then\]" --include="*.rs" crates/); cfdb extract ...; QUERY_COUNT=$(cfdb query ...); [ "$QUERY_COUNT" -ge "$GREP_COUNT" ]`.

**Cross dogfood attribute-name safety (042-A and 042-B):** As established in RS-8, the new `reachable_from_production_entry` attribute does not appear in any existing graph-specs-rust query. The cross-dogfood check is genuinely a no-op regression. However, if a future graph-specs-rust query is added that reads `reachable_from_entry`, it will silently work with the new dual-BFS output (since `reachable_from_entry` is unchanged by RFC-042). There is no cross-dogfood theater here.

**042-B self-dogfood lower-bound:** The assertion "delta ≥ 1" for the default vs `--production-only` count difference is a weak bound. If it fails (delta = 0), the most likely causes are: (a) RS-1 unresolved — the dual-BFS was not actually invoked, so `reachable_from_production_entry` is absent or identical to `reachable_from_entry`; (b) cfdb's own test infrastructure happens to reach all items that production reaches (unlikely). The implementer should assert `delta >= 1` AND separately assert that `reachable_from_production_entry` is present as an attribute on at least one `:Item` node in the self-dogfood keyspace.

---

## D4. Graph-specs-rust update against real code

**Proposed Cypher:**

```cypher
// rust-systems-dyn-trait-test-only-impl.cypher
//
// Rule: a trait that is used as `dyn Trait` (a vtable dispatch site)
// but whose only concrete implementations are reached exclusively from
// test entry points = an unused vtable in production. The vtable entry
// could be removed (monomorphise the trait out of production paths) or
// the test coverage is the only user, indicating the abstraction leaks
// into production wiring unnecessarily.
//
// Rationale: from the rust-systems lens, `dyn Trait` constructions
// imply a vtable allocation and pointer-indirection cost that is only
// justified when multiple concrete impls must be dispatched at runtime.
// If the only callers of the `dyn Trait` invocation site are test entry
// points, the vtable cost exists in the production binary for no
// production benefit.
//
// Detection shape: find items whose callee_path references a trait
// method (heuristic: callee_path ends with a method name that matches
// a known trait's method names — this is approximate without full type
// resolution). Filter to items that are NOT reachable_from_production_entry.
//
// Note: this rule requires RFC-042 attributes to be present in the
// keyspace (`reachable_from_production_entry`). On a pre-RFC-042
// keyspace, the WHERE clause on `reachable_from_production_entry = false`
// will match no items (the attribute is absent / null), producing zero
// rows safely.
//
// Expected: zero rows on a clean graph-specs-rust tree.
// Intent: zero-violation policy from day one (graph-specs-rust has no
// production `dyn Trait` dispatch sites that are test-only reached).

MATCH (item:Item)
WHERE item.reachable_from_production_entry = false
  AND item.reachable_from_entry = true
  AND item.is_test = false
  AND item.kind IN ["Fn", "Method"]
RETURN item.qname, item.crate, item.file, item.bounded_context
ORDER BY item.crate, item.qname
```

**Filed at (or proposed for):** `.cfdb/queries/rust-systems-test-only-reachable-non-test-items.cypher` on `yg/graph-specs-rust`.

**Citation against current graph-specs-rust pinned SHA `913f06f`:** The four existing queries at `/var/mnt/workspaces/graph-specs-rust/.cfdb/queries/` match on `caller.is_test = false` already, meaning the graph-specs-rust tree is expected to have `is_test` populated on `:Item` nodes. The new query adds the `reachable_from_production_entry` filter. On the pinned SHA, the attribute is absent pre-RFC-042 — the WHERE clause produces zero rows (absent attribute does not match `= false` in cfdb's Cypher subset). Zero-finding expected at pinned SHA.

**Intent:** Zero-violation policy from day one. After RFC-042 lands and graph-specs-rust re-extracts with the new entry-point kinds, any item in graph-specs-rust that is reachable from test entry points but NOT from production entry points, AND that item is not itself a test item, represents a production code path only exercised by tests. In graph-specs-rust (a library crate with no production binary), this class is expected to be empty or near-zero; if non-zero, it identifies API surface that might be dead in the production use case.

**Rationale:** From the rust-systems lens, the most important invariant RFC-042 enables is "no dead vtable entries in production binaries." The Cypher above is the foundation query. A more precise rule would match specifically on `:Item` nodes that are targets of `dyn Trait` call sites (requiring `callee_path` to resolve against a trait method); that requires type resolution the current cfdb Cypher subset cannot express. The proposed query is the accessible approximation: non-test items reachable only from test entry points. This is expressible in the current Cypher subset, cites real files in the pinned companion, and implements a zero-violation policy rather than a cleanup-driving rule.

---

## Change requests summary

| ID | Blocking | Section | Required edit |
|---|---|---|---|
| RS-1 | YES | §3.3 | Add "Trait surface impact" subsection specifying whether `EnrichBackend::enrich_reachability` gains the `entry_kind_filter` parameter (with ripple into `cfdb-core/src/enrich.rs:177` and all impls) or whether the production-filtered variant is a separate `PetgraphStore`-specific method not on the trait. |
| RS-3 | NO | §3.1 | Add one sentence clarifying that `attr.meta().path()` for `#[cfg(test)]` yields path `cfg`, not `test` — the `test` token is an argument, not a path segment. |
| RS-5 | NO | §3.1 or §2 | State explicitly: "All test/bench detection (attribute-based and file-location-based) requires `--features hir`; there is no syn-only partial path." |
