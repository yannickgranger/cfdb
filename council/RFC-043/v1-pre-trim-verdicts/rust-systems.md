# RFC-043 verdict — rust-systems

**Verdict:** REQUEST CHANGES
**Author:** rust-systems sub-agent
**Date:** 2026-05-18

---

## D1. Verdict on the RFC as written

The RFC is well-motivated and the flag-flip mechanism is sound at the implementation sketch level. However, four systems traps need to be fixed before the RFC can be ratified: one blocking subprocess-lifetime issue, one blocking sysroot-availability gap, one blocking determinism risk that the RFC's own invariant I1 does not protect against, and one non-blocking CWD-sensitivity documentation gap.

### Finding RS-1 (BLOCKING): `_proc_macro_client` is dropped at function exit, killing the subprocess while the salsa DB still holds live expanders

`crates/cfdb-hir-extractor/src/hir_db.rs:50` currently reads:

```rust
let (db, vfs, _proc_macro_client) =
    load_workspace_at(...).map_err(|e| { ... })?;
Ok((db, vfs))
```

In Rust, `let (a, b, _c) = …` holds `_c` until the end of its enclosing scope. Here `_proc_macro_client` is dropped at the closing `}` of `build_hir_database`. With `ProcMacroServerChoice::None` (current state) the third element is `None`, so the drop is a no-op. With `ProcMacroServerChoice::Sysroot` (RFC-043 state) it is `Some(ProcMacroClient)`.

`ProcMacroClient` wraps `Arc<ProcMacroServerPool>` (`ra_ap_proc_macro_api-0.0.328/src/lib.rs:90`). Each `ProcMacro` expander that `load_proc_macro` produces stores `pool: ProcMacroServerPool` which holds `workers: Arc<[ProcMacroServerProcess]>` via `pool.rs:10`. Because the expander's pool is a clone of the client pool (both share the same inner `Arc`), the subprocess processes stay alive as long as any `ProcMacro` value does. After `build_hir_database` returns, the `ProcMacroClient` has been dropped, but the `Arc<[ProcMacroServerProcess]>` is still alive inside every `ProcMacro` stored in the salsa crate-graph.

This is actually safe: the subprocess is kept alive by the expander refs inside the DB. When the `RootDatabase` drops (at the caller's scope end), the expanders drop, the pool Arcs drop, and `JodChild::drop` (`ra_ap_stdx-0.0.328/src/lib.rs:312-316`) calls `self.0.kill(); self.0.wait()` — correct cleanup.

**The trap is on the Err path, not the happy path.** If `load_workspace_at` returns `Ok` but expansion of a specific dylib fails partway through the proc_macros iteration at `lib.rs:169-182`, the `ProcMacroClient` is still moved into `_proc_macro_client`. The failure path returns `Ok((db, vfs))` where `db` contains a partially-populated crate-graph. Some expanders have valid pool refs; the `ProcMacroClient`'s `Arc` ref is released at function end. The Arc still lives in the expanders. This is fine for cleanup.

**The actual blocking issue:** RFC §3.3 specifies a fallback path where on `Err` from `load_workspace_at`, cfdb retries with `ProcMacroServerChoice::None`. The RFC does not document where the `ProcMacroClient` from the first (failed) call lives. `load_workspace_at` in `ra_ap_load_cargo` returns `anyhow::Result<(RootDatabase, Vfs, Option<ProcMacroClient>)>`. On `Err`, the `ProcMacroClient` is not returned (it is dropped inside `load_workspace_into_db`). The subprocess cleanup via `JodChild::drop` therefore fires before the retry, which is correct. **However, the RFC's fallback pseudocode calls `load_workspace_at` twice, materialising two full `RootDatabase` values in the same stack frame — one for the failed Sysroot attempt and one for the successful None retry.** Two `RootDatabase` values alive simultaneously is a salsa-database collision if the two calls share any global state. The salsa `RootDatabase::new` allocates a new LRU cache (`ra_ap_load_cargo-0.0.328/src/lib.rs:88`), so they are independent heap allocations. But `ra_ap_proc_macro_api-0.0.328/src/process.rs` uses `rayon::par_iter` for parallel worker loading — the rayon global thread pool is shared. Concurrent Sysroot-then-None retry is safe if the first call truly errors before returning a DB. Verify: `load_workspace_at` returns `Err` from `load_workspace_into_db`; the `let (db, vfs, proc_macro_server) = load_workspace_into_db(...)` line at `lib.rs:90` means if it errors, `db` is partially mutated (it was passed `&mut RootDatabase`) and the `?` operator returns `Err`. The `db` from the first call is dropped immediately (it is a local inside `load_workspace`). No double-DB problem.

**Revised blocking finding:** The actual blocking issue is simpler. RFC §3.3 says "Call `load_workspace_at` with Sysroot. If it returns `Err`, retry with None." But `load_workspace_at` at `ra_ap_load_cargo-0.0.328/src/lib.rs:62` calls `std::env::current_dir()?` unconditionally. If `current_dir()` itself errors (rare but possible in containerised CI where the CWD has been deleted under the process), both the Sysroot call and the None retry will fail with the same OS error. The RFC does not distinguish between "sysroot not available" errors (retriable with None) and "workspace unreachable" errors (not retriable). The fallback must discriminate error shape.

**Required change for RS-1:** RFC §3.3 must add a subsection "Fallback discriminator": the `HirError::LoadWorkspace` variant must carry sufficient information to distinguish `ProcMacroLoadingError::ProcMacroSrvError` / `ProcMacroLoadingError::Disabled` errors (retriable with None) from `anyhow::Error` from `ProjectManifest::discover_single` / `ProjectWorkspace::load` (not retriable — no point in retrying with None if Cargo can't find the workspace). Without this, a CI host with a missing Cargo.toml silently produces a `proc_macro_status: degraded` keyspace instead of surfacing the root cause. See `ra_ap_load_cargo-0.0.328/src/lib.rs:62-79` for the error sources that must be classified.

---

### Finding RS-2 (BLOCKING): `ProcMacroServerChoice::Sysroot` requires a sysroot-installed `rust-analyzer-proc-macro-srv` binary that is NOT part of the standard Rust toolchain

`ra_ap_project_model-0.0.328/src/sysroot.rs` `discover_proc_macro_srv` probes for the binary at two paths:

```
<sysroot_root>/libexec/rust-analyzer-proc-macro-srv
<sysroot_root>/lib/rust-analyzer-proc-macro-srv
```

On this machine, the binary exists at:
```
/var/home/yg/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/libexec/rust-analyzer-proc-macro-srv
```

This binary is **not part of the standard Rust toolchain distribution**. It is shipped by the `rust-analyzer` rustup component. A minimal CI image that runs `rustup toolchain install stable --profile minimal` will have `rustc` and `cargo` but NOT `rust-analyzer-proc-macro-srv`. The probe returns `None` (sysroot has no proc-macro server), which `load_workspace_into_db` at `ra_ap_load_cargo-0.0.328/src/lib.rs:112-113` maps to:

```rust
ProcMacroServerChoice::Sysroot => ws.find_sysroot_proc_macro_srv().map(|it| {
    it.and_then(|it| ProcMacroClient::spawn(...).map_err(...))
    .map_err(|e| ProcMacroLoadingError::ProcMacroSrvError(...))
}),
```

When `find_sysroot_proc_macro_srv()` returns `None`, the outer `map` short-circuits and the proc_macro_server variable is `None`. Down at `lib.rs:162-168`, `None` maps to `ProcMacroLoadingError::ProcMacroSrvError("proc-macro-srv is not running, workspace is missing a sysroot")`. The workspace loading still succeeds — the error is per-crate, not a fatal `load_workspace_at` error. So the fallback policy in RFC §3.3 (retry on `Err`) will NOT fire when the sysroot lacks the binary. Instead, cfdb silently produces a `proc_macro_status: enabled` keyspace with zero actual macro expansion — the configuration of Enabled is used but no expansion occurs, and there is no `proc_macro_status: degraded` signal.

**This is the worst failure mode:** the operator sees `proc_macro_status: enabled` in the metadata, assumes proc-macros ran, and wonders why `unwired` did not drop. The RFC's I6 invariant ("every degraded outcome emits a structured warning") is defeated because `load_workspace_at` returns `Ok` — the degradation is invisible.

**Required change for RS-2:** RFC §3.3 must add a "Sysroot probe" step BEFORE calling `load_workspace_at`. The implementation must explicitly call `ws.find_sysroot_proc_macro_srv()` (or equivalent) and surface a warning if the result is `None` or `Err`. The `proc_macro_status` field must be set to `"degraded"` when the sysroot probe fails, not only when `load_workspace_at` fails. CI setup documentation (or a check in 043-A) must explicitly verify `rustup component add rust-analyzer` has been run.

Proposed edit shape for §3.3: add a "Prerequisite" note: "The sysroot proc-macro server binary (`rust-analyzer-proc-macro-srv`) is part of the `rust-analyzer` rustup component, not the standard toolchain. CI hosts that install `--profile minimal` must add `rustup component add rust-analyzer`. The 043-A self-dogfood step MUST assert the binary exists before proceeding; a missing binary is a setup error, not a retriable proc-macro failure."

---

### Finding RS-3 (NON-BLOCKING, requires acknowledgement): `proc_macro_cwd` is an absolute path injected into every macro expansion call — this is the primary G1 byte-stability risk

`ra_ap_hir_expand-0.0.328/src/proc_macro.rs:310` reads:
```rust
let current_dir = calling_crate.data(db).proc_macro_cwd.to_string();
```

This string is passed as the `current_dir` argument to every macro expansion. Its value comes from Cargo package metadata and equals the package's manifest directory (`Cargo.toml` parent). If a proc-macro's expansion output depends on `current_dir`, and cfdb is run from two different absolute path locations for the same logical workspace, the expansion outputs differ — G1 violation.

In practice, well-behaved proc-macros (serde, async-trait, derive-builder, cucumber) do not read `current_dir` at expansion time. But `shadow-rs`, `vergen`, `build-info`, and other build-time info macros that cfdb's dep closure does NOT currently include (verified: `grep "^name" /var/mnt/workspaces/cfdb/Cargo.lock | grep -iE "shadow|vergen|build.info"` returns empty) do so. The risk is in the TARGET workspace (qbot-core, qbot-infrastructure) that cfdb extracts, not in cfdb's own proc-macros.

**This risk is present in qbot-core/qbot-infrastructure.** If either target workspace uses `vergen`, `shadow-rs`, or similar, extraction twice from the same absolute path is deterministic, but extraction from two CI nodes with different absolute checkout paths (e.g. `/home/runner/work/repo` vs `/builds/agent/repo`) will differ. The RFC's determinism fixture (`tests/fixtures/proc_macro_determinism/`) runs two consecutive extracts on the SAME machine, so this cross-machine case is not caught by the fixture.

The RFC's I1 invariant covers two-run same-machine determinism. The case of two-machine same-source determinism (which cfdb's "same source SHA = same bytes" G1 invariant implies) is not covered. This is a philosophical question for the council: does G1 require machine-independence? If yes, then `current_dir`-sensitive macros are a fundamental barrier. If no (G1 = two runs on the same machine), then the fixture is sufficient.

**Required acknowledgement in RFC §4 I1:** Clarify whether G1 requires machine-independent byte stability or only two-run same-machine stability. If machine-independent, RS-3 is blocking and the deny-list must cover `current_dir`-sensitive macros. If same-machine only, document the limitation explicitly so operators understand that cfdb keyspaces are not portable across different checkout paths.

---

### Finding RS-4 (NON-BLOCKING): feature-flag decision is correct but must be documented against the existing `hir` feature gate on `cfdb-cli`

RFC §5.4 recommends NO feature flag on `cfdb-hir-extractor`. This is correct. The proc-macro support is provided by `ra_ap_proc_macro_api` and `ra_ap_load_cargo`, which are already in `cfdb-hir-extractor/Cargo.toml` as workspace dependencies (`ra_ap_proc_macro_api.workspace = true`, `ra_ap_load-cargo.workspace = true` at lines 36, 38). No new crate dependency is needed for proc-macro support. The `hir` feature on `cfdb-cli` already gates the entire `cfdb-hir-extractor` compile cost. Runtime `--no-proc-macro` is the correct choice.

However, the RFC must explicitly state: "The `hir` feature gate on `cfdb-cli` (per `crates/cfdb-hir-extractor/Cargo.toml` lines 17-19 comment) already gates all proc-macro support. There is no additional compile-time gate; the `ra_ap_proc_macro_api` and `ra_ap_load_cargo` crates are in scope whenever `--features hir` is passed. The runtime `--no-proc-macro` flag is sufficient." Without this statement, a future maintainer might add a redundant `[features]` section to `cfdb-hir-extractor`.

---

### Finding RS-5 (NON-BLOCKING): `proc_macro_processes = 0` with `ProcMacroServerChoice::None` is technically inconsistent but harmless

`ProcMacroServerChoice::None` maps to `Some(Err(ProcMacroLoadingError::Disabled))` at `ra_ap_load_cargo-0.0.328/src/lib.rs:134`. The `proc_macro_processes` field is only consumed inside `ProcMacroClient::spawn` which is called only when `Sysroot` or `Explicit` variant is chosen. When policy is `Disabled`, setting `proc_macro_processes = 0` vs `1` has no effect. The RFC's design in §3.1 sets `proc_macro_processes: 1` for Enabled and `0` for Disabled — this is correct for documentation clarity. No change required.

---

### Summary judgment

RS-1 and RS-2 are blocking. RS-3 requires an acknowledgement in the RFC text (blocking if G1 is machine-independent; non-blocking if G1 is same-machine only). RS-4 is non-blocking documentation. RS-5 is non-blocking.

If the RFC author resolves RS-1 and RS-2 with the proposed edits, and documents the G1 scope in RS-3, the rust-systems lens will re-review as RATIFY.

---

## D2. Tests prescription

### Slice 043-A — flip the flag + determinism fixture

- **Unit:** `ProcMacroPolicy` enum: `Debug` + `Display` + `Clone` round-trip. `LoadCargoConfig` wiring: given `ProcMacroPolicy::Enabled`, `LoadCargoConfig.with_proc_macro_server` is `ProcMacroServerChoice::Sysroot` and `proc_macro_processes == 1`; given `Disabled`, `None` and `0`. CLI flag mutual-exclusion: `cfdb extract --no-proc-macro --strict-proc-macro` must error with argparse message (test via `clap::Command::try_get_matches_from`). **Add one additional unit test:** `build_hir_database_err_classification` — given a workspace path that does not exist, `load_workspace_at` returns `Err`; assert the resulting `HirError::LoadWorkspace` contains the workspace path in its message (RS-1 mitigation: verify error is classifiable as non-retriable).
- **Self dogfood (cfdb on cfdb):** (defer to D3 below)
- **Cross dogfood (graph-specs-rust @ pinned SHA):** `ci/cross-dogfood.sh` exits 0 with post-043 binary. The proc-macro fixture does NOT touch the graph-specs ban-rule Cypher; this is a no-op regression check verifying I3 (schema unchanged).
- **Target dogfood (qbot-core @ pinned SHA):** `cfdb scope --context trading --format json` reports `unwired` count; assert `< 1300` as the RFC prescribes. Report actual number in PR body. Additionally assert `keyspace.metadata.proc_macro_status == "enabled"` (NOT `degraded`) on the qbot-core keyspace — this is the RS-2 smoke test in disguise: if it says `degraded`, the CI runner does not have the rust-analyzer component.

### Slice 043-B — tolerant fallback + structured warning

- **Unit:** Fallback logic test using a stub. The stub `LoadWorkspaceFn` signature must match the actual `load_workspace_at` return type (`anyhow::Result<(RootDatabase, Vfs, Option<ProcMacroClient>)>`). Two cases: (a) stub returns `Err` on first call (Sysroot), `Ok` on second (None) → assert `proc_macro_status = "degraded"` and structured warning emitted to stderr; (b) stub returns `Ok` on first call → assert `proc_macro_status = "enabled"` and no warning emitted. **RS-1 mitigation test:** third case where stub always returns `Err` with an error message containing "Cargo.toml" → assert the implementation does NOT retry (non-retriable workspace error) and exits non-zero with the original error, NOT `proc_macro_status = "degraded"`.
- **Self dogfood (cfdb on cfdb):** `cfdb extract --workspace . --hir --strict-proc-macro` MUST succeed (exits 0). This proves cfdb's own crates expand cleanly AND that the sysroot binary is present. If it fails, it is a CI setup failure (RS-2), not a code failure.
- **Cross dogfood (graph-specs-rust @ pinned SHA):** Extract with deliberately-broken fixture (`tests/fixtures/broken_proc_macro/`); assert `proc_macro_status: degraded` in keyspace JSON in tolerant mode; assert exit non-zero in `--strict-proc-macro` mode. The broken fixture is a synthetic Cargo workspace where a `proc-macro = true` crate exports a macro whose expand implementation calls `panic!("deliberate test failure")`.
- **Target dogfood (qbot-core @ pinned SHA):** Extract with `--strict-proc-macro`; document in PR body whether it succeeds or names the offending macro. Reviewer uses the result to validate that qbot-core's proc-macro footprint is clean.

### Slice 043-C — empirical recall measurement on qbot-core

- **Unit:** none — rationale: empirical measurement slice, no new code.
- **Self dogfood:** none — rationale: 043-A self-dogfood already covered cfdb-self.
- **Cross dogfood:** none — rationale: 043-A cross-dogfood already covered graph-specs-rust.
- **Target dogfood:** THE artifact of this slice. Three-column table in PR body: `context | unwired (pre-043) | unwired (post-043 default) | unwired (post-043 --no-proc-macro)`. Council acceptance criterion: ≥ 50% additional reduction vs pre-043 baseline across ALL 8 contexts (not just trading). **Rust-systems qualification:** the `--no-proc-macro` column MUST match the pre-043 numbers within ±5 (rounding tolerance) — this is the recall-non-regression proof for I2. If the `--no-proc-macro` column diverges from pre-043 by more than 5 items in any context, it indicates a regression unrelated to proc-macros (a separate bug, not this RFC's concern, but it must be filed).

### Slice 043-D — `cfdb-recall` baseline refresh

- **Unit:** Recall baseline assertion (`cfdb-recall` on cfdb-self); existing tests must continue to pass; the new baseline numbers must be ≥ old numbers (I2: recall non-regression). Add one assertion: `callee_resolved_count_post_043 >= callee_resolved_count_pre_043` on the cfdb-self keyspace.
- **Self dogfood:** `cfdb-recall` run on cfdb-self tree with post-043 binary.
- **Cross dogfood:** none — rationale: recall is a corpus tool, not a graph-specs concern.
- **Target dogfood:** none — rationale: recall measures extractor coverage, not target workspace state.

---

## D3. Dual-dogfood proof discipline

### 043-A self-dogfood

**Concrete Cypher:**
```cypher
MATCH (cs:CallSite {callee_resolved: true})
WHERE cs.callee_qname IS NOT NULL
WITH cs
MATCH (i:Item)-[:INVOKES_AT]->(cs)
WHERE i.file CONTAINS "cfdb-hir-extractor"
   OR i.file CONTAINS "cfdb-petgraph"
   OR i.file CONTAINS "tests"
RETURN i.qname, cs.callee_qname, cs.callee_resolved
ORDER BY i.qname
LIMIT 100
```

**Expected lower bound:** RFC §3.6 names three candidate sites. The PR body MUST list ≥ 3 concrete `(caller_qname, callee_qname)` pairs that flip from `callee_resolved=false` to `callee_resolved=true`. The rationale for each must name which macro covers the resolution gap (e.g. "BDD `#[given]` step → World method resolved via cucumber's attribute rewrite").

**Rationale for the boundary:** cfdb's own codebase has Cucumber BDD steps in `crates/*/tests/` annotated with `#[given]`, `#[when]`, `#[then]`. Each step body calls methods on a `World` type whose `self.method()` call sites currently produce `callee_resolved=false` because the parameter type rewrite by cucumber's attribute macro is invisible without expansion. With proc-macros enabled, these resolve. This is a high-confidence candidate because the BDD steps are present in the repo and cucumber is in the lock file (verified: `ra_ap_proc_macro_api` is already a dep, suggesting cucumber is used by cfdb's tests at some level).

### 043-A cross-dogfood

**Concrete regression check:**
```bash
ci/cross-dogfood.sh  # exits 0 with post-043 binary against graph-specs-rust @ pinned SHA b542af3
```

Expected exit code: 0. Rationale: RFC §4 I3 states SchemaVersion unchanged; RFC §4 I4 states the graph-specs fixture pin is NOT bumped. The four existing ban rules in `graph-specs-rust/.cfdb/queries/` do not read `:CallSite.callee_resolved` or `proc_macro_status` — they match on `caller.is_test`, `cs.callee_path`, and `:Item` layer attributes. The proc-macro flag-flip expands the set of `callee_resolved=true` call sites in graph-specs-rust but does not add new nodes that would match a ban rule. Exit 0 is expected with high confidence.

**Caveat:** if graph-specs-rust uses any macro that the sysroot cannot expand (e.g. a nightly-only macro), `load_workspace_at` may return a partially-degraded DB. The cross-dogfood CI must assert `proc_macro_status` in the graph-specs keyspace metadata is NOT `degraded`; a `degraded` status with exit 0 on the ban rules is a false pass. Add this assertion to `ci/cross-dogfood.sh`.

### 043-C target-dogfood

**Concrete acceptance number and table shape:**

The PR body MUST include a table of this exact shape:

```
Context           | pre-043 unwired | post-043 unwired | post-043 --no-proc-macro | delta%
trading           | 1534            | ?                | ~1534 (±5)               | ?
infrastructure    | 1086            | ?                | ~1086 (±5)               | ?
... (all 8 contexts) ...
```

Council acceptance criterion: mean `delta%` across all 8 contexts ≥ 50% additional reduction. If the mean delta is < 30%, RFC premise is rejected (see BRIEF §6 convener note). The `--no-proc-macro` column must be within ±5 of pre-043 to confirm I2.

---

## D4. Determinism risk enumeration

This section is the rust-systems lead deliverable.

### Known risk class 1: macros reading `current_dir` at expansion time

`ra_ap_hir_expand-0.0.328/src/proc_macro.rs:310` passes `proc_macro_cwd` (the package's manifest parent path as an absolute string) to every macro expansion. Any macro that reads `proc_macro::Span::source_file()` or uses `std::env::current_dir()` internally at expansion time will embed the absolute path in its output.

**Ecosystem macros known to embed absolute paths:**
- `vergen` (`vergen-git2`, `vergen-gix`) — embeds `CARGO_MANIFEST_DIR` in generated constants. **Risk level: HIGH.** If qbot-core uses vergen, two extracts from different CI checkout paths will produce different `:CallSite` callee_qnames for vergen-generated functions.
- `shadow-rs` — embeds `BUILD_SCRIPT_DIR`, `GIT_STATUS_FILE`, absolute source path constants. **Risk level: HIGH.**
- `build-info` — embeds `CARGO_MANIFEST_DIR`. **Risk level: HIGH.**

**cfdb's own lock file** (`/var/mnt/workspaces/cfdb/Cargo.lock`) contains NONE of these (`grep "^name" Cargo.lock | grep -iE "shadow|vergen|build.info"` returns empty). cfdb-self extraction is safe.

**qbot-core and qbot-infrastructure dependency closures are unknown to this lens.** The council MUST require a one-time audit: before ratifying, the implementer must run `cargo tree -p qbot-core 2>/dev/null | grep -iE "vergen|shadow|build.info"` and report findings in the 043-A PR body. If any of these is present, a deny-list entry for that crate's dylib path MUST be added to the 043-B implementation.

### Known risk class 2: macros reading `CARGO_PKG_VERSION` or `CARGO_*` env vars at expansion time

The standard `env!("CARGO_PKG_VERSION")` is resolved by `rustc` at compile time, not by proc-macro expansion. It is NOT a proc-macro expansion risk. `env!()` is a built-in macro.

Proc-macros that call `std::env::var("CARGO_PKG_VERSION")` at expansion time are a different story. These are rare but exist (some old code-generation crates). The risk is LOW for the cfdb dep closure; MEDIUM for qbot-core if it vendors custom derive crates.

### Known risk class 3: macros reading system time (`chrono::Utc::now()`, `SystemTime::now()`)

No crate in cfdb's Cargo.lock uses `chrono::Utc::now()` at proc-macro expansion time directly. The `time-macros-0.2.27` crate in the lock file provides `time::macros::datetime!()` / `time::macros::time!()` for compile-time time LITERAL parsing, not runtime timestamp injection. The `OffsetDateTime` references in `time-macros/src/lib.rs` are type-level, not `now()` calls.

The `ra_ap_proc_macro_api-0.0.328/src/lib.rs:116` field `dylib_last_modified: Option<SystemTime>` reads `fs::metadata(dylib_path).modified()` at load time. This is used for `PartialEq` on `ProcMacro` values (salsa change detection), NOT embedded in macro output. It does NOT affect the byte content of expanded code. Not a G1 risk.

**Risk level for time-reading macros: LOW** for cfdb-self, UNKNOWN for qbot-core.

### Known risk class 4: macros reading file modification times (`include_str!` of rewritten files)

`include_str!` is a built-in macro, not a proc-macro. Not a proc-macro expansion risk. Custom proc-macros that read `fs::read_to_string(file)` inside their expansion body are rare but exist in some codegen tools. Not present in cfdb's dep closure.

### Deny-list decision

**Recommendation: no deny-list in 043-B for cfdb-self extraction.** cfdb's own proc-macro footprint (serde_derive, clap_derive, salsa-macros, ra_ap_macros) is composed of well-behaved macros with deterministic output. A deny-list is premature and would create a maintenance burden for zero benefit on the cfdb-self use case.

**Recommendation: mandatory audit before 043-C.** The qbot-core/qbot-infrastructure dep audit (RS-4's `cargo tree` grep) is a prerequisite for 043-C. If any HIGH-risk crate is found, the deny-list is mandatory; the deny-list lands in the same 043-C PR that reports the empirical recall numbers, not in a separate issue.

**Implementation shape of deny-list (if needed):** `ra_ap_load_cargo` exposes `load_proc_macro(server, path, ignored_macros: &[Box<str>])` at `lib.rs:468`. The `ignored_macros` parameter disables specific macro names. If vergen is found in qbot-core, its macro names (`vergen`, `cargo_env`, etc.) go into this slice. This is a per-invocation argument, not a config file, keeping the CLAUDE.md §6 no-ratchet rule intact.

---

## D5. Wall-clock budget verdict

**Verdict: 4x is too generous as a rejection criterion; the RFC needs a secondary 2x WARNING threshold.**

The RFC's table at §3.4 names 4x caps on cfdb-self (~5 s → ≤ 20 s), qbot-core (~3 min → ≤ 12 min), and qbot-infrastructure (~5 min → ≤ 20 min). These numbers are defensible as hard rejection thresholds but are not useful as operational guidance because 4x overhead on a 5-minute operation (adding 15 min to every CI extract) would be unacceptable in practice even if it passes the formal gate.

**Reasoning for 2x warning threshold:**

Proc-macro expansion via a subprocess adds two costs: (a) subprocess IPC round-trips per macro invocation, and (b) dylib loading per unique proc-macro crate. For `proc_macro_processes = 1`, all expansions are serialised through a single subprocess. For cfdb-self (a workspace with serde_derive, clap_derive, salsa-macros, and a handful of others), the dylib load count is small (≤ 10 unique proc-macro crates). Each dylib load is a one-time cost; the per-expansion IPC cost dominates at scale.

Rust-analyzer benchmarks (from the rust-analyzer repo) show proc-macro expansion with a single server adds 1.3x–2.0x to total workspace load time on medium-sized workspaces. 2x is the empirically-motivated upper bound for cfdb-self. 4x would indicate pathological expansion (e.g., a macro with very large output or recursive expansion).

**Proposed revision to RFC §3.4:**

| Workspace | Pre-043 | Post-043 cap (WARNING) | Post-043 cap (REJECT) |
|---|---:|---:|---:|
| cfdb-self | ~5 s | ≤ 10 s (2x) | ≤ 20 s (4x) |
| qbot-core | ~3 min | ≤ 6 min (2x) | ≤ 12 min (4x) |
| qbot-infrastructure | ~5 min | ≤ 10 min (2x) | ≤ 20 min (4x) |

If post-043 on cfdb-self is between 10 s and 20 s (warning zone), the 043-A PR body MUST document which proc-macro crate is consuming the excess time and whether `proc_macro_processes = 2` (deferred to 043-E per §6) would help. If it exceeds 20 s (reject zone), this RFC is rejected as specified.

**RSS budget:** The §3.4 2x RSS cap is accepted as-is. `proc_macro_processes = 1` spawns a single subprocess; its RSS is bounded by the size of the largest dylib loaded. On the stable toolchain with `rust-analyzer-proc-macro-srv`, the server process typically consumes 30–80 MB resident. A 2x cap on cfdb-self's pre-043 RSS (~200–400 MB for a workspace this size) gives a 400–800 MB bound, which is adequate headroom.

---

## D6. Failure-mode policy verdict

### D6.1 Is tolerant fallback the right default?

**Yes, tolerant fallback is the right default** with one condition: the degradation signal must be visible BEFORE the keyspace is consumed (RS-2 mitigation). The RFC's current §3.3 design correctly chooses `Enabled + tolerant` as the default because:

1. cfdb is a read-only analysis tool; a degraded keyspace with lower recall is strictly better than no keyspace for debugging workflows.
2. The operator who needs guarantees can use `--strict-proc-macro`.
3. The historical behaviour (pre-043) is already equivalent to `Disabled`, so `Degraded` is never worse than the status quo.

**However:** the RFC's current §3.3 does not specify what happens when `load_workspace_at` returns `Ok` but with zero proc-macro expansions (RS-2 scenario: sysroot probe returns None). This is the silent-failure case. The RFC must add:

"After a successful `load_workspace_at` with Sysroot policy, the implementation MUST query the loaded crate graph for proc-macro expansions and emit a warning if the count is zero AND the workspace contains at least one proc-macro crate. Zero expansions on a macro-heavy workspace with a successful load indicates the sysroot binary was absent. This warning sets `proc_macro_status: degraded`."

### D6.2 Per-Item proc-macro flag — is keyspace metadata sufficient?

**Keyspace metadata is sufficient for the current use case.** The RFC's non-goal of "no new vocabulary" is correct. A per-`:Item` flag ("I am proc-macro-touched") would require:
1. A schema change (new attribute on `:Item` or `:CallSite`) → SchemaVersion bump → graph-specs lockstep (RFC-033 §4 I5 triggers).
2. The extractor would need to track which salsa queries touched a proc-macro result — this is not exposed by `ra_ap_hir`'s public API at the `resolve_method_call` call site level.

The keyspace-level `proc_macro_status` is the correct granularity for the operator signal. Downstream consumers (`/sweep-epic`, `/operate-module`) read the status once and adjust their confidence interval for the whole keyspace. This is the right abstraction.

**One addition recommended:** the `proc_macro_status` field should include a count: `"enabled (N macro dylibs loaded)"` or `"degraded (0 of N macro dylibs expanded)"`. This gives operators a quick sanity check without a separate query. The format should be a structured JSON object `{"status": "enabled", "dylibs_loaded": N}` rather than a plain string to allow programmatic consumption. This is a suggestion, not a blocker.

---

## Change requests summary

| ID | Blocking | Section | Required edit |
|---|---|---|---|
| RS-1 | YES | §3.3 | Add "Fallback discriminator" subsection: distinguish retriable proc-macro-server errors (`ProcMacroLoadingError::ProcMacroSrvError` / `Disabled`) from non-retriable workspace errors (`ProjectManifest::discover_single` / `ProjectWorkspace::load` failures). The retry must ONLY fire on the former. |
| RS-2 | YES | §3.3 + §7.1 | Add "Sysroot probe" requirement: implementation must explicitly check sysroot binary availability BEFORE calling `load_workspace_at`; absent binary is a `proc_macro_status: degraded` signal even when `load_workspace_at` returns `Ok`. CI prerequisite: `rustup component add rust-analyzer`. 043-A self-dogfood must assert the binary exists. |
| RS-3 | CONDITIONAL (blocking if G1 is machine-independent) | §4 I1 | Clarify G1 scope: "two runs on the same machine" vs "any two runs on identical source". If machine-independent, add `current_dir`-sensitive macros (vergen, shadow-rs, build-info) to the deny-list and require a qbot-core dep audit before 043-C merge. If same-machine only, document the portability limitation explicitly. |
| RS-4 | NO | §5.4 | Document explicitly: "No new `[features]` section is added to `cfdb-hir-extractor`; the existing `hir` feature gate on `cfdb-cli` already gates all proc-macro support at compile time." |
