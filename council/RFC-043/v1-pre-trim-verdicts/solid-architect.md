# RFC-043 verdict — solid-architect

**Verdict:** REQUEST CHANGES
**Author:** solid-architect sub-agent
**Date:** 2026-05-18

---

## D1. Verdict on the RFC as written

### SRP on `build_hir_database` post-fallback-logic — CHANGE REQUEST (load-bearing)

The current `build_hir_database` at `crates/cfdb-hir-extractor/src/hir_db.rs:40-58` is a thin,
single-responsibility function: construct `LoadCargoConfig`, delegate to `load_workspace_at`,
map the error, return `(RootDatabase, Vfs)`. Its module-doc at line 1 states its responsibility
cleanly: "load a Cargo workspace into a monomorphic salsa `RootDatabase`, paired with the VFS
needed to enumerate files during extraction." One reason to change: if `LoadCargoConfig` fields
or the `load_workspace_at` signature changes.

RFC §3.1 adds `ProcMacroPolicy` threading — acceptable, because `LoadCargoConfig` construction
is already the function's stated job. The function mapping `policy` to `ProcMacroServerChoice`
and `proc_macro_processes` fields is a transparent extension of the existing construction
responsibility. This alone does NOT violate SRP.

RFC §3.3 is the SRP violation. The fallback retry path requires `build_hir_database` to:

1. Call `load_workspace_at` with `ProcMacroServerChoice::Sysroot`.
2. On `Err`, log a structured warning naming (a) the originating command, (b) the `HirError`,
   (c) the workspace path (RFC §4 I6).
3. Retry `load_workspace_at` with `ProcMacroServerChoice::None`.
4. Tag keyspace metadata with `proc_macro_status: "degraded"`.

Responsibility 4 — tagging keyspace metadata — is not about loading a workspace. Keyspace
metadata (`extract.proc_macro_status`) belongs to the `cfdb-core` schema layer. The function's
current return type is `Result<(RootDatabase, Vfs), HirError>`. To emit `proc_macro_status`,
the function would need to return an extended tuple `Result<(RootDatabase, Vfs, ProcMacroStatus), HirError>`
or mutate an output parameter. Either shape embeds keyspace-layer semantics into a workspace-loader
function. This is a second reason to change the function: if keyspace metadata schema evolves
(e.g., adding more granular status codes), `build_hir_database` must change even though the
workspace-loading logic is unchanged.

Responsibility 2 — emitting the structured warning — also crosses the boundary. The warning
names "the originating cfdb command" (RFC §4 I6 item a). `build_hir_database` does not know
which CLI command invoked it; it only knows the `workspace_root`. Encoding the originating
command name in the warning requires either passing it as a parameter (API pollution) or relying
on caller context that the function cannot cleanly access. This is orchestration-level knowledge,
not workspace-loading knowledge.

The combined fallback-retry-plus-status-tagging logic belongs in an orchestration layer that:
- owns the retry policy
- holds the invocation context (CLI subcommand name)
- decides what to report to the keyspace metadata layer
- calls `build_hir_database` once or twice based on policy

**CHANGE REQUEST 1 (load-bearing for SRP):** Introduce `extract_orchestrator.rs` in
`cfdb-hir-extractor/src/` (or, if the clean-arch lens agrees, place it in `cfdb-cli/src/compose.rs`
as a composition-root concern). The orchestrator function signature should be:

```rust
pub fn load_hir_workspace(
    workspace_root: &Path,
    policy: ProcMacroPolicy,
    strict: bool,
) -> Result<(RootDatabase, Vfs, ProcMacroStatus), HirError>
```

Where `ProcMacroStatus` is a new `pub enum { Enabled, Degraded, Disabled }` in this crate.
`build_hir_database` retains its current single responsibility (construct config, call
`load_workspace_at`, map error). The keyspace metadata tagging (`proc_macro_status` attribute
write) is handled by the CLI layer consuming the returned `ProcMacroStatus`. The structured
warning is emitted by the orchestrator, not by the loader.

**Preferred placement:** `cfdb-hir-extractor/src/hir_db.rs` contains ONLY `build_hir_database`
(the loader). A new `cfdb-hir-extractor/src/extract_orchestrator.rs` contains `load_hir_workspace`
(the retry-policy + status-reporting orchestrator). The CLI compose layer calls
`load_hir_workspace`. Keyspace metadata tagging happens in the CLI layer from the returned
`ProcMacroStatus`. This keeps `cfdb-hir-extractor` as the owner of retry policy (appropriate,
since the crate owns the `HirError` variants and knows which errors are retriable) while
keeping `build_hir_database` SRP-clean.

If clean-arch lens or rust-systems lens disagrees on the placement (compose.rs vs. orchestrator
module), council reconciles — but the SRP split itself is not negotiable: the loader and the
retry-policy-plus-status-tag must be in different functions.

---

### `ProcMacroPolicy` as a public enum — ACCEPTABLE with OCP note

RFC §3.1 introduces `ProcMacroPolicy::{Enabled, Disabled}` as a public enum in
`cfdb-hir-extractor`. The crate is consumed by `cfdb-hir-petgraph-adapter` (unconditionally)
and `cfdb-cli` (behind the `hir` feature flag). Both are in the same workspace, so the
"independently versionable" REP concern is not load-bearing here.

The enum is two-variant and closed. The RFC §3.1 shape:

```rust
match policy {
    ProcMacroPolicy::Enabled => ProcMacroServerChoice::Sysroot,
    ProcMacroPolicy::Disabled => ProcMacroServerChoice::None,
}
```

OCP question: if a future RFC adds `ProcMacroPolicy::AsyncRetry` or per-crate gating, does
this extend cleanly? The `match` is exhaustive, so adding a variant forces callers to update.
For the two-variant case this is acceptable — the alternative (a trait or builder pattern) would
be over-engineering. However, the enum should be marked `#[non_exhaustive]` so that adding a
third variant in a future RFC is a non-breaking change for downstream consumers outside the
workspace. Without `#[non_exhaustive]`, a downstream user who pattern-matches on
`ProcMacroPolicy` (e.g., a hypothetical `cfdb-cloud-extract` crate) would get a compile error
on upgrade.

**CHANGE REQUEST 2 (minor, OCP compliance):** Mark `ProcMacroPolicy` as `#[non_exhaustive]`
at its declaration site. The crate's own `match` arms in `build_hir_database` (or the proposed
orchestrator) are in the same crate and are unaffected. Any future in-workspace consumer can
still use exhaustive matches; the attribute only affects out-of-workspace consumers.

---

### Feature flag vs. runtime flag — SOLID lens agrees with RFC recommendation

RFC §5.4 recommends NO compile-time feature flag (`--features hir-proc-macro`) and favors
runtime `--no-proc-macro`. From the SOLID lens:

The SDP question: would a `hir-proc-macro` feature flag make `cfdb-hir-extractor` less stable?
No — the crate already depends on `ra_ap_proc_macro_api` unconditionally (Cargo.toml:37,
`ra_ap_proc_macro_api.workspace = true`). A feature flag that controls whether `build_hir_database`
passes `ProcMacroServerChoice::Sysroot` vs `None` is not a compile-time dependency gate; it is
a behavior gate over an already-linked library. The compile-time isolation ship has sailed at
the `ra_ap_proc_macro_api` dep level, not at the `ProcMacroServerChoice` enum variant level.

From the ISP perspective: splitting `cfdb-hir-extractor` into `hir-proc-macro`-gated and
non-gated surfaces would give consumers a false sense that they can depend on a "lighter"
version of the crate without proc-macro overhead. But the overhead is in the subprocess spawn,
not in linking. A feature flag here would increase the CI matrix without reducing the dependency
surface. RFC recommendation is sound from a SOLID/ISP lens.

**No change request on the feature-flag question.**

---

### Stable abstractions for `cfdb-hir-extractor` public surface — ACCEPTABLE

Current public surface (from source inspection):

| Symbol | Location | Type |
|---|---|---|
| `build_hir_database` | `hir_db.rs:40` | pub fn |
| `extract_call_sites` | `lib.rs:83` (re-export from `call_site_emitter`) | pub fn |
| `extract_entry_points` | `lib.rs:84` (re-export from `entry_point_emitter`) | pub fn |
| `HirError` | `error.rs:15` | pub enum |
| `CallSiteEmitter` | `emit.rs:59` | pub trait |
| `EmitStats` | `emit.rs:84` | pub struct |

RFC-043 adds `ProcMacroPolicy` (new pub enum) and changes `build_hir_database`'s signature.

SAP check: `cfdb-hir-extractor` is an instability contributor (high Ce — it depends on 11+
`ra_ap_*` crates and `cfdb-core`). Its Ca is exactly two non-self-referential consumers:
`cfdb-hir-petgraph-adapter` and `cfdb-cli` (feature-gated). Instability I = Ce/(Ca+Ce) ≈ high.
A high-I crate should lean concrete, not abstract — the current surface is appropriately
concrete (`pub fn`, `pub enum`, one trait for the orphan-rule adapter pattern). Adding
`ProcMacroPolicy` as a concrete enum is consistent with the existing abstraction posture.

SDP check: Does adding `ProcMacroPolicy` to `cfdb-hir-extractor` create a dependency-direction
problem? `cfdb-hir-petgraph-adapter` already depends on `cfdb-hir-extractor` unconditionally.
The adapter's `impl CallSiteEmitter` does not call `build_hir_database`; it only consumes the
`(Vec<Node>, Vec<Edge>)` output. Whether the adapter needs to import `ProcMacroPolicy` depends
on whether any function in the adapter takes it as a parameter. Per the RFC design, `ProcMacroPolicy`
flows CLI → `build_hir_database` only; the adapter never sees it. No SDP concern.

ADP check: cfdb-cli → {cfdb-hir-extractor, cfdb-hir-petgraph-adapter} → {cfdb-core}. No cycles.
`ProcMacroPolicy` does not introduce any new dependency edge. The RFC adds no new `[dependencies]`
entry to `cfdb-hir-extractor/Cargo.toml`.

---

### Component-level CRP/CCP — ACCEPTABLE

The four slices span correct component boundaries:

- 043-A: `cfdb-hir-extractor` (new enum + signature change) + `cfdb-cli` (flag plumbing). These
  change for one reason each: extractor changes when proc-macro policy vocabulary changes, CLI
  changes when the CLI contract changes.
- 043-B: `cfdb-hir-extractor` (fallback logic — or the proposed orchestrator module). Changes
  when retry policy changes.
- 043-C: cross-repo empirical measurement. No code change.
- 043-D: `cfdb-recall` baseline update. Changes when the recall baseline changes.

CCP check: the proposed `extract_orchestrator.rs` (CR1 target) groups retry-policy logic with
structured-warning emission — both change when the failure-handling policy changes. This is the
correct CCP group. Keeping it separate from `build_hir_database` (which changes when the
workspace-loading interface changes) maintains CCP purity.

---

### Summary of change requests

| # | Severity | Target | Description |
|---|---|---|---|
| CR1 | REQUIRED | RFC §3.3 + `hir_db.rs` | Hoist fallback retry + structured warning + `ProcMacroStatus` return into a new `extract_orchestrator.rs` function; `build_hir_database` remains SRP-clean (loader only). The CLI layer reads returned `ProcMacroStatus` to write keyspace metadata. |
| CR2 | MINOR | RFC §3.1 | Mark `ProcMacroPolicy` `#[non_exhaustive]` at declaration. Preserves OCP for out-of-workspace consumers when a third variant is added in a future RFC. |

CR1 is required for RATIFY. CR2 is minor but should be addressed in the same slice.

If CR1 is resolved (RFC §3.3 updated to name `extract_orchestrator.rs` as the retry-policy home
and `build_hir_database` retains its current thin-wrapper shape), and CR2 is noted in RFC §3.1,
verdict changes to RATIFY.

---

## D2. Tests prescription

### Slice 043-A

- Unit: `ProcMacroPolicy` `Debug`/`Display` assertions. `LoadCargoConfig` wiring round-trip:
  given `ProcMacroPolicy::Enabled`, assert `with_proc_macro_server == ProcMacroServerChoice::Sysroot`
  and `proc_macro_processes == 1`; given `ProcMacroPolicy::Disabled`, assert `::None` and `0`.
  CLI flag mutual-exclusion: assert that passing both `--no-proc-macro` and `--strict-proc-macro`
  returns a CLI argparse error. Place in `crates/cfdb-hir-extractor/tests/proc_macro_policy.rs`
  for the enum/config tests; CLI flag test in `crates/cfdb-cli/tests/extract_flags.rs`.
- Self dogfood: (defer to D3 for concrete Cypher shape)
- Cross dogfood: (defer to D3 for exit-code contract)
- Target dogfood: `cfdb scope --context trading --keyspace qbot-core --format json` reports
  `unwired` count < 1300 (vs pre-043 1534). Number committed to PR body. Lower-bound of 1300
  is the SOLID lens' minimum bar; council prescribes the exact ceiling — see D5 note that if
  the 4x wall-clock budget holds, the 1300 ceiling is sound.

### Slice 043-B

- Unit: Orchestrator retry logic in isolation. Mock the `load_workspace_at` call via a
  `LoadWorkspaceFn: Fn(&Path, &CargoConfig, &LoadCargoConfig, &dyn Fn(Message)) -> anyhow::Result<(RootDatabase, Vfs, ProcMacroClient)>`
  parameter (or a trait if the rust-systems lens prefers). Given a stub that returns `Err` on
  the first call (simulating sysroot failure) and `Ok` on the second call (simulating successful
  None-mode fallback): assert (a) the warning is emitted to the structured log, (b) the returned
  `ProcMacroStatus` is `Degraded`, (c) the function returns `Ok`. Strict-mode variant: same first
  `Err` call, assert `load_hir_workspace` returns `Err` immediately without retrying.
- Self dogfood: `cfdb extract --workspace . --hir --strict-proc-macro` MUST succeed on cfdb-self.
  This proves cfdb's own crates expand cleanly under the sysroot proc-macro server. Assert
  exit code 0 and `proc_macro_status: "enabled"` in the keyspace metadata output of
  `cfdb schema-describe`.
- Cross dogfood: `cfdb extract --hir` on `tests/fixtures/broken_proc_macro/` (the deliberately-
  broken fixture shipped in 043-B) MUST: exit 0 in tolerant mode with `proc_macro_status: "degraded"`;
  exit non-zero in `--strict-proc-macro` mode. The fixture is part of the 043-B scope and ships
  in the same PR. Assert the structured warning message contains the workspace path.
- Target dogfood: `cfdb extract --workspace qbot-core --hir --strict-proc-macro` on the pinned
  qbot-core SHA. PR body documents the result: either success (proc-macros expand cleanly) or
  names the offending macro + the structured warning text. This is a sanity-check signal, not a
  merge gate — the real gate is that tolerant-mode extract (no `--strict-proc-macro`) succeeds.

### Slice 043-C

- Unit: none — rationale: empirical measurement slice, no new code.
- Self dogfood: none — rationale: 043-A self-dogfood already covers cfdb-self recall. 043-C is
  a qbot-core measurement slice, not a cfdb-self exercise.
- Cross dogfood: none — rationale: graph-specs-rust coverage already provided by 043-A.
- Target dogfood: THE artifact of this slice. 8-context unwired delta table in PR body with three
  columns: `pre-043 (post-RFC-042)`, `post-043 default`, `post-043 --no-proc-macro`. From the
  SOLID lens, the acceptance criterion is ≥ 50% additional reduction beyond the RFC-042 12.5%
  ceiling (i.e., `unwired` drops from 1534 to ≤ 767 in at least the `trading` context). If
  reduction is < 30%, the RFC premise is rejected and 043-D/043-E are scoped as described in
  RFC §7.3. The ≥ 50% threshold is this lens' prescription; the convener should reconcile with
  the ddd and clean-arch lenses' views on acceptable recall improvement.

### Slice 043-D

- Unit: recall baseline assertion (existing `cfdb-recall` corpus check), updated with post-043
  numbers. Assert that the new baseline is strictly ≥ the pre-043 baseline (I2 invariant —
  recall non-regression).
- Self dogfood: `cfdb-recall` run on cfdb-self with post-043 binary. Assert recall percentage
  does not decrease. Report new percentage in PR body.
- Cross dogfood: none — rationale: recall is a corpus tool measuring extractor coverage against
  rustdoc ground truth. graph-specs-rust is a companion policy tool, not a recall corpus.
- Target dogfood: none — rationale: recall measures extractor coverage per crate, not workspace-
  level `unwired` counts. qbot-core target measurement is already 043-C's artifact.

---

## D3. Dual-dogfood proof discipline

### 043-A self-dogfood

Concrete Cypher and assertion shape:

```cypher
MATCH (cs:CallSite)
WHERE cs.callee_resolved = true
  AND cs.file =~ '.*cfdb-hir-extractor.*'
RETURN count(cs) AS resolved_in_extractor_crate
```

Run before and after applying the 043-A change. The `resolved_in_extractor_crate` count should
increase by at least 1 (the RFC §3.6 candidate: `call_site_emitter.rs` Semantics receiver calls).
This is the minimum bar. The RFC §3.6 requirement for ≥ 3 named concrete call sites that flip
from `callee_resolved=false` to `callee_resolved=true` is the stronger gate; the PR body MUST
list those three qnames.

Rationale: `call_site_emitter.rs` at `crates/cfdb-hir-extractor/src/call_site_emitter.rs`
(20.1K — the largest source file in the crate) calls `Semantics<'db, RootDatabase>` methods.
`ra_ap_hir`'s own `Semantics` impl is touched by salsa's `#[salsa::query_group]` macro. With
proc-macros disabled, some receiver-type resolutions on `Semantics` methods collapse. Enabling
proc-macros should resolve at least the calls documented in RFC §3.6 first bullet.

The BDD/test candidate (RFC §3.6 third bullet) — Cucumber `#[given]`/`#[when]`/`#[then]`
consumers in `crates/cfdb-*/tests/` — is a valid second source. However, self-dogfood should
prefer the production-code call sites (first and second bullets) as stronger signal than test-
file call sites, since `callee_resolved` on test code is lower-priority for the `unwired`
classifier.

### 043-A cross-dogfood

Expected exit code: 0 on all existing `.cfdb/queries/*.cypher` rules against `yg/graph-specs-rust`
at the current pinned SHA (`b542af3` per BRIEF §4). RFC §4 I3 confirms no `SchemaVersion` bump,
so RFC-033 §4 I5 lockstep does not trigger. The `proc_macro_status` metadata attribute is
keyspace-header–level, not schema-vocabulary–level; it does not appear in any `MATCH` clause
in the four existing graph-specs-rust ban rules.

Verification step in the PR: run `ci/cross-dogfood.sh` and confirm exit code 0. If exit 30, a
new finding was introduced by the proc-macro change on the macro-light companion workspace —
this would be a true positive requiring companion-side cleanup, not a false positive, because
macro-light workspaces do not trigger proc-macro-resolution path changes.

### 043-C target-dogfood

Acceptance table shape (to be reported in PR body):

| Context | Pre-043 `unwired` | Post-043 default | Post-043 `--no-proc-macro` | Delta (default) |
|---|---:|---:|---:|---:|
| trading | 1534 | ? | ? | ?% |
| infrastructure | 1086 | ? | ? | ?% |
| (other 6 contexts) | … | … | … | …% |

Acceptance criterion: `trading` context `unwired` count ≤ 767 (≥ 50% reduction from 1534).
If the delta column shows < 30% reduction on ANY context that contains macro-heavy code
(async_trait, derive(Builder) patterns confirmed in qbot-core), the RFC premise is rejected.
The `Post-043 --no-proc-macro` column serves as the regression check: it MUST equal the
pre-043 numbers within ± 5 (minor variance from keyspace re-extract ordering is acceptable).

---

## D4. Determinism risk enumeration

This is primarily the rust-systems lens' responsibility; the SOLID lens contributes the
architectural consequence.

From a SOLID/SRP perspective, if any macro in the dep closure is non-deterministic at expansion
time (reads `chrono::Utc::now()`, `std::env::var("BUILD_TIME")`, or file modification times),
the correct architectural response is NOT to add a deny-list to `build_hir_database` — that
would give the loader a third responsibility (policy enforcement on macro behavior).

**Architectural prescription:** if a deny-list is warranted (rust-systems lens decides the
empirical question), it belongs in the orchestrator layer (`extract_orchestrator.rs` per CR1).
Specifically, a `ProcMacroPolicy::Filtered { deny: Vec<String> }` variant (or a pre-flight
check function `validate_macro_determinism(workspace_root)`) belongs at the orchestration layer,
not in `build_hir_database`. This is consistent with CR1 — the orchestrator owns policy
decisions, the loader owns workspace loading.

If rust-systems concludes no deny-list is warranted and the tolerant fallback (§3.3) is
sufficient, the SOLID lens has no objection. The G1 invariant verification (`ci/determinism-check.sh`
extension with the macro-heavy fixture) is the enforcement mechanism; a deny-list is an
optimization, not a correctness requirement.

---

## D5. Wall-clock budget verdict

The RFC's 4x cap (RFC §3.4) is assessed from the SOLID/SAP lens: is the abstraction stable
enough to enforce a budget cap at this granularity?

The 4x cap is reasonable as a ratification gate (not a runtime enforcement). The key reason:
`proc_macro_processes = 1` is the bounding constraint — one subprocess serializes all macro
expansion. The wall-clock cost on cfdb-self (5s pre-043, ≤ 20s post-043) is a 4x absolute
ceiling on a workspace with moderate macro density. qbot-core's 3-min extract is large but
is a serial pipeline — 4x is consistent with the linear scaling of adding one subprocess to
workspace load.

However, the SOLID lens raises a component stability concern: the 4x cap is stated as a
per-invocation empirical number, but there is no enforcement mechanism in the code. If a
future RFC adds `proc_macro_processes > 1` (the §6 non-goal deferred to 043-E), the cap
becomes meaningless.

**Prescription:** the 4x cap should be encoded as a CI assertion in the 043-A slice:
`ci/wall-clock-budget.sh` (or an extension of `ci/determinism-check.sh`) measures the
macro-heavy fixture extract time and asserts it is ≤ 4 × the `--no-proc-macro` time on the
same fixture. This makes the cap a testable invariant, not just a statement in the RFC.
If the rust-systems lens sees a reason to prefer a stricter 2x cap, the SOLID lens has no
objection — 2x is safer for SAP (more stable abstraction contract) but the empirical data
from 043-A should decide.

---

## D6. Failure-mode policy verdict

### Is tolerant fallback the right default?

Yes. From the SRP/OCP lens: operators should not be forced to debug proc-macro server failures
as part of routine extract runs. The default should be maximally useful (best-effort recall),
not maximally strict. The `--strict-proc-macro` flag is the correct escape hatch for CI gates
that need recall guarantees.

The `proc_macro_status` metadata attribute in the keyspace JSON header is the correct signal
granularity for the default path. It is keyspace-level, not per-item-level, which is appropriate
because the proc-macro server is workspace-wide (RFC §3.3: "the salsa DB is workspace-wide").
A per-`:Item` flag would require a schema vocabulary change (violating RFC §4 I3) and would
pollute the `:CallSite` schema with infrastructure status information — a CCP violation (the
`:CallSite` vocabulary changes for extractor-infrastructure reasons, not for domain reasons).

**No change request on the tolerant-fallback default.**

### Is per-Item proc-macro flag needed?

No. From the ISP lens: consumers of the keyspace (`/sweep-epic`, `/operate-module`, the
§−1 resorbing-loop rule) need to know whether to trust `callee_resolved=false` as a true
absence or as a proc-macro gap. The keyspace-level `proc_macro_status` attribute provides this
signal at the right granularity: if `status == "enabled"`, every `callee_resolved=false` is
a genuine absence; if `status == "degraded"`, the `unwired` classifier output is suspect and
should be interpreted with pre-RFC-043 confidence. No consumer needs finer granularity at the
per-item level, and adding it would be a schema vocabulary change (violating I3).

If a future field analysis shows that degraded-mode extracts have heterogeneous macro coverage
(some crates succeeded, some failed), a per-crate status attribute would be the correct
granularity — still not per-item. This is a future RFC concern, not RFC-043 scope.

**No change request on per-item flag question.**
