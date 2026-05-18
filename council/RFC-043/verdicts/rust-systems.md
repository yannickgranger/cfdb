# RFC-043 verdict — rust-systems

**Verdict:** RATIFY (with one implementation-time note)
**Author:** rust-systems sub-agent (Round 2)
**Date:** 2026-05-18

---

## D1. Verdict on the RFC as written

### R1 blocking findings — disposition

**RS-1 (BLOCKING in R1 — CLEARED in v2)**

The v2 fix is sound at the ownership level. The RFC §3.1 design code block changes the return type of `build_hir_database` to `(RootDatabase, Vfs, ProcMacroClient)`, and §4 I7 makes the lifetime invariant explicit: the third element MUST outlive the salsa DB's last use. The caller stack frame in `cfdb-cli::extract` owns all three and drops them together.

One type-level inaccuracy in the RFC's design code block needs to be caught at implementation time, not at RFC ratification: `load_workspace_at` at `ra_ap_load-cargo-0.0.328/src/lib.rs:61` returns `anyhow::Result<(RootDatabase, Vfs, Option<ProcMacroClient>)>` — the third element is `Option<ProcMacroClient>`, not bare `ProcMacroClient`. The `Option` collapses to `None` both when the availability probe fires the fallback path (§3.3 case 1) and when `ProcMacroServerChoice::None` is used (via `proc_macro_server.and_then(Result::ok)` at `lib.rs:204`). The public signature of `build_hir_database` as documented in §3.1 should therefore return `(RootDatabase, Vfs, Option<ProcMacroClient>)` to remain upstream-faithful. The lifetime invariant in §4 I7 still holds: the `Option` element binds the drop of the subprocess handle to the caller's scope whether it contains `Some(client)` or `None`. This is an implementation-time correction, not a design flaw — the RFC's ownership reasoning is correct.

**RS-2 (BLOCKING in R1 — CLEARED in v2)**

The v2 mitigation (§3.1 / §3.3 case 1) specifies an explicit `proc_macro_server_available()` probe before calling `load_workspace_at`. Reading the upstream source confirms why this probe is necessary: when `ProcMacroServerChoice::Sysroot` is chosen and `find_sysroot_proc_macro_srv()` returns `None` (binary absent), the `load_workspace_into_db` code at `ra_ap_load-cargo-0.0.328/src/lib.rs:113` maps the outer `None` to an inner `None` — not `Some(Err(...))`. Down at `lib.rs:204`, `proc_macro_server.and_then(Result::ok)` then gives `None`. The function returns `Ok((db, vfs, None))`. No `Err` is raised. Without the pre-call probe, the caller sees a successful load and an empty `ProcMacroClient`, with zero macro expansion silently occurring.

The RFC's design correctly threads around this by probing before calling `load_workspace_at`. One implementation challenge is worth documenting: the canonical probe mechanism is `ProjectWorkspace::find_sysroot_proc_macro_srv()` at `ra_ap_project_model-0.0.328/src/workspace.rs:737`, which delegates to `self.sysroot.discover_proc_macro_srv()` at `sysroot.rs:206-218`. This requires a materialized `ProjectWorkspace`. Since `build_hir_database` currently calls `load_workspace_at` which internally constructs a `ProjectWorkspace`, the implementer has two options: (a) call `ProjectManifest::discover_single` + `ProjectWorkspace::load` as a pre-step to probe the sysroot, then pass the workspace into `load_workspace` (bypassing `load_workspace_at`), or (b) implement a lighter probe — `probe_for_binary` at `ra_ap_toolchain-0.0.328/src/lib.rs:139-144` performs a plain `is_file()` filesystem stat on the two sysroot paths (`<sysroot_root>/libexec/rust-analyzer-proc-macro-srv` and `<sysroot_root>/lib/rust-analyzer-proc-macro-srv`), and the sysroot root can be obtained via `rustup show active-toolchain` + path inference or via `RUSTUP_HOME` env var. This is a simpler implementation than constructing a full `ProjectWorkspace` twice.

Either approach satisfies the RFC's probe requirement. The RFC deliberately does not prescribe the probe implementation shape (§2 "Does NOT ship" cuts the `proc_macro_server_available()` signature from the RFC surface), which is correct — the implementer picks the lighter option.

The v2 mitigation is architecturally sound. RS-2 is cleared.

### Non-blocking findings from R1 — disposition

**RS-3 (proc_macro_cwd determinism) — ACCEPTED as documented**

v2 §3.6 documents the risk clearly: `proc_macro_cwd` is an absolute path injected per `ra_ap_hir_expand-0.0.328/src/proc_macro.rs:310`; macros that capture it verbatim into output break cross-machine G1. The §3.5 same-workspace determinism extension covers the same-machine case. Cross-machine portability is a future-RFC concern. The Expander implementation at `ra_ap_load-cargo-0.0.328/src/lib.rs:579` confirms `current_dir` is passed through to `self.0.expand(...)` at line 731 — it is live in every expansion call. The documentation in §3.6 is accurate. Acceptance with no mitigation in 043-A is the correct posture given the YAGNI constraint.

**RS-4 (feature-flag) — ACCEPTED**

The Cargo.toml at `crates/cfdb-hir-extractor/Cargo.toml:37-38` confirms `ra_ap_proc_macro_api` and `ra_ap_load-cargo` are already unconditional workspace dependencies of `cfdb-hir-extractor`. No new compile-time gate is needed. The runtime `--no-proc-macro` flag is the correct and sufficient discriminator. The RFC retains this design. RS-4 is accepted as-is.

**RS-5 (proc_macro_processes consistency) — ACCEPTED**

`proc_macro_processes: 1` when `proc_macros: true` and `0` when `false` is cleaner documentation than `0` in both cases. The upstream code at `lib.rs:112-135` only consumes `proc_macro_processes` inside `ProcMacroClient::spawn`, which is not called when policy is `Disabled`. The RFC §3.1 design sets `proc_macro_processes: if proc_macros { 1 } else { 0 }` — correct for documentation clarity, harmless in the disabled branch. RS-5 is accepted.

### Overall D1 verdict

The two R1 blockers are addressed. The v2 RFC is architecturally sound. The `Option<ProcMacroClient>` type-level inaccuracy in the §3.1 code block is an implementation-time fix, not a ratification blocker. **RATIFY.**

---

## D2. Tests prescription for slice 043-A

The R1 verdict prescribed tests for four slices (043-A through 043-D). After the YAGNI trim, only 043-A remains. The tests prescription below supersedes the R1 D2 prescription for 043-B/C/D (which are now folded into 043-A or deferred).

```
Tests:
  - Unit:
      (1) build_hir_database with proc_macros=true on a workspace that has the proc-macro-srv binary
          installed: assert the returned Option<ProcMacroClient> is Some, and that the caller can
          drop it after a mock VFS walk without panic. Requires a sysroot with rust-analyzer
          component installed; gate this test behind #[cfg_attr(not(ci_minimal), test)] or a
          feature flag so stock-CI (without rust-analyzer component) skips it rather than fails.
      (2) build_hir_database with proc_macros=true on a tmpdir workspace (no proc-macro-srv binary):
          assert the returned Option<ProcMacroClient> is None AND that stderr contains the warning
          naming the missing binary path (§3.3 case 1). Use a stub sysroot directory without the
          binary. This exercises the availability-fallback path without a real sysroot.
      (3) CLI flag round-trip: cfdb extract --no-proc-macro parses without error; the resulting
          clap-derived value is true for no_proc_macro; build_hir_database receives proc_macros=false.
          Use clap::Command::try_get_matches_from — no subprocess needed.
      (4) proc_macro_server_available() equivalent (however the implementer names the probe):
          returns false on a tmpdir sysroot, returns true on the live sysroot (skip on CI_MINIMAL).

  - Self dogfood (cfdb on cfdb):
      `cfdb extract --workspace <cfdb-root> --hir` against the cfdb workspace on a sysroot WITH
      rust-analyzer component. Assert:
        (a) ci/determinism-check.sh extended with --hir extract pair exits 0 (G1 holds on macro path).
        (b) At least 3 :CallSite nodes that had callee_resolved=false in the pre-RFC keyspace now
            have callee_resolved=true. Candidate sites: Cucumber #[given]/#[when]/#[then] step
            bodies where World-receiver method calls previously collapsed. List concrete
            (caller_qname, callee_qname) pairs in PR body.
        (c) cfdb-recall baseline numbers are refreshed; new baseline MUST NOT regress vs old
            (I2: callee_resolved_count_post_043 >= callee_resolved_count_pre_043).

  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA b542af3):
      ci/cross-dogfood.sh exits 0 with the post-RFC binary. No ban-rule rows expected: existing
      arch-ban-*.cypher rules match on :Item layer attributes, not :CallSite.callee_resolved.
      The proc-macro flag-flip expands callee_resolved=true coverage but adds no new nodes that
      would match a ban rule. This is a regression-only check (I3 schema unchanged, I5 cross-fixture
      pin not bumped).

  - Target dogfood (qbot-core at pinned SHA):
      `cfdb scope --context trading` (or equivalent unwired-count query) on the post-RFC keyspace.
      Assert unwired count drops materially below the pre-RFC 1534 ceiling. PR body MUST include a
      before/after table: context | pre-043 unwired | post-043 unwired | delta%. Council acceptance
      criterion: any measurable reduction (the 87% gap estimate from §1.2 is aspirational; a 20%+
      additional reduction from 1534 would demonstrate the feature works; a <5% reduction would
      indicate the qbot-core dep closure is not macro-heavy in the receiver-type-resolution sense,
      and the PR body must explain why). The qbot-core keyspace MUST NOT be tagged proc_macro_status
      degraded (the availability probe must NOT fire on the canonical sysroot used for this run).
```

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

**Expected lower bound:** at least 3 concrete `(caller_qname, callee_qname)` pairs that flip from `callee_resolved=false` (pre-RFC keyspace) to `callee_resolved=true` (post-RFC keyspace). The candidacy rationale: cfdb's test suite uses Cucumber step annotations (`#[given]`, `#[when]`, `#[then]`) whose parameter-type rewrites are invisible without proc-macro expansion, and cfdb's own crates use `#[derive]` and `#[tokio::test]` throughout. With proc-macros enabled, method calls on macro-generated types or into macro-rewritten function bodies should resolve.

**G1 check:** `ci/determinism-check.sh` with the `--hir` extension (§3.5) must exit 0. The two consecutive `--hir` extracts must produce sha256-identical keyspace JSON on the same machine. This is the byte-stability proof on the macro path.

**cfdb-recall:** the recall baseline refresh is a deliverable of 043-A. The PR body MUST include before/after recall numbers. The new floor must not be lower than the old floor on any metric (I2).

### 043-A cross-dogfood

**Concrete regression check:**
```bash
ci/cross-dogfood.sh  # exits 0 with post-043 binary against graph-specs-rust @ pinned SHA b542af3
```

Expected exit code: 0. The schema is unchanged (I4), the cross-fixture pin is not bumped (I5), and the existing ban rules do not read `:CallSite.callee_resolved`. The proc-macro expansion expands the set of `callee_resolved=true` sites in graph-specs-rust's keyspace but does not introduce new vocabulary that the ban rules would match.

### 043-A target-dogfood

**Concrete acceptance shape:**

PR body must include this table:

```
Context    | pre-043 unwired | post-043 unwired | delta (abs) | delta%
trading    | 1534            | ?                | ?           | ?
```

The council prescribes: any measurable reduction is a pass; the PR body must report the actual number and explicitly note whether the availability probe fired (i.e., whether the qbot-core run used a sysroot with the rust-analyzer component installed). If the reduction is under 5%, the PR body must include a `cargo tree -p qbot-core | grep -iE "async-trait|derive-builder|tokio|cucumber"` snippet demonstrating which macro crates are present, as evidence that the macro coverage is genuinely sparse rather than that the probe silently degraded.

---

## D4. Determinism risk enumeration

This is the rust-systems lead deliverable.

### Summary verdict

The §3.5 same-workspace determinism extension (two consecutive `--hir` extracts on the same machine, sha256-compared) is **sufficient for 043-A**. No deny-list is required to ship 043-A. The reasoning:

### Risk class 1: macros reading `current_dir` at expansion time

`ra_ap_hir_expand-0.0.328/src/proc_macro.rs:310` passes `proc_macro_cwd` (the package manifest parent as an absolute path string) to every expansion via the `Expander::expand` signature at `ra_ap_load-cargo-0.0.328/src/lib.rs:579`. A macro that verbatim embeds this path into its output will produce different bytes when extracted from two different absolute checkout paths.

cfdb's own `Cargo.lock` does not contain `vergen`, `shadow-rs`, or `build-info` (verified: `grep "^name" Cargo.lock | grep -iE "shadow|vergen|build.info"` returns empty). cfdb-self extraction is safe.

For qbot-core: the implementer MUST run `cargo tree -p qbot-core 2>/dev/null | grep -iE "vergen|shadow|build.info"` before the 043-A PR merges and report the result in the PR body. If any HIGH-risk crate is present, a follow-up RFC must document the cross-machine portability limitation explicitly in §4 I1. This is not a blocker for 043-A ratification because (a) the G1 invariant is same-machine by the documented scope in §3.6, and (b) the qbot-core target dogfood run for 043-A uses a single fixed sysroot and absolute path, so same-machine stability is all that is required for the empirical close-out.

### Risk class 2: macros reading `CARGO_*` env vars at expansion time

`env!()` and `env_var!()` builtins are resolved by rustc, not by proc-macro expansion. The risk is proc-macros that call `std::env::var(...)` at expansion time. These are rare in well-maintained crates. The `time-macros-0.2.27` crate in cfdb's lock file performs compile-time literal parsing only — not a risk. Assessment: LOW for cfdb-self, UNKNOWN for qbot-core.

### Risk class 3: macros reading system time

`ra_ap_proc_macro_api-0.0.328/src/lib.rs` `dylib_last_modified: Option<SystemTime>` reads file metadata at dylib load time for salsa change-detection only — it does not embed into macro output. No crate in cfdb's lock file uses `chrono::Utc::now()` at expansion time. Assessment: LOW.

### Risk class 4: other non-determinism sources

Random seeds (`uuid::Uuid::new_v4()` inside a macro body), hostname, PID — none present in cfdb's dep closure. The `proc_macro_api` protocol uses IPC (not shared memory), so process isolation is maintained.

### Deny-list decision

No deny-list in 043-A. cfdb's own proc-macro footprint (serde_derive, clap_derive, salsa-macros) is composed of well-behaved macros. The qbot-core dep audit (cargo tree grep) is a prerequisite for the 043-A PR merge — reviewer blocks merge if the grep reveals a HIGH-risk crate without documentation. If a HIGH-risk crate is found, the deny-list implementation shape is: `load_proc_macro(server, path, ignored_macros: &[Box<str>])` at `ra_ap_load-cargo-0.0.328/src/lib.rs:468` accepts a `ignored_macros` slice that disables specific macro names by name. This keeps the no-ratchet rule intact (no config file; per-invocation argument).

---

## D5. Wall-clock budget verdict

**Verdict: 4x cap as the REJECT threshold is retained. A 2x WARNING threshold is strongly recommended but not a ratification blocker.**

### Reasoning for retaining 4x reject

The v2 RFC collapses the earlier multi-slice design into a single vertical slice. The empirical measurement lands in the same PR body as the code. If the 4x cap fires, the PR is blocked at merge time — not post-ratification. This makes the RFC ratification's 4x threshold a low-stakes choice: the implementer takes the risk, and the PR body measurement catches the breach before anything merges.

The R1 verdict's argument for a 2x warning threshold remains valid for operational guidance: rust-analyzer benchmarks show single-process proc-macro expansion adds 1.3x–2.0x to total workspace load time on medium-sized workspaces. 4x would indicate pathological expansion. However, with `proc_macro_processes = 1` and cfdb-self being a modestly macro-dense workspace (serde_derive, clap_derive, salsa-macros, tokio-macros, a handful of others — estimated 8-12 unique proc-macro dylibs), the realistic ceiling is closer to 1.5x–2.5x.

**R2 recommendation:** the RFC text at §3.4 and the I3 invariant should document both thresholds inline:

```
Post-043 wall-clock on cfdb-self:
  WARNING zone: > 2× pre-043 (~10 s) — PR body MUST document which dylib causes excess and
                  whether proc_macro_processes > 1 would help (deferred to follow-up RFC).
  REJECT zone:  > 4× pre-043 (~20 s) — RFC is rejected pending tuning or partial expansion.
```

This is a documentation edit to §3.4 and I3, not a design change. The convener may incorporate it as a final pre-ratification edit or defer to the implementer's PR body, per discretion. It does not block RATIFY.

### Startup-cost concern (R2 new finding — LOW severity)

The v2 design runs `proc_macro_server_available()` at every `build_hir_database` call. The underlying `probe_for_binary` at `ra_ap_toolchain-0.0.328/src/lib.rs:139-144` performs a plain `is_file()` filesystem stat on two paths (`<sysroot>/libexec/rust-analyzer-proc-macro-srv` and `<sysroot>/lib/rust-analyzer-proc-macro-srv`). This is a negligible cost under normal sysroot conditions (local filesystem, warm VFS cache) — two stat syscalls.

The concern raised in the council brief about network-mounted or Docker overlay sysroots is valid in principle. In practice: (a) cfdb's self-dogfood and qbot-core target-dogfood runs use a local rustup-managed sysroot; (b) the CI runner for this project uses a local sysroot per the infrastructure quick reference. A network-mounted sysroot would affect all other Cargo operations too — cfdb is not uniquely exposed. This concern is noted but does not warrant RFC text modification.

---

## D6. Failure-mode policy verdict

### D6.1 Is the §3.3 availability-fallback + hard-fail policy correct?

**Yes.** The three cases in §3.3 cover the failure surface correctly:

- **Case 1** (sysroot binary missing): availability probe fires before `load_workspace_at`; fallback to `ProcMacroServerChoice::None` + stderr warning. This is the CI-on-stock-rustc path. Upstream behavior confirmed: with `Sysroot` policy and the binary absent, `load_workspace_at` returns `Ok` with `None` for the client — the degradation is invisible without the pre-call probe. The v2 design correctly adds the probe to make degradation visible before the call.

- **Case 2** (load fails after probe passed): `HirError::LoadWorkspace` propagates. The operator re-runs with `--no-proc-macro`. No retry. This is correct — a failed `load_workspace_at` with the binary present indicates a macro panic or workspace corruption, not an availability problem.

- **Case 3** (lazy expansion fails during VFS walk): `callee_resolved=false` for the affected call sites. Walk continues. This is the correct graceful-degradation behaviour — individual expansion failures should not abort the whole extraction.

The R1 verdict's concern about distinguishing retriable vs non-retriable errors (RS-1's "fallback discriminator" requirement) is NO LONGER LOAD-BEARING in v2. The v1 RFC's "retry-after-Err" tolerant fallback has been cut (SYNTHESIS-R1 §Pre-trim). Without retry logic, there is no need to classify errors as retriable vs non-retriable. The `HirError::LoadWorkspace` variant carries the error message from `load_workspace_at`, which gives the operator sufficient signal to diagnose. The RS-1 blocking finding in v1 was specifically about the retry discrimination problem — v2 eliminates retry entirely, so the finding is moot.

### D6.2 Per-Item proc-macro flag

The decision to NOT add a per-`:Item` flag is correct. The §4 I6 descriptor caveat (a sentence in the schema descriptor noting the epistemic precision shift post-RFC-043) is the minimal-footprint signal. A per-`:Item` flag would require:

1. A schema vocabulary addition — new attribute on `:CallSite` or `:Item` — triggering a SchemaVersion bump and graph-specs-rust lockstep (RFC-033 §4 I5).
2. An extractor implementation that tracks which salsa query results crossed a proc-macro expansion boundary — this is not exposed by `ra_ap_hir`'s public API at the `resolve_method_call` call site level.

The keyspace-level signal (the descriptor caveat + the stderr warning in §3.3 case 1) is the right granularity. Downstream consumers re-extract to get the post-RFC keyspace. This is the stated operator contract (§4 I6 / §6 non-goals). **No change requested.**

---

## Change requests summary

No blocking change requests.

One implementation-time correction (not a ratification blocker):

| ID | Severity | Section | Note |
|---|---|---|---|
| RS-2-impl | LOW | §3.1 code block | The return type `(RootDatabase, Vfs, ProcMacroClient)` should be `(RootDatabase, Vfs, Option<ProcMacroClient>)` to match `load_workspace_at`'s actual return type at `ra_ap_load-cargo-0.0.328/src/lib.rs:61`. The availability fallback (§3.3 case 1) and the `ProcMacroServerChoice::None` path both produce `None` for the client. The I7 lifetime invariant is unaffected — `Option<ProcMacroClient>` still binds the subprocess handle lifetime to the caller's scope. |
| RS-D5-docs | LOW | §3.4 + I3 | Add a 2x WARNING threshold alongside the 4x REJECT threshold in §3.4 and I3, for operational guidance. Suggested text in D5 above. |

Both are documentation corrections the convener or implementer may fold in without re-convening the council.

**Final verdict: RATIFY.**
