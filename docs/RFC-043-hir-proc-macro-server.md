# RFC-043 — enable the proc-macro server in `cfdb-hir-extractor` for receiver-type-resolution recall

Status: **RATIFIED** 2026-05-18.
Author: a0 (session 2026-05-18, worktree `rfc/043-hir-proc-macro-server`).
Originating issue: [`yg/cfdb#398`](https://agency.lab:3000/yg/cfdb/issues/398).
Relates: cfdb-029-code-facts-database (v0.2 :EntryPoint vocabulary), cfdb-042-test-bench-entry-points (test/bench :EntryPoint kinds — established the receiver-type-resolution gap as the next bottleneck).

## 1. Problem

`crates/cfdb-hir-extractor/src/hir_db.rs:40-48` disables proc-macro expansion in the HIR loader:

```rust
let load_config = LoadCargoConfig {
    load_out_dirs_from_check: false,
    with_proc_macro_server: ProcMacroServerChoice::None,
    prefill_caches: false,
    num_worker_threads: 0,
    proc_macro_processes: 0,
};
```

With proc-macros off, `ra_ap_hir::Semantics::resolve_method_call` returns `None` on any receiver whose type-inference path crosses a proc-macro-rewritten signature. The `:CallSite` is still emitted, but with `callee_resolved=false` — the receiver's static type collapses to `{unknown}` inside the macro-touched scope.

### 1.1 Empirical evidence

cfdb-042-test-bench-entry-points shipped 042-A (`:EntryPoint{kind=test|bench}` emission) and 042-B (`--production-only` flag + dual-BFS reachability). Issue #378's 042-C close-out (comment 2026-05-18) measures the realized reduction in `unwired` false positives:

| Workspace · context | Default unwired | Production-only unwired | Reduction |
|---|---:|---:|---:|
| qbot-core · trading | 1534 | 1754 | 12.5% |
| qbot-infrastructure · infrastructure | 1086 | 1246 (est) | 12.8% |

Two contexts agree on a ~13% ceiling. The remaining 87% is the receiver-type-resolution gap. Field report from issue #398's body:

> The remaining 87% are largely receiver-type-resolution failures: `config.validate()` where `config` is a local variable does not produce a CALLS edge because the HIR extractor cannot infer the type of `config`.

### 1.2 Common idioms affected

- `#[async_trait]` — rewrites trait/impl signatures to `Pin<Box<dyn Future>>` returns. `self.method().await` collapses to `{unknown}`.
- `#[derive(Builder)]` (derive-builder, typed-builder) — `Foo::builder().with_x(1).build()` chains depend on macro-generated `FooBuilder`.
- `#[tokio::test]` / `#[tokio::main]` — wraps body in a runtime; parameter types may not survive.
- Cucumber `#[given]` / `#[when]` / `#[then]` — rewrites the step-function parameter list; `World`-receiver inference fails downstream.

### 1.3 Why this is RFC-flavored

Enabling proc-macros changes:

1. **Extract-time dependency.** Needs a usable `proc-macro-server` binary in the sysroot.
2. **Determinism.** Macros can depend on `chrono::Utc::now()`, env vars, file mtimes. G1 byte-stability MUST be re-verified.
3. **Wall-clock cost.** Macro expansion is materially more expensive than the syn-only path.
4. **Precision/recall tradeoff.** Today `callee_resolved=true` means "syn or HIR resolved with high precision." Post-043 the bar shifts.

The original disable decision (`50acca6 feat(cfdb-hir-extractor): build_hir_database`) carries no rationale. This RFC closes the gap.

## 2. Scope

**Ships:**

- `crates/cfdb-hir-extractor/src/hir_db.rs::build_hir_database` switches `ProcMacroServerChoice::None` → `ProcMacroServerChoice::Sysroot` and `proc_macro_processes: 0 → 1` by default. The function gains a single `bool` parameter — `proc_macros: bool` — that the caller threads through. Return type grows to `(RootDatabase, Vfs, ProcMacroClient)` to keep the subprocess handle alive across the VFS walk (§3.1 / §4 I7).
- Availability probe: when `proc_macros: true` but the sysroot lacks `rust-analyzer-proc-macro-srv` (typical on stock CI rustc images), `build_hir_database` falls back to `ProcMacroServerChoice::None` with a stderr warning. No `Err` is returned — degradation is silent at the API but loud at the user (§3.3 case 1).
- `cfdb extract` gains one CLI flag: `--no-proc-macro` (default `false`). When set, the caller passes `proc_macros: false` to `build_hir_database`, restoring the pre-043 behaviour without invoking the availability probe.
- `ci/determinism-check.sh` extended to ALSO run a `--hir` extract pair against cfdb-self (the existing syn-only check stays). G1 byte-stability is asserted on the new path.
- `cfdb-recall` baseline numbers refreshed in the same PR — the recall metric improves; we re-baseline with the post-043 binary.
- Empirical close-out lands in the SAME PR's body: qbot-core `--context trading` unwired count before/after, with the explicit target of "materially less than the pre-043 1534 ceiling."
- Schema descriptor caveat: `:CallSite.callee_resolved` descriptor in `crates/cfdb-core/src/schema/describe/nodes.rs` gains one sentence noting the epistemic precision shift (see §4 I6). Descriptor-text-only — no schema vocabulary change.

**Does NOT ship:**

- A `ProcMacroPolicy` wrapper enum, `extract.proc_macro_status` keyspace metadata, `cfdb schema-describe` extension, `--strict-proc-macro` flag, or retry-after-`Err` tolerant fallback. The availability probe + stderr warning (§3.3 case 1) and the existing `HirError::LoadWorkspace` propagation (§3.3 case 2) are the entire failure-mode policy. If field experience demands richer signalling, file a follow-up RFC naming the concrete consumer.
- Multi-process expansion (`proc_macro_processes > 1`). Single-process is the correctness gate; tuning is deferred.
- New `:Label`, `:EdgeLabel`, or `:CallSite` attribute. The existing `callee_resolved`/`callee_qname` attrs gain coverage; no schema vocabulary change.
- Backfill of pre-043 keyspaces. Operators re-extract.
- A synthetic macro-heavy fixture under `tests/fixtures/`. cfdb-self is macro-heavy enough (`#[derive(Debug,Clone,...)]` throughout, `#[tokio::test]` in async crates) to serve as the determinism corpus.

## 3. Design

### 3.1 The change

```rust
// crates/cfdb-hir-extractor/src/hir_db.rs — after RFC-043
pub fn build_hir_database(
    workspace_root: &Path,
    proc_macros: bool,
) -> Result<(RootDatabase, Vfs, Option<ProcMacroClient>), HirError> {
    // Single shared condition — both `with_proc_macro_server` and
    // `proc_macro_processes` MUST agree (see solid-architect R2 CR1).
    // When the caller asks for proc-macros but the sysroot binary is
    // missing, both fields fall back together; otherwise the probe-
    // fires-false path would leave them inconsistent.
    let pm_enabled = proc_macros && proc_macro_server_available();
    let cargo_config = CargoConfig::default();
    let load_config = LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: if pm_enabled {
            ProcMacroServerChoice::Sysroot
        } else {
            ProcMacroServerChoice::None
        },
        prefill_caches: false,
        num_worker_threads: 0,
        proc_macro_processes: if pm_enabled { 1 } else { 0 },
    };
    let (db, vfs, proc_macro_client) = load_workspace_at(...).map_err(...)?;
    Ok((db, vfs, proc_macro_client))
}
```

`load_workspace_at`'s third return element is `Option<ProcMacroClient>` (per `ra_ap_load_cargo-0.0.328/src/lib.rs:204`), not bare `ProcMacroClient` — when `pm_enabled=false` the option is `None`. Callers handle both arms but the lifetime invariant (§4 I7) applies only to the `Some` arm.

Two upstream-shape concerns this signature handles:

1. **Lifetime**: the third element of `load_workspace_at`'s return tuple is a `ProcMacroClient` handle that owns the proc-macro subprocess. The salsa `RootDatabase` keeps live references to its expanders. The handle MUST outlive the database — if dropped, the subprocess terminates and later lazy expansions during VFS walks fail. Pre-RFC the function discarded the handle (`let (_, _, _proc_macro_client) = ...`) because with `ProcMacroServerChoice::None` salsa never needed it. Post-RFC the caller MUST hold it alongside `db` and `vfs`. The return tuple grows to three elements; callers in `cfdb-cli::compose` thread the handle through to extraction's terminal scope.

2. **Availability**: `ProcMacroServerChoice::Sysroot` requires `rust-analyzer-proc-macro-srv` (or equivalent) to be present in the active sysroot. This binary is NOT shipped with stable rustc — it ships with the `rust-analyzer` rustup component, which may not be installed on stock CI runners. A `proc_macro_server_available()` probe (filesystem check on the sysroot path, or `which`-style PATH lookup) gates the upstream choice: present → `Sysroot`, missing → `None` + stderr warning. This is an availability fallback, NOT a retry-after-error; the probe runs before `load_workspace_at` to avoid the cost of a failed load.

No new wrapper enums. `proc_macros: bool` is the upstream-faithful shape; `ProcMacroServerChoice` from `ra_ap_load_cargo` is the only enum in play.

### 3.2 Composition root

`cfdb-cli`'s `extract` sub-command reads `--no-proc-macro` (clap-derived flag, default `false`) and passes `!no_proc_macro` to `build_hir_database`. The returned `ProcMacroClient` is owned by the extract sub-command's stack frame and dropped when extraction completes — bounding the subprocess lifetime to one CLI invocation. No new module, no orchestrator hoist, no fallback orchestration at the CLI layer (the availability fallback lives inside `build_hir_database` because it's a startup-time check, not a recovery-time loop).

### 3.3 Failure modes

Three failure shapes are distinguished:

1. **Sysroot binary missing** (`proc_macro_server_available()` returns false): silently fall back to `ProcMacroServerChoice::None` + stderr warning naming the missing binary path and noting "proc-macro recall unavailable on this run; receiver-type-resolution recall is the pre-cfdb-043-hir-proc-macro-server baseline." This is the CI-on-stock-rustc path. No `Err` is returned; the extract proceeds in degraded mode.
2. **`load_workspace_at` returns `Err` even after the availability probe passed** (e.g., a proc-macro panics on expansion during `load_workspace_at`'s eager phase): hard fail. The error propagates through the existing `HirError::LoadWorkspace` mapping — no new error variant. The operator sees the underlying message on stderr and re-runs with `--no-proc-macro` if needed.
3. **Lazy expansion failure during VFS walk** (e.g., the subprocess crashes after `load_workspace_at` returns): individual call sites where expansion failed are emitted with `callee_resolved=false` — same as the syn-only path. The walk does NOT abort; the operator sees the new `callee_resolved=false` ratio at end-of-extract and decides if a re-run is warranted.

We do NOT implement tolerant fallback for case 2 (retry-after-Err). Case 1 (availability fallback) IS load-bearing per RS-2 (rust-systems v1 verdict); without it, CI on stock rustc images breaks on every PR.

The two retained policies — availability fallback (case 1) and continue-on-lazy-failure (case 3) — are the minimum signal-preserving behaviour. A status metadata field on the keyspace was considered and rejected as YAGNI (no concrete consumer; the stderr warning in case 1 is the operator-visible signal).

### 3.4 Wall-clock expectation

Pre-RFC numbers (syn-only `--hir` extract):

- cfdb-self: ~5 s
- qbot-core: ~3 min

Post-RFC expected ceiling (proc-macros on): 2-3× the pre-RFC times. Tiered evaluation (per rust-systems R2 D5):

- **≤ 2× pre-RFC** — pass-with-note; no operational concern.
- **≤ 4× pre-RFC** — pass-with-warning; PR body must name the dominant macro/crate cluster responsible for the cost.
- **> 4× pre-RFC** — reject; pending a single-process tuning or partial-expansion variant.

CI gate: §4 invariant I3 caps cfdb-self at 4× (so `ci/determinism-check.sh`'s `--hir` mode budget is ≤ 20 s).

### 3.5 Determinism extension

`ci/determinism-check.sh` extended with a second extract pair:

```bash
"$CFDB_BIN" extract --workspace "$CFDB_SELF" --db "$DB_A" --keyspace ks --hir >/dev/null
"$CFDB_BIN" extract --workspace "$CFDB_SELF" --db "$DB_B" --keyspace ks --hir >/dev/null
# assert sha256 equality of cfdb dump output
```

cfdb-self is the corpus (it carries `#[derive]` and `#[tokio::test]` shapes; no synthetic fixture needed). The existing syn-only check on `spikes/qa5-utc-now` continues to run; the new `--hir` check runs alongside it.

If determinism fails (G1 violation), the CI gate exits 1 and names the diverging sha256s — same shape as the existing check.

### 3.6 Known determinism risk: `proc_macro_cwd`

`ra_ap_load_cargo` injects the workspace's absolute path as `proc_macro_cwd` into every macro expansion call. Macros that capture this path verbatim into their output (e.g., via `concat!(env!("CARGO_MANIFEST_DIR"), ...)` or `file!()`) will produce keyspace JSON that differs across two runs IF the workspace is checked out at different absolute paths (e.g., the CI runner's tempdir vs the local clone). The §3.5 determinism check captures this because both extracts run in the SAME absolute path, but a regression where a macro's output leaks the absolute path into a node attribute would survive that check.

The extractor already canonicalizes file paths to workspace-relative via `vfs_path_to_pathbuf` (see `crates/cfdb-hir-extractor/src/call_site_emitter.rs:113-114`). The risk surface is macro-introduced attributes that bypass this canonicalization — e.g., a future RFC that adds a new `:Item` attribute populated from macro-expanded source might inadvertently include the absolute path.

**Mitigation strategy:** no new policy in this RFC. The §4 I1 invariant covers the same-workspace case. If a cross-workspace determinism check is needed in a future RFC, it would land alongside whatever attribute introduced the divergence. Documented here per rust-systems v1 verdict RS-3 for future-RFC awareness.

## 4. Invariants

| ID | Invariant | Verification |
|---|---|---|
| I1 | **G1 byte-stability** — `cfdb extract --hir` twice on cfdb-self produces sha256-identical keyspace JSON. | `ci/determinism-check.sh` extension (§3.5). Hard fail on diff. |
| I2 | **Recall non-regression** — every `:CallSite` resolved pre-cfdb-043-hir-proc-macro-server remains resolved post-cfdb-043-hir-proc-macro-server. | `cfdb-recall` baseline refresh in the same PR. New baseline numbers MUST NOT regress vs old (only added resolutions). |
| I3 | **Wall-clock budget** — `cfdb extract --hir` against cfdb-self post-RFC takes ≤ 4× pre-RFC (reject) with a 2× warning threshold (pass-with-note, document offending crate cluster in PR body). | Measured in PR body; CI gate (a `time` wrapper in determinism-check.sh) fails over the 4× budget; the 2× threshold is an operator-visible note. |
| I4 | **Schema unchanged** — no SchemaVersion bump, no new node/edge/attribute. | `cfdb schema-describe` diff between pre and post keyspaces is empty. |
| I5 | **Cross-fixture pin not bumped** — I4 implies no `yg/graph-specs-rust` lockstep (cfdb-033-cross-dogfood#4 I5 only triggers on SchemaVersion bumps). | `ci/cross-dogfood.sh` exits 0 with the post-RFC binary against the current pinned graph-specs SHA. |
| I6 | **`:CallSite.callee_resolved` descriptor updated** — the schema descriptor at `crates/cfdb-core/src/schema/describe/nodes.rs` (the `:CallSite.callee_resolved` attribute paragraph) gains a sentence noting that, post-cfdb-043-hir-proc-macro-server, the predicate's epistemic precision improved. The descriptor must ALSO note that the silent probe fallback (§3.3 case 1) produces a keyspace indistinguishable from `--no-proc-macro` — operators reading `cfdb schema-describe` should understand why two keyspaces with identical `callee_resolved` distributions can have different recall. Consumers wishing to disambiguate pre/post-cfdb-043-hir-proc-macro-server keyspaces must re-extract — there is no per-keyspace status flag, by design (§3.3 / §6 YAGNI cuts). | Descriptor diff in slice 043-A; verified by `cfdb schema-describe` output containing both sentences. |
| I7 | **`ProcMacroClient` lifetime bounded by extraction scope** — `build_hir_database` returns `(RootDatabase, Vfs, ProcMacroClient)`; the third element MUST outlive the database's last use (the VFS walk). Dropping the client while salsa holds live expanders kills the subprocess and breaks lazy macro expansion. The CLI sub-command's stack frame owns all three. | Code review on slice 043-A; the function signature and call sites are inspected. |

## 5. Architect lenses (council fills in)

Per cfdb CLAUDE.md §2.3, each lens renders a verdict (RATIFY / REQUEST CHANGES / REJECT) with evidence, plus prescribes the `Tests:` 4-row block for the slice (`Unit`, `Self dogfood`, `Cross dogfood`, `Target dogfood`).

### 5.1 Clean architecture

Question: Does adding a single `bool` parameter to `build_hir_database` + one CLI flag in `cfdb-cli` change the composition-root contract? The dependency direction is unchanged (`cfdb-cli` → `cfdb-hir-extractor` → `ra-ap-load-cargo`). No new modules, no hoisted orchestrator.

### 5.2 Domain-driven design

Question: Does macro-resolved `:CallSite{callee_resolved=true}` introduce a vocabulary homonym? Pre-043 the predicate meant "syn-or-HIR-can-name-the-callee-with-high-precision." Post-043 the predicate domain expands to include macro-touched receivers, but the SEMANTICS — "the extractor can statically name the callee" — is unchanged. The bar shifts; the meaning does not.

Edge case: when `#[async_trait]` desugars `async fn foo()` to `fn foo() -> Pin<Box<...>>`, the `:CallSite.callee_path` for a caller's `.foo().await` expression — does it textually match `foo` (the syn-visible name)? Or `poll` (the desugared method)? The expectation in this RFC is "matches what `syn` would show" — i.e., the textual `foo` path.

### 5.3 SOLID + component principles

Question: `build_hir_database` post-RFC does ONE more thing than before — branch on `proc_macros: bool`. The function stays single-responsibility ("load the HIR database"); the parameter selects the upstream config flavour. No SRP violation.

Stable abstractions: `cfdb-hir-extractor`'s public surface gains one function parameter. Minor signature change; downstream callers (`cfdb-cli`) updated in the same PR. No SDP violation.

### 5.4 Rust systems

Three concerns:

**RS-1 — Lifetime (BLOCKING in v1; addressed in v2 §3.1 / §4 I7).** `load_workspace_at` returns `(RootDatabase, Vfs, ProcMacroClient)`; the third element owns the subprocess handle. Salsa keeps live references to expanders inside the DB. Dropping the client at `build_hir_database`'s end of scope kills the subprocess and breaks lazy expansion during the caller's VFS walk. v1 of this RFC inherited the pre-RFC pattern of discarding the third element via `_proc_macro_client`; v2 changes the return type to `(RootDatabase, Vfs, ProcMacroClient)` and bounds the client's lifetime to the CLI sub-command's stack frame.

**RS-2 — Sysroot binary availability (BLOCKING in v1; addressed in v2 §3.1 / §3.3 case 1).** `ProcMacroServerChoice::Sysroot` requires `rust-analyzer-proc-macro-srv` in the active sysroot. The binary ships with the `rust-analyzer` rustup component — not with stable rustc. Stock CI runners that install rustc via apt/dnf/curl-bash without `rust-analyzer` will be missing the binary. v1 of this RFC's "hard fail" policy would have broken CI on every PR in that scenario; v2 adds an availability probe (`proc_macro_server_available()`) at `build_hir_database` entry that falls back to `ProcMacroServerChoice::None` + stderr warning when the binary is missing. This is the ONLY tolerant-fallback case; retry-after-`Err` from `load_workspace_at` is still hard-fail.

**RS-3 — `proc_macro_cwd` determinism (NON-BLOCKING; documented in v2 §3.6).** `ra_ap_load_cargo` injects the workspace's absolute path into every macro expansion. Macros that capture this verbatim leak path-specific bytes into the keyspace. The §3.5 same-workspace determinism check covers regression in fixed-position runs; cross-position (e.g., CI tempdir vs local clone) drift is a future-RFC concern. No mitigation policy in 043-A.

remaining lens questions (feature-flag-vs-runtime-flag if any survives, wall-clock budget calibration, deny-list candidacy for known non-deterministic macros).

## 6. Non-goals

- **Retry-after-`Err` tolerant fallback / `--strict-proc-macro` flag / `extract.proc_macro_status` keyspace metadata / `cfdb schema-describe` extension.** Availability probe (§3.3 case 1) + hard fail on macro-panic Err (§3.3 case 2) + continue-on-lazy-failure (§3.3 case 3) + `--no-proc-macro` escape is the entire policy. Speculative additions land in follow-up RFCs only when a concrete consumer is named.
- **`ProcMacroPolicy` wrapper enum.** `bool` is the upstream-faithful shape.
- **Multi-process expansion.** `proc_macro_processes > 1` is a tuning question; not in scope.
- **Persistent macro-expansion caching.** Salsa is in-memory; ephemeral per extract run.
- **Telemetry attribution.** "Which macro caused this slowdown" is operator-useful but out of scope.
- **Schema vocabulary additions.** No new node label, edge label, or `:CallSite` attribute.
- **Backfill.** Operators re-extract.
- **Synthetic macro-determinism fixture.** cfdb-self is macro-heavy enough.

## 7. Issue decomposition

**Architect prescription convention.** Each slice carries a 4-row `Tests:` block per CLAUDE.md §2.5.

### 7.1 Slice 043-A — flip the flag + CLI escape + determinism extension + empirical close-out

Single vertical slice. The PR body carries the qbot-core empirical measurement and the cfdb-recall baseline refresh; no separate measurement slice.

**Scope:**

- Change `build_hir_database`'s signature: accept `proc_macros: bool`, return `(RootDatabase, Vfs, ProcMacroClient)`. Caller (`cfdb-cli::extract` sub-command) reads `--no-proc-macro` CLI flag (default `false`), passes `!no_proc_macro` through, and owns the returned `ProcMacroClient` for the lifetime of the extraction walk.
- Add `proc_macro_server_available()` probe inside `build_hir_database`. When `proc_macros: true` AND probe returns `false`, fall back to `ProcMacroServerChoice::None` with a stderr warning naming the missing binary path (§3.3 case 1).
- Update `:CallSite.callee_resolved` descriptor in `crates/cfdb-core/src/schema/describe/nodes.rs` with the epistemic-precision sentence (§4 I6).
- Extend `ci/determinism-check.sh` with a `--hir` extract pair against the cfdb workspace itself. Assert sha256 equality.
- Refresh `cfdb-recall` baseline numbers (the metric improves; document the new floor).
- PR body: qbot-core `--context trading` empirical before/after table; cfdb-self wall-clock measurement against I3 budget; explicit note on whether the availability probe fired (CI baseline vs sysroot-with-rust-analyzer baseline).

**Tests:**
```
- Unit: build_hir_database accepts both proc_macros: bool values; ProcMacroClient is returned and held by caller; availability probe returns true on a sysroot with `rust-analyzer-proc-macro-srv` and false on a tmpdir-stubbed sysroot without it; CLI --no-proc-macro flag parses round-trip.
- Self dogfood (cfdb on cfdb): `cfdb extract --workspace . --hir` produces a keyspace where ≥3 named :CallSites flip from callee_resolved=false to callee_resolved=true on a sysroot with proc-macro-srv installed. Concrete qnames in PR body. `ci/determinism-check.sh` --hir mode exits 0 (G1 holds on macro path).
- Cross dogfood (graph-specs-rust @ pinned SHA): `ci/cross-dogfood.sh` exits 0 with post-RFC binary. Macro-light workspace; no regression on arch-ban-*.
- Target dogfood (qbot-core @ pinned SHA): `cfdb scope --context trading` unwired count drops materially below 1534 (the pre-RFC ceiling). Number in PR body; final lower bound is council-prescribed.
```

## 8. References

- Issue [#398](https://agency.lab:3000/yg/cfdb/issues/398) — RFC-stub origin.
- Issue [#378](https://agency.lab:3000/yg/cfdb/issues/378) — cfdb-042-test-bench-entry-points 042-C close-out; the 12.5% ceiling motivating this RFC.
- cfdb-042-test-bench-entry-points — test/bench `:EntryPoint` kinds (the layer below this RFC).
- cfdb-029-code-facts-database v0.2 §A2 — `:EntryPoint` vocabulary distribution prediction.
- cfdb-033-cross-dogfood#4 I5 — SchemaVersion-lockstep invariant (not triggered by this RFC per I4).
- CLAUDE.md §1 (RFC-first), §2.3 (council), §2.5 (Tests template), §3 (dogfood enforcement).
- `crates/cfdb-hir-extractor/src/hir_db.rs:40-48` — current `LoadCargoConfig` declaration site.
- `ra_ap_load_cargo::ProcMacroServerChoice` — the upstream enum the `bool` parameter selects.
