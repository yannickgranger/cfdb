# RFC-043 verdict — ddd-specialist

**Verdict:** REQUEST CHANGES
**Author:** ddd-specialist sub-agent
**Date:** 2026-05-18

---

## D1. Verdict on the RFC as written

### Finding 1 — Homonym: `callee_resolved=true` now encodes two epistemically distinct facts

This is the headline vocabulary trap for RFC-043, analogous to the `kind=test` vs `is_test` three-way homonym caught in RFC-042.

Pre-RFC-043, `:CallSite{callee_resolved=true, resolver="hir"}` has a precise, single meaning established at `crates/cfdb-core/src/schema/labels.rs:232-231` (SchemaVersion V0_1_3 docstring): "syn-or-HIR resolved via the HIR extractor's *current* inference model — i.e., a model in which proc-macro-expanded signatures are NOT in scope." Operationally: the resolver knows the callee because static type inference succeeded on un-expanded source. The predicate is high-precision by construction of the loader configuration (`ProcMacroServerChoice::None`).

Post-RFC-043, the same attribute `callee_resolved=true` on a `resolver="hir"` `:CallSite` means one of two distinct epistemic facts depending on the run configuration:

- **Fact A (default path, `proc_macro_status="enabled"`):** "The HIR resolved the callee with proc-macro-expanded type information available — a richer, higher-recall claim than pre-043." The receiver type was inferred after macro expansion, so the resolution crosses a macro boundary.
- **Fact B (disabled/degraded path, `proc_macro_status="disabled"` or `"degraded"`):** "The HIR resolved the callee in the pre-043 inference model — same semantics as the pre-043 attribute." The resolution does NOT cross a macro boundary.

These are different propositions about the resolution's epistemic basis. A query author who writes `MATCH (cs:CallSite{callee_resolved: true, resolver: "hir"})` on a mixed-vintage keyspace corpus (some extracted with proc-macros enabled, some with the flag disabled via `--no-proc-macro`) will get nodes whose `callee_resolved=true` means two different things depending on the `proc_macro_status` of the keyspace each node came from. The attribute is a homonym: same spelling, two distinct semantics gated on keyspace-level metadata that is NOT on the `:CallSite` node itself.

**RFC §3.3** makes this a metadata-only flag — it lives on the keyspace header, not on individual `:CallSite` nodes. This design ensures that a single query on a single keyspace is internally consistent (every `callee_resolved=true` node in one keyspace was resolved under the same macro-expansion policy). However, a consumer who:
1. Archives or caches multiple keyspaces (different extract runs, different workspaces),
2. Runs queries across them (e.g., a multi-workspace call-graph federation), or
3. Interprets `callee_resolved=true` as a durable fact (stores it in a downstream system),

will encounter the homonym without being warned by the schema vocabulary.

**The homonym is NOT blocking on its own** for a single-keyspace consumer. Within one keyspace, the `proc_macro_status` is uniform and the attribute is consistent. But RFC §3.3's current schema descriptor text for `callee_resolved` (at `crates/cfdb-core/src/schema/describe/nodes.rs:280`) does not acknowledge that the semantics of `true` depend on the `proc_macro_status` of the containing keyspace. A future query author reading `cfdb describe` sees only: "`true` when method dispatch / re-export / trait impl was resolved via HIR" — no mention that "via HIR" now means "via HIR with variable macro-expansion coverage."

**Change request C1:** The RFC §4 (Invariants) should add an invariant I7: "The descriptor for `:CallSite.callee_resolved` MUST be updated to note that, post-RFC-043, the semantic precision of `callee_resolved=true` co-varies with the keyspace's `proc_macro_status`. Consumers federating across keyspaces MUST join on `proc_macro_status` before comparing resolved call sets." The descriptor edit goes in `crates/cfdb-core/src/schema/describe/nodes.rs:280` and is a descriptor-only change. The RFC §2 scope block should name this as a deliverable.

### Finding 2 — `callee_path` stability under `#[async_trait]` desugars: the phantom poll-call risk

RFC §5.2 names the question: when `#[async_trait]` desugars `async fn foo(&self) -> Bar` to `fn foo(&self) -> Pin<Box<dyn Future<Output=Bar>>>`, what does the `:CallSite.callee_path` for `caller.foo().await` contain?

The current `call_site_emitter.rs` populates `callee_path` at line 339 via `callee_qname`, which is computed by `function_qname(sema, callee)` at line 320. `function_qname` descends through `assoc.container(db)` → `normalize_impl_target` → `method_qname`. The callee is a `hir::Function` resolved by `resolve_method_call`. With proc-macros disabled, `resolve_method_call` returns `None` on a `#[async_trait]`-rewritten receiver because the type inference collapses. With proc-macros enabled, `resolve_method_call` may succeed — the question is WHAT it returns.

The risk is specifically this: `#[async_trait]` rewrites `async fn foo()` at the trait level to `fn foo(&self) -> Pin<Box<dyn Future<Output=Bar>>>`, and the call expression `caller.foo().await` desugars at the HIR level to a `Future::poll()` invocation on an intermediate object. If ra-ap-hir, with proc-macro expansion enabled, sees the DESUGARED HIR rather than the source-level `foo()` call, `resolve_method_call` on the `foo()` call expression could return either:
- The macro-expanded `foo` (the renamed, boxed version) — path stable, name correct but type signature no longer matches source.
- Or `None` on the `.await` expression's implicit `poll` dispatch — in which case `.await` emits no `:CallSite`, which is the same behavior as today (no regression, no phantom).

The key observation from `call_site_emitter.rs:113-121`: the file iterator filters to `vfs_path_to_pathbuf(vfs_path)?` returning `Some` — i.e., concrete filesystem paths only. Macro-expanded virtual VFS files (which live in ra-ap-vfs as in-memory paths) return `None` from `vfs_path_to_pathbuf` at line 114 and are silently excluded from the walk. This means synthetic files injected by proc-macro expansion are never walked by `walk_file`. Call expressions that exist ONLY in the macro-expanded virtual file (e.g., a poll-loop introduced by `#[async_trait]` in the generated code) will never be visited, so no phantom `:CallSite` nodes are emitted for them.

This is actually protective behavior. However, it is implicit and undocumented. The protection relies on:
(a) `vfs_path_to_pathbuf` returning `None` for `VfsPath::Virtual` (the in-memory variant),
(b) the filter at line 114 dropping these.

If ra-ap-vfs or ra-ap-load-cargo ever changes the VFS path representation for proc-macro expansion outputs from virtual to a temp-file-backed path (which would be a concrete filesystem path), the filter would no longer protect against phantom call sites.

The `callee_path` for a source-level `caller.foo().await` where `foo` is `#[async_trait]`-rewritten: with proc-macros enabled, ra-ap-hir resolves the `foo()` call expression in the source file to the macro-generated `foo` implementation. The `callee_qname` produced by `function_qname` will be the HIR's name for the generated function. Whether this matches the pre-043 syn extractor's textual `callee_path` for the same call is the cross-extractor ID stability concern.

**Change request C2:** RFC §3.1 should add an explicit note that the virtual-VFS filter in `call_site_emitter.rs:113-121` is the mechanism by which phantom macro-generated call sites are excluded, and that this property MUST be preserved if `vfs_path_to_pathbuf` is ever changed. This is a documentation obligation, not a code change. The RFC §4 invariants should include: "I8 — No phantom macro-generated call sites: `:CallSite` nodes are emitted only for call expressions in concrete filesystem-backed source files. VFS virtual paths (proc-macro expansion outputs) are excluded by the VfsPath filter in `call_site_emitter::extract_call_sites_attached`. This invariant MUST be verified in 043-A's determinism fixture by asserting that the fixture produces zero `:CallSite` nodes whose `file` attribute names a path not present in the fixture's workspace on disk."

### Finding 3 — `proc_macro_status`: keyspace metadata is the right shape for this concept

RFC §3.3's decision to make `proc_macro_status` a keyspace-level metadata attribute (not a per-`:Item` flag) is DDD-correct. The question "was proc-macro expansion available during this extract?" is a **fact about the extraction run**, not a fact about individual items. It belongs in the provenance layer of the keyspace header alongside `cfdb_version` and `schema_version`. A per-`:Item` `is_proc_macro_touched: bool` flag would be a different and harder-to-answer question — it would require the extractor to track, for each resolved call, whether the resolution path crossed a macro-expanded module. That is not computable from the current HIR resolution API without invasive changes.

The DDD verdict on D6: keyspace metadata is correct. **No per-`:Item` flag is needed.** The correct consumer pattern — described in RFC §3.3 and reproduced in Finding 1's change request — is to check `proc_macro_status` at the keyspace level before interpreting `callee_resolved` precision.

However, there is a vocabulary gap: `proc_macro_status` is described in RFC §3.3 as a "top-level keyspace metadata attribute" but it is NOT currently present in `cfdb-core::SchemaDescribe`. The RFC's §4 invariant I3 says "Schema unchanged — no `SchemaVersion` bump. New `proc_macro_status` is keyspace metadata, not part of the `SchemaDescribe`-visible node/edge vocabulary." This is correct about SchemaVersion — no bump is needed because `proc_macro_status` is not a node/edge attribute. But it creates a discoverability gap: a consumer reading `cfdb schema-describe` will not see any reference to `proc_macro_status` unless the schema-describe output is extended to include keyspace-level metadata fields.

**Change request C3:** RFC §2 (Scope) should add as a deliverable: "Extend `cfdb schema-describe` output to include a `keyspace_metadata` section naming `proc_macro_status` (enum: `enabled | degraded | disabled`) with a description of its semantics. This is NOT a SchemaVersion bump — the `keyspace_metadata` section is an informational extension to `SchemaDescribe` that consumers can read to discover available keyspace attributes." This parallels Finding 4 from RFC-042 (the `reachable_from_production_entry` descriptor gap) and is the same category of defect.

### Finding 4 — `callee_path` and `callee_qname` alignment: a pre-existing vocabulary inconsistency exposed by the RFC

The schema descriptor at `nodes.rs:279` describes `callee_path` as "Best-effort path of the callee (may be unresolved)." The HIR extractor (`call_site_emitter.rs:339`) stores `callee_qname` — the HIR-resolved fully-qualified name — in the `callee_path` property. For resolved call sites (`callee_resolved=true`), `callee_path` is NOT a "path" in the source-textual sense — it is a HIR-derived qname that may differ from what the programmer wrote.

This is a pre-existing inconsistency (not introduced by RFC-043) but RFC-043 amplifies it: with proc-macros enabled, `callee_path` for a macro-touched receiver will be the HIR's expanded form (e.g., the macro-renamed method). A query author who uses `callee_path` to match against source-text names (`WHERE cs.callee_path STARTS WITH "config."`) will silently fail to match macro-expanded names.

This is not a blocking finding for RFC-043 (it pre-dates the RFC) and is out of scope for this council. It is noted here as a related vocabulary debt that the RFC author should reference in a follow-up issue.

### Summary of change requests

1. **C1 — RFC §4 + `nodes.rs:280` descriptor:** Add invariant I7. Update `callee_resolved` descriptor to note that semantic precision co-varies with `proc_macro_status`.
2. **C2 — RFC §4:** Add invariant I8. Document the virtual-VFS filter as the phantom-call-site protection mechanism. Add a verification assertion to 043-A's fixture test.
3. **C3 — RFC §2 scope + schema-describe extension:** Add `proc_macro_status` to `cfdb schema-describe` output as a `keyspace_metadata` section.

None of these requires code changes to the core design (flag flip, fallback policy, issue decomposition). The RFC is DDD-sound on its central design decisions. All three requests are descriptor/documentation changes plus one scope extension to `schema-describe`. The verdict is REQUEST CHANGES, not REJECT. If the RFC author adds C1–C3, this lens would RATIFY on resubmit.

---

## D2. Tests prescription

### Slice 043-A

- **Unit:** `ProcMacroPolicy` debug/display round-trip; `LoadCargoConfig` wiring for both `Enabled` and `Disabled` variants (assert `with_proc_macro_server` and `proc_macro_processes` values); CLI flag mutual-exclusion parsing (both flags together → argparse error). **DDD addition:** assert that the synthetic `proc_macro_determinism` fixture emits zero `:CallSite` nodes whose `file` attribute names a path not present in the fixture workspace on disk — this verifies the virtual-VFS exclusion invariant I8 (C2 above).
- **Self dogfood (cfdb on cfdb):** `cfdb extract --workspace . --hir` on cfdb-self with proc-macros enabled; assert `:CallSite{callee_resolved: true, resolver: "hir"}` count increases vs the pre-043 baseline. List at minimum the 3 concrete qnames from RFC §3.6 that flip from `false` to `true`. Additionally assert that `proc_macro_status = "enabled"` appears in the keyspace metadata (verifies C3's schema-describe extension once landed).
- **Cross dogfood (graph-specs-rust @ pinned SHA):** `ci/cross-dogfood.sh` exits 0. All existing `.cfdb/queries/*.cypher` produce zero new rows. Macro-light workspace — no new findings expected.
- **Target dogfood (qbot-core @ pinned SHA):** `cfdb scope --context trading` `unwired` count < 1300 (vs 1534 pre-043). DDD note: the PR body MUST include a spot audit of at least 5 items that flip from `unwired` to `reached` — the operator needs confidence the newly-resolved call sites are semantically correct (not phantom macro-generated paths). One item from an `#[async_trait]` context and one from a `#[derive(Builder)]` context are the highest-value spot-check targets.

### Slice 043-B

- **Unit:** Fallback orchestration in isolation — given a stub `LoadWorkspaceFn` returning `Err` on first call and `Ok` on second: (a) warning is emitted with the three required fields (command, `HirError`, workspace path per RFC §4 I6), (b) keyspace metadata carries `proc_macro_status = "degraded"`, (c) `--strict-proc-macro` mode propagates the `Err` rather than retrying. DDD note: the warning fields named in I6 are part of the ubiquitous language of the `extract.proc_macro_status` concept — the test MUST assert all three, not just that A warning was emitted.
- **Self dogfood (cfdb on cfdb):** `cfdb extract --workspace . --hir --strict-proc-macro` exits 0 (cfdb's own crates expand cleanly under proc-macros).
- **Cross dogfood (deliberately-broken fixture `tests/fixtures/broken_proc_macro/`):** tolerant mode exits 0 with `proc_macro_status = "degraded"`; strict mode exits non-zero. This fixture is the contract test for the I6 invariant.
- **Target dogfood (qbot-core @ pinned SHA):** `cfdb extract --hir --strict-proc-macro` on qbot-core. PR body documents result: success (all macros expand cleanly) or names the offending macro/crate.

### Slice 043-C

- **Unit:** none — rationale: empirical measurement slice, no new code.
- **Self dogfood:** none — rationale: 043-A self-dogfood already covers cfdb-self.
- **Cross dogfood:** none — rationale: 043-A cross-dogfood already covers graph-specs-rust.
- **Target dogfood (THE artifact of this slice):** 8-context `unwired` delta table in the PR body: columns for pre-043 count (post-RFC-042), post-043 default, post-043 `--no-proc-macro`. DDD acceptance criterion: ≥ 50% additional reduction on the trading context (i.e., `unwired` drops from 1534 to ≤ 767). If actual reduction < 30%, the RFC premise (proc-macros are the dominant resolution bottleneck) is falsified and the RFC author must file a follow-up issue identifying the actual bottleneck before 043-D proceeds. The PR body must also call out any context where `unwired` INCREASES post-043 — a non-monotonic result would indicate phantom call sites leaking through the virtual-VFS filter.

### Slice 043-D

- **Unit:** recall baseline assertion (existing cfdb-recall suite passes unmodified).
- **Self dogfood (cfdb on cfdb):** cfdb-recall on cfdb-self with post-043 binary; new baseline numbers documented in recall README. Assert recall coverage does NOT decrease (RFC §4 I2).
- **Cross dogfood:** none — rationale: recall is a corpus tool measuring extractor coverage, not a graph-specs concern.
- **Target dogfood:** none — rationale: recall measures extractor coverage against rustdoc ground truth, not target-workspace state.

---

## D3. Dual-dogfood proof discipline

### 043-A self-dogfood

**Concrete Cypher:**
```cypher
MATCH (cs:CallSite {callee_resolved: true, resolver: "hir"})
RETURN count(cs) AS resolved_count
```
Run this against the cfdb-self keyspace extracted (a) with the pre-043 binary and (b) with the post-043 binary. The post-043 count MUST be strictly greater than the pre-043 count. The PR body must name the specific qnames that flip — RFC §3.6 identifies three candidate sites in `crates/cfdb-hir-extractor/src/call_site_emitter.rs`, `crates/cfdb-petgraph/src/eval/`, and BDD tests. A concrete lower bound on the delta cannot be prescribed without a live extract; the PR body names whatever ≥ 3 sites flip, and the reviewer spot-checks at least one.

**Rationale:** The grep-count approach used in RFC-042 self-dogfood is not applicable here because the resolution gain comes from previously-`None` resolution results becoming `Some` — there is no source-text annotation to grep. The only ground truth is the before/after resolved count delta.

**Keyspace metadata assertion (C3):** After 043-B lands `proc_macro_status` in `schema-describe`, add to the 043-A self-dogfood: `cfdb schema-describe --keyspace <cfdb-self-keyspace>` output includes `proc_macro_status = "enabled"` in the keyspace_metadata section.

### 043-A cross-dogfood

**Regression check:**
```bash
ci/cross-dogfood.sh  # against yg/graph-specs-rust at current pinned SHA b542af3
```
Expected exit code: 0. All four existing `.cfdb/queries/*.cypher` in graph-specs-rust produce zero rows. RFC §4 invariant I3 (no SchemaVersion bump) and I4 (no cross-fixture pin bump) both apply — the cross-dogfood is a pure no-regression check.

**DDD-specific note:** None of the four graph-specs-rust queries reads `callee_resolved`, `resolver`, or `proc_macro_status`. The vocabulary additions in this RFC do not affect them. The risk of a new finding from enabling proc-macros on graph-specs-rust is assessed as low (macro-light workspace per RFC §7 cross-dogfood row) but MUST be measured empirically, not assumed.

### 043-C target-dogfood

**Acceptance table shape (to be filled by implementer, verified by reviewer):**

| Context | Pre-043 unwired | Post-043 default | Post-043 `--no-proc-macro` | Delta (default vs pre) |
|---|---:|---:|---:|---:|
| trading | 1534 | ? | ? | ? |
| (other 7 contexts) | … | … | … | … |

**Acceptance criterion:** `trading` context `unwired` post-043 default ≤ 767 (≥ 50% additional reduction). If any context shows `unwired` INCREASING post-043 vs the `--no-proc-macro` run, the PR must explain why — this is the canary for phantom call sites slipping through (Finding 2).

---

## D4. Determinism risk enumeration

RFC §5.4 (Rust systems lens lead) is the primary contributor here. DDD contribution to the question: which macros in the qbot-core / qbot-infrastructure dependency closure produce outputs that include **non-deterministic identifiers**?

Known risk classes:
- `#[async_trait]` (dtolnay/async-trait): expansion is deterministic — it rewrites method signatures in a stable way, no timestamps or random IDs.
- `#[derive(Builder)]` (typed-builder, derive_builder): expansion is deterministic — generated field setters have stable names derived from field names.
- `#[tokio::test]` / `#[tokio::main]`: expansion is deterministic — wraps body in a runtime block, no env-var reads at expansion time.
- `#[given]` / `#[when]` / `#[then]` (cucumber-rs): expansion is deterministic — registers step patterns, no timestamps.
- **Potential risk — `build_info` / `vergen` macros:** `build_info::build_info!()` and `vergen::vergen!()` read `CARGO_PKG_VERSION`, `VERGEN_BUILD_TIMESTAMP`, and `VERGEN_GIT_SHA` at expansion time. If qbot-infrastructure uses either crate (likely, given it is a production deployment binary), these macros are non-deterministic across CI runs. The RFC §3.4 determinism check extension MUST include a test that these macros (if present in the fixture's dep closure) do not cause a sha256 diff between two extracts. If they do, they are deny-list candidates.
- **Risk: proc-macros calling `std::env::var("OUT_DIR")`:** `OUT_DIR` is a Cargo build-script output directory that changes per-run. Macros that embed it in generated code produce non-deterministic outputs.

**Deny-list recommendation:** The DDD lens recommends against a deny-list in 043-B as a first resort. The `ci/determinism-check.sh` extension with the macro-heavy fixture is the correct gate — if the fixture passes (sha256-stable), the deny-list is unnecessary. If it fails, the failing macro is identified by binary search over the dep closure and the deny-list (or fallback to `--no-proc-macro` on the identified crate) is introduced as a targeted fix. A pre-emptive deny-list would be premature vocabulary hardcoding.

---

## D5. Wall-clock budget verdict

The 4x cap in RFC §3.4 is acceptable as a **ceiling**, not a target. The DDD lens has no objection to the 4x number on technical grounds — it is an operational constraint, not a vocabulary or bounded-context question. The BRIEF §3.2 D5 question (whether 2x is warranted) is deferred to the `rust-systems` lens as the authoritative voice on runtime cost modeling.

One DDD-flavored observation: if the actual post-043 extract on qbot-infrastructure exceeds 4x, the operator escape hatch is `--no-proc-macro`, which restores pre-043 semantics. The vocabulary design (keyspace metadata `proc_macro_status = "disabled"`) correctly communicates the degraded recall to downstream consumers. The budget is a correctness gate for the RFC, not a vocabulary constraint. If 4x is exceeded and the RFC is narrowed (e.g., Sysroot only on first call site per file, as named in the RFC), that is a scope change, not a vocabulary change, and does not require a DDD re-verdict.

---

## D6. Failure-mode policy verdict

**Is tolerant fallback the right default?** Yes. From the DDD perspective, the ubiquitous language of cfdb operators is "I want the best-available recall." `Enabled + tolerant fallback` with `proc_macro_status = "degraded"` on failure matches that language: the operator gets the best available output and can see when the best was unavailable. The alternative default — `--strict-proc-macro` — would produce extract failures in CI environments without a proc-macro-capable sysroot, which is the wrong behavior for a tool whose primary value is recall improvement. The operator can always opt into strict mode.

**Is `proc_macro_status` keyspace metadata sufficient, or do consumers need a per-`:Item` flag?** As argued in Finding 3: keyspace metadata is sufficient for single-keyspace consumers. The per-`:Item` flag is not DDD-correct because the answer to "was this item's resolution macro-touched?" is not computable from the HIR resolution API without invasive changes. The correct consumer pattern (join on `proc_macro_status` before federating keyspaces) is a documentation obligation, not a schema change.

**What MUST be added (C3):** `proc_macro_status` must appear in `cfdb schema-describe` output so consumers can discover it. The current RFC §4 invariant I3 explicitly excludes it from `SchemaDescribe`. This exclusion should be reversed: `schema-describe` should emit a `keyspace_metadata` section that names `proc_macro_status` and its three values. This is NOT a SchemaVersion bump (it's informational metadata about the extract run), but it is a discoverability requirement. Without it, a query author has no machine-readable way to know that `proc_macro_status` exists or what it means.

---

## R2 pre-conditions (for RATIFY on resubmit)

The DDD lens will RATIFY on a round-2 submission that satisfies:

1. **C1** — RFC §4 adds invariant I7; `crates/cfdb-core/src/schema/describe/nodes.rs:280` `callee_resolved` descriptor updated to note `proc_macro_status` co-variance.
2. **C2** — RFC §4 adds invariant I8; 043-A fixture test includes the virtual-VFS exclusion assertion (zero `:CallSite` nodes with `file` naming a non-existent path).
3. **C3** — RFC §2 scope adds `proc_macro_status` to `cfdb schema-describe` output as a `keyspace_metadata` section; RFC §4 invariant I3 revised to reflect this.

All three are descriptor/documentation changes plus one scope extension to `schema-describe`. None requires rework of the core design (flag flip, fallback policy, issue decomposition, or schema vocabulary). The RFC's DDD fundamentals are sound.
