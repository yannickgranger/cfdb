# RFC-043 verdict — clean-arch

**Verdict:** REQUEST CHANGES
**Author:** clean-arch sub-agent
**Date:** 2026-05-18

---

## D1. Verdict on the RFC as written

### Finding 1 — `ProcMacroPolicy` placement: belongs in `cfdb-hir-extractor`, NOT `cfdb-core` (CONFIRM)

RFC §3.1 places `ProcMacroPolicy::{Enabled, Disabled}` in `cfdb-hir-extractor`. This is architecturally correct. The dependency rule requires that inner layers (`cfdb-core`) not depend on outer layers (`cfdb-hir-extractor`). Placing `ProcMacroPolicy` in `cfdb-core` would mean `cfdb-core` encodes a concept that is exclusively an implementation concern of a single adapter (`cfdb-hir-extractor`). No other backend (`cfdb-extractor`, the syn-based extractor) has or ever will have a proc-macro server — the enum would be an abstraction with exactly one implementer, a textbook stable-abstractions inversion.

The correct boundary is: `ProcMacroPolicy` is a parameter type of `build_hir_database` in `cfdb-hir-extractor`. The CLI constructs it. `cfdb-core` never sees it. This is already how the RFC §3.1 design reads; the verdict on this sub-question is CONFIRM as written.

Verification: `crates/cfdb-hir-extractor/src/hir_db.rs:40` currently takes no policy parameter. The new signature `pub fn build_hir_database(workspace_root: &Path, policy: ProcMacroPolicy) -> Result<(RootDatabase, Vfs), HirError>` as stated in RFC §3.1 keeps `ProcMacroPolicy` crate-local to `cfdb-hir-extractor`. Clean. No violation.

### Finding 2 — Fallback retry logic placement: CHANGE REQUIRED

RFC §3.1 places `ProcMacroPolicy` in the extractor, but RFC §3.3 implicitly places the tolerant-fallback retry loop (try Sysroot → on Err → retry None → tag `proc_macro_status`) INSIDE `build_hir_database`. RFC §5.3 ("SOLID + component principles") explicitly notes this as a question — "Is this still SRP, or has the function grown a second responsibility?" — but defers to that lens. The clean-arch lens has a direct answer.

The current `hir_db.rs:40` function has exactly one responsibility: call `load_workspace_at` with a given config and wrap errors into `HirError`. That is an adapter function. Adding the retry loop to `build_hir_database` gives it two responsibilities: (1) load the workspace, and (2) orchestrate the failure-recovery policy including the metadata-tagging side effect. Responsibility (2) is an orchestration concern — it belongs at the composition root, not inside the adapter.

The correct architecture is:

- `build_hir_database(workspace_root: &Path, policy: ProcMacroPolicy)` remains a single-shot adapter function: attempt to load with the specified policy, return `Ok(RootDatabase, Vfs)` or `Err(HirError)`. No retry logic, no metadata tagging.
- A new function — `crates/cfdb-cli/src/hir.rs::extract_and_ingest_hir` is the current CLI-layer composition site at `hir.rs:24-68` — is WHERE the retry/fallback orchestration should live.

Concretely: `cfdb-cli/src/hir.rs:33` currently calls `build_hir_database(workspace_root)`. Under RFC-043 this becomes the site that implements the `Enabled + tolerant fallback` policy: call `build_hir_database(root, ProcMacroPolicy::Enabled)`, if `Err` emit the structured warning and call `build_hir_database(root, ProcMacroPolicy::Disabled)`, then propagate the degraded status upward. The `proc_macro_status` value is determined at this orchestration site and passed up to the caller that writes `KeyspaceFile`.

This decomposition keeps `build_hir_database` a pure adapter (no orchestration logic, no fallback behavior, no metadata emission) and keeps the retry/tagging policy in the composition root that already owns the "what to do when things go wrong" concerns. `cfdb-cli/src/hir.rs` is already the correct layer: it knows the Keyspace, it knows the store, it is the wiring point for the HIR pipeline. Fallback policy belongs here.

**Proposed RFC §3.1 + §3.3 edit:** Add one sentence to §3.1: "The retry/fallback policy is NOT implemented inside `build_hir_database`. The function is single-shot: it attempts the requested policy and returns `Ok` or `Err`. The fallback orchestration (try Enabled, on Err retry Disabled, emit warning, propagate `proc_macro_status`) is the responsibility of the calling site in `cfdb-cli/src/hir.rs`."

Update §3.3 step 1 to read: "Default path: at `cfdb-cli/src/hir.rs`, call `build_hir_database(root, ProcMacroPolicy::Enabled)`. If `Ok`, set `proc_macro_status = enabled`. If `Err`, log the structured warning and call `build_hir_database(root, ProcMacroPolicy::Disabled)`, setting `proc_macro_status = degraded`. The keyspace is still produced." Replace the implicit "inside the extractor" framing with this explicit "at the CLI composition site" framing.

### Finding 3 — `extract.proc_macro_status` metadata: correctly placed as keyspace metadata, NOT in `cfdb-core` (CONFIRM)

RFC §3.3 and §4 invariant I3 state: `proc_macro_status` is a top-level keyspace metadata attribute, NOT a schema node/edge attribute, and the `SchemaVersion` does NOT bump. This is architecturally sound.

The actual `KeyspaceFile` structure lives in `cfdb-petgraph/src/persist.rs:36-40` and currently has three fields: `schema_version`, `nodes`, `edges`. The RFC proposes adding a top-level `proc_macro_status` alongside these. This is an extension to the persistence format, not to the graph schema. `cfdb-core::SchemaVersion` governs the node/edge vocabulary; it does NOT govern the persistence-file header. This is the correct distinction.

However, the RFC describes `proc_macro_status` as "top-level keyspace metadata attribute" but `cfdb-core` is cited as its home via the phrase "next to `cfdb_version`, `schema_version`, `extracted_at` in the keyspace JSON header" (RFC §3.3). In the actual codebase, `KeyspaceFile` in `cfdb-petgraph/src/persist.rs` holds `schema_version` but NOT `cfdb_version` or `extracted_at` — those fields do not currently exist. The RFC §3.3 description implies a richer metadata header than what is actually in the code.

**Proposed RFC §3.3 edit (MINOR CLARIFICATION REQUIRED):** Replace "next to `cfdb_version`, `schema_version`, `extracted_at` in the keyspace JSON header" with "as an additional top-level field in `KeyspaceFile` (alongside the existing `schema_version` field in `cfdb-petgraph/src/persist.rs:36-40`)." This removes the reference to `cfdb_version` and `extracted_at` which do not exist in the current `KeyspaceFile`, preventing the implementer from inventing those fields as collateral scope.

RFC §4 invariant I3 ("no `SchemaVersion` bump") and I4 ("graph-specs cross-fixture pin not bumped") are correct and should be preserved as written. The `cfdb schema-describe` extension (one-line addition in `cfdb-core::schema::describe`) is acceptable as long as it reads the `proc_macro_status` value from wherever it is stored (the `KeyspaceFile` header in `cfdb-petgraph`) and does NOT store the value in `cfdb-core` itself. Verify: the `describe` command must query the store/persistence layer for this value, not derive it from any `cfdb-core` type.

### Finding 4 — CLI flag composition root contract: CLEAN with one caveat

RFC §3.2 says the CLI flag wiring goes through `cfdb-cli::compose::load_hir_extractor` (or equivalent at the extract sub-command site). The actual code path is `cfdb-cli/src/hir.rs::extract_and_ingest_hir` (called from `cfdb-cli/src/commands/extract.rs`). The `compose.rs` module currently has NO hir-extractor factory (`compose.rs:1-308` inspected — the module handles `PetgraphStore` construction only; there is no `load_hir_extractor` function). RFC §3.2 should name the actual wiring site (`cfdb-cli/src/hir.rs::extract_and_ingest_hir`) rather than a hypothetical `compose::load_hir_extractor`, otherwise the implementer may create a dead function in `compose.rs` instead of threading the policy through the correct call site.

**Proposed RFC §3.2 edit:** Replace "cfdb-cli::compose::load_hir_extractor (or equivalent at the extract sub-command site)" with "cfdb-cli::hir::extract_and_ingest_hir (hir.rs:24), which is the existing CLI composition site for the HIR pipeline. The `--no-proc-macro` and `--strict-proc-macro` flags are parsed in `ExtractArgs` and forwarded as a `ProcMacroPolicy` value to `extract_and_ingest_hir`. No new factory in `compose.rs` is needed or wanted."

The `ExtractArgs` struct in `crates/cfdb-cli/src/main_command/args/extract_args.rs:15-53` currently does NOT have `--no-proc-macro` or `--strict-proc-macro` fields. The 043-A implementer must add them there. The mutually-exclusive argparse constraint (both flags together = error) must be enforced at the `ExtractArgs` level, not inside `hir.rs`. This is correctly scoped to 043-A.

### Finding 5 — Existing `build_hir_database` call sites: BREAKAGE SCOPE (CHANGE REQUIRED)

The signature change to `build_hir_database` adds a `policy: ProcMacroPolicy` parameter. The current public API has the zero-parameter form at `cfdb-hir-extractor/src/hir_db.rs:40`. The following call sites will break on signature change and MUST be updated as part of 043-A:

- `cfdb-cli/src/hir.rs:33` — the primary CLI path; receives the policy from `ExtractArgs`.
- `cfdb-hir-extractor/tests/http_route.rs:66` — test call site; should pass `ProcMacroPolicy::Disabled` (tests don't need macro expansion; `Enabled` in tests would cause unnecessary wallclock cost and sysroot dependency).
- `cfdb-hir-extractor/tests/callsite_line.rs:110`
- `cfdb-hir-extractor/tests/entry_point.rs:114`, `:201`, `:261`, `:305`, `:364`, `:423`, `:502`, `:563`, `:620`, `:692`
- `cfdb-hir-extractor/tests/v02_1_coverage.rs:98`
- `cfdb-hir-extractor/tests/resolved_dispatch.rs:91`, `:246`, `:371`
- `cfdb-hir-extractor/tests/test_bench_entry.rs:49`
- `cfdb-hir-petgraph-adapter/tests/cfdb_self_dogfood.rs:88`

That is approximately 17 call sites across tests and one production path. ALL existing test call sites should default to `ProcMacroPolicy::Disabled` — existing tests do not test proc-macro expansion, they test HIR extraction on fixtures that don't require it. Passing `Enabled` to all existing tests would couple the entire test suite to a sysroot being present, extending CI prerequisites without benefit.

**Proposed RFC §2 (Scope) addition:** Under "Ships:" add: "All existing `build_hir_database` call sites in tests/ pass `ProcMacroPolicy::Disabled`. No test suite dependency on a sysroot proc-macro binary is introduced by 043-A beyond the new `tests/fixtures/proc_macro_determinism/` fixture (which tests proc-macro behavior explicitly)."

The RFC §7.1 (043-A scope) currently does NOT enumerate the ~17 existing call sites. This gap risks the implementer overlooking them and shipping a compile-broken PR. The scope description should acknowledge the breakage and prescribe the migration policy.

### Summary: REQUEST CHANGES

Four changes before ratification:

1. **RFC §3.1 + §3.3:** Fallback retry logic moves from `build_hir_database` to `cfdb-cli/src/hir.rs::extract_and_ingest_hir`. `build_hir_database` is single-shot. (Finding 2 — architectural boundary.)
2. **RFC §3.3:** Replace "next to `cfdb_version`, `schema_version`, `extracted_at`" with "as an additional top-level field in `cfdb-petgraph/src/persist.rs::KeyspaceFile`". (Finding 3 — accuracy; prevents scope creep.)
3. **RFC §3.2:** Replace hypothetical `compose::load_hir_extractor` with actual `cfdb-cli/src/hir.rs::extract_and_ingest_hir`. (Finding 4 — naming the correct composition site.)
4. **RFC §7.1 (043-A scope):** Add explicit enumeration of ~17 existing call-site migrations; prescribe `Disabled` for all existing tests. (Finding 5 — implementation completeness.)

Findings 1 (policy enum in extractor crate) and the structural correctness of Findings 3's no-schema-change intent are already sound as written. The core design — a `ProcMacroPolicy` enum threaded through the extractor, CLI flags in `ExtractArgs`, tolerant fallback, `proc_macro_status` metadata — is architecturally valid and preserves the dependency rule. The requested changes are editorial/boundary-precision corrections, not redesign.

---

## D2. Tests prescription

### Slice 043-A

- **Unit:** `ProcMacroPolicy` display/debug round-trip. `LoadCargoConfig` wiring: assert `ProcMacroPolicy::Enabled` → `with_proc_macro_server = Sysroot, proc_macro_processes = 1`; `ProcMacroPolicy::Disabled` → `None, 0`. Mutually-exclusive CLI flag group: `ExtractArgs::validate()` or clap `conflicts_with` returns error when both `--no-proc-macro` and `--strict-proc-macro` are passed. These are pure config/CLI tests; no HIR database needed.
- **Self dogfood:** (defer to ddd-specialist for the Cypher shape; clean-arch prescribes the assertion structure) After re-extract of cfdb-self with `policy = Enabled`, run `MATCH (cs:CallSite{callee_resolved: true}) WHERE cs.callee_qname STARTS WITH 'ra_ap_' RETURN count(cs)` and confirm the count is strictly GREATER than with `policy = Disabled`. The comparison is load-bearing: it proves macro-resolved call sites are being added, not just that the extractor ran.
- **Cross dogfood:** `ci/cross-dogfood.sh` against graph-specs-rust at current pinned SHA must exit 0. The macro-light companion gains no new findings because RFC §4 I3 (no SchemaVersion bump) and I4 (no cross-fixture pin bump) hold. The test is a non-regression assertion.
- **Target dogfood:** (defer to ddd-specialist for the exact Cypher and threshold) cfdb scope `--context trading` `unwired` count on qbot-core @ pinned SHA after re-extract with proc-macros enabled. Count must be < 1300 (RFC §7.1 threshold). Report actual number in PR body.

### Slice 043-B

- **Unit:** Fallback orchestration in `cfdb-cli/src/hir.rs` (per Finding 2 above: NOT inside `build_hir_database`). Given a test double for `build_hir_database` that returns `Err` on the first call and `Ok` on the second, the fallback path in `extract_and_ingest_hir` must: (a) emit a structured warning to stderr naming the HirError, the workspace path, and the originating command; (b) retry with `Disabled`; (c) propagate `proc_macro_status = "degraded"` to the `KeyspaceFile`. The test double avoids invoking real `load_workspace_at`.
- **Self dogfood:** `cfdb extract --workspace . --hir --strict-proc-macro` on cfdb-self must exit 0 (cfdb's own crates expand cleanly, no panicking macros). This validates I6: the strict path does not degrade silently.
- **Cross dogfood:** `cfdb extract --hir` on `tests/fixtures/broken_proc_macro/` (a new fixture with a panic in macro body): in tolerant mode (default), exit 0 + keyspace tagged `degraded`. In `--strict-proc-macro` mode, exit non-zero. Both paths asserted in the same test.
- **Target dogfood:** `cfdb extract --workspace <qbot-core-path> --hir --strict-proc-macro` at pinned SHA: report whether it succeeds or names the offending macro in the PR body. This is a reviewer sanity-check, not a merge-blocking assertion.

### Slice 043-C

- **Unit:** none — rationale: empirical measurement slice, no new logic (per RFC §7.3).
- **Self dogfood:** none — rationale: 043-A self-dogfood already exercises cfdb-self call-site recall.
- **Cross dogfood:** none — rationale: 043-A cross-dogfood already covers the graph-specs-rust regression check.
- **Target dogfood:** THE artifact of this slice. 8-context `unwired` delta table in PR body for qbot-core @ pinned SHA: columns are `context`, `pre-043 unwired`, `post-043 (proc-macros on)`, `post-043 (--no-proc-macro)`. Clean-arch lens endorses the council-prescribed acceptance criterion from RFC §7.3: ≥ 50% additional reduction required; < 30% triggers premise-rejection and RFC pivot to 043-D/043-E. The clean-arch lens adds: if any context shows `unwired` INCREASING post-043, this is a precision regression and must be investigated before ratification — new macro-resolved call sites should not produce phantom `CALLS` edges that inflate `unwired` through an unexpected path.

### Slice 043-D

- **Unit:** Recall baseline assertion — existing `cfdb-recall` test asserts cfdb-extractor sees ≥ N% of rustdoc-visible items. The updated baseline number (after proc-macro expansion) must satisfy N_new ≥ N_old. Failure means proc-macro expansion inadvertently dropped previously-seen items.
- **Self dogfood:** `cfdb-recall` on cfdb-self with post-043 binary.
- **Cross dogfood:** none — rationale: recall is a corpus tool, not a graph-specs concern (per RFC §7.4). However, if the recall tooling itself uses `build_hir_database` internally (to be verified by the rust-systems lens), it MUST pass `ProcMacroPolicy::Enabled` for the post-043 baseline run.
- **Target dogfood:** none — rationale: recall measures extractor coverage, not target workspace state (per RFC §7.4).

---

## D3. Dual-dogfood proof discipline

### 043-A self-dogfood

**Concrete Cypher (two queries, both required):**

```cypher
// Query 1 — count of resolved call sites before and after
MATCH (cs:CallSite)
WHERE cs.resolver = 'hir' AND cs.callee_resolved = true
RETURN count(cs) AS resolved_count
```

Run this against both: (a) keyspace extracted with `--no-proc-macro`, (b) keyspace extracted without that flag (proc-macros on). Assert `(b).resolved_count > (a).resolved_count`. The delta is the headline recall improvement for cfdb-self.

```cypher
// Query 2 — name at least 3 specific flip sites (per RFC §3.6)
MATCH (cs:CallSite)
WHERE cs.resolver = 'hir'
  AND cs.callee_resolved = true
  AND cs.callee_qname IS NOT NULL
  AND cs.file STARTS WITH 'crates/cfdb-'
RETURN cs.file, cs.line, cs.callee_qname
ORDER BY cs.file, cs.line
LIMIT 20
```

The PR body must list at least 3 rows from this result that were `callee_resolved = false` in the pre-043 keyspace. These are the concrete flip-site witnesses per RFC §3.6.

**Expected lower bound:** RFC §3.6 names three candidate call-site files (`call_site_emitter.rs`, `cfdb-petgraph/src/eval/`, `crates/cfdb-*/tests/`). The 043-A PR body must confirm at least 3 concrete `(file, line, callee_qname)` triples. A count-only assertion (e.g., "resolved_count went from 450 to 467") without named triples is insufficient — it does not prove the right call sites flipped.

### 043-A cross-dogfood

`ci/cross-dogfood.sh` against `yg/graph-specs-rust` at current pinned SHA (`b542af3`). Expected exit code: 0. The companion is macro-light; RFC §4 I3 ensures no schema change; RFC §4 I4 ensures no pin bump. The cross-dogfood test is a pure no-regression assertion. If it exits non-zero, the cause is NOT a schema incompatibility (I3 holds) — the cause is either a binary incompatibility at the CLI level or a false-positive in an existing ban rule caused by the new proc-macro-resolved call sites. Investigate the specific rule that fires before declaring a cross-dogfood failure.

### 043-C target-dogfood

**Acceptance shape:** Three-column table in the PR body:

| context | pre-043 unwired | post-043 (proc-macros on) | post-043 (--no-proc-macro) |
|---|---|---|---|
| trading | 1534 | TBD | TBD |
| … (all 8 contexts) | … | … | … |

**Acceptance criterion** (council-prescribed, clean-arch endorsement):
- Primary: `trading` context `unwired` count with proc-macros on < 1300 (RFC §7.1 threshold).
- Secondary: ≥ 50% additional reduction across the contexts named in RFC §7.3 (vs the 12.5% achieved by RFC-042 alone). Numerically: the post-043 `unwired` count for `trading` must be ≤ 767 (= 1534 × 0.50) for a ≥ 50% additional reduction. This is strict; the RFC is permitted to accept 30-49% if council agrees to revise the lower bound in SYNTHESIS-R1.
- Regression guard: no context increases post-043. Any increase is a precision regression requiring root-cause analysis before slice 043-C can be marked complete.

---

## D4. Determinism risk enumeration

This is the `rust-systems` lens's lead deliverable. The clean-arch lens contributes the policy framing only.

The clean-arch position: the **tolerant fallback (§3.3) is sufficient as the primary mechanism**; a compile-time deny-list for non-deterministic macros is NOT warranted at this time. The rationale:

1. The determinism invariant (I1) is tested post-hoc by `ci/determinism-check.sh` on the new `tests/fixtures/proc_macro_determinism/` fixture. This catches any non-determinism in the fixture set, regardless of cause.
2. A deny-list in `cfdb-hir-extractor` would require `cfdb-hir-extractor` to know the names of specific third-party crates (e.g., `shadow-rs` which calls `chrono` at expansion). This is an outer-layer concern (ecosystem knowledge) leaking into an inner-layer component. The deny-list would need continuous maintenance as the ecosystem evolves.
3. The correct boundary: if `ci/determinism-check.sh` fires in production against a user's workspace (not our fixture), the user's workspace has a non-deterministic macro. The `proc_macro_status: degraded` tag from the tolerant fallback is not the right signal for non-determinism (determinism failures are silent, not error-producing). A separate "determinism failed" exit code or warning might be warranted — but that is a follow-up, not a blocker for RFC-043.

The `rust-systems` lens should enumerate specific macro crates at risk (e.g., `shadow-rs`, `build-info`, anything using `std::env::var("BUILD_TIMESTAMP")`). The clean-arch lens defers to that enumeration for the specific names but endorses: no deny-list in the extractor crate itself.

---

## D5. Wall-clock budget verdict

**The 4x cap is acceptable as stated in RFC §3.4.** The clean-arch lens does not impose a stricter cap. Rationale: wall-clock is an operational concern, not an architectural concern. Architecturally, the fallback policy (§3.3) and the `--no-proc-macro` escape hatch are the correct mechanisms — they give operators control without the architecture being responsible for specific timing guarantees.

The RFC §3.4 table prescribes 4x cap verification via 043-A's perf gate, with actual numbers in the PR body. This is the right mechanism. The council should reject the RFC only if 043-A's prototype exceeds 4x on cfdb-self — not as an architectural objection but as an empirical one.

One clarification the RFC should add: if the 043-A extract of cfdb-self exceeds 4x, the fallback policy alone (§3.3) does NOT protect operators — it only triggers on `Err`, not on wall-clock overrun. The RFC should acknowledge that wall-clock budget enforcement in the tolerant-fallback mode is the operator's responsibility (via `--no-proc-macro`) and not enforced by the code. This is not a change request; it is a clarification for the implementer.

---

## D6. Failure-mode policy verdict

### Is tolerant fallback the right default?

**Yes.** The tolerant fallback (`Enabled + on Err retry Disabled + tag degraded`) is the correct default. The architectural argument: `cfdb extract` is a data-pipeline command, not a transaction. Its contract is "produce a keyspace or fail loudly." Degrading silently (no tag) would violate I6. Failing hard by default would make the first-run experience on macro-heavy workspaces (qbot-core, qbot-infrastructure) break 100% of operators who haven't diagnosed their sysroot configuration. The `proc_macro_status = degraded` tag IS the loud failure — it is not silent.

The `--strict-proc-macro` mode for CI gates (RFC §3.5) is the correct mechanism for hard-fail. CI gates already know they need a working sysroot; they can opt in.

### Is `proc_macro_status` keyspace-level metadata sufficient, or is a per-Item flag needed?

**Keyspace-level is sufficient for RFC-043.** The architectural argument: a per-`:Item` `proc_macro_touched` attribute would require (a) the extractor to know which items are macro-touched before expansion, (b) a schema vocabulary change (new attribute on `:Item`), and (c) a `SchemaVersion` bump — all of which RFC §4 invariants I2 and I3 explicitly prohibit. The keyspace-level `proc_macro_status` flag gives consumers everything they need: if `degraded`, treat all `CALLS` edges with `callee_resolved = false` as suspect. The pre-RFC-043 behavior is fully recoverable via `--no-proc-macro`.

A per-`:Item` flag would be valuable in a future RFC if consumers need finer-grained signal (e.g., "this specific call site failed resolution because its receiver was macro-touched"). That is a separate RFC with a `SchemaVersion` bump. Not in scope for RFC-043.

**One additional concern:** RFC §3.3 says consumers of the keyspace (`/operate-module`, `/sweep-epic`) should check `proc_macro_status` and interpret `unwired` output with pre-RFC-043 confidence if `degraded`. This is correct consumer guidance. The clean-arch lens notes it should be added to the RFC §3.3 text as a formal behavioral contract, not left as a parenthetical remark. This is the only additional editorial change beyond the four listed in the Summary above: "if `proc_macro_status` is `disabled` or `degraded`, the `unwired` classifier output is interpreted with pre-RFC-043 confidence; this is not enforced by the code but is the prescribed consumer protocol." This sentence should appear explicitly in §3.3 as a **consumer contract**, not just as an aside.
