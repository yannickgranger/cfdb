# RFC-043 verdict — solid-architect

**Verdict:** REQUEST CHANGES
**Author:** solid-architect sub-agent
**Date:** 2026-05-18
**Round:** R2
**R1 verdict:** REQUEST CHANGES (CR1: SRP on `build_hir_database`; CR2: `ProcMacroPolicy` `#[non_exhaustive]`)

---

## D1. Verdict on the RFC as written

### R1 CR1 — SRP on `build_hir_database` — RESOLVED

The v1 SRP violation arose because fallback retry logic and keyspace metadata tagging were both inside `build_hir_database`. Both are cut in v2 (SYNTHESIS-R1.md §convener-pre-trim-pass). The v2 function at RFC §3.1 does exactly one thing: construct `LoadCargoConfig` from its input flags, delegate to `load_workspace_at`, propagate the error or return the `(RootDatabase, Vfs, ProcMacroClient)` triple. The module-doc at `crates/cfdb-hir-extractor/src/hir_db.rs:1` accurately describes the function's single reason to change.

The `proc_macro_server_available()` probe inside `build_hir_database` (RFC §3.1 / §3.3 case 1) is startup-time config selection, not orchestration. It answers "which upstream `ProcMacroServerChoice` variant is available on this host?" before constructing the config — within the loader's stated responsibility. R1 CR1 is resolved.

### R1 CR2 — `ProcMacroPolicy` `#[non_exhaustive]` — MOOT

The `ProcMacroPolicy` enum was cut entirely. v2 uses `proc_macros: bool` — the upstream-faithful shape per RFC §3.1 and §6 non-goals. CR2 has no target.

### Stable abstractions (SAP / SDP / ADP) — ACCEPTABLE

Public surface of `cfdb-hir-extractor` post-RFC (from `crates/cfdb-hir-extractor/src/lib.rs:83–86`):

| Symbol | Change |
|---|---|
| `build_hir_database` | Gains `proc_macros: bool` param; return grows to `(RootDatabase, Vfs, ProcMacroClient)` |
| `extract_call_sites`, `extract_entry_points`, `HirError`, `CallSiteEmitter`, `EmitStats` | Unchanged |

SAP: `cfdb-hir-extractor` is high-instability (Ce >> Ca: 11 `ra_ap_*` crates + `cfdb-core`; two workspace-internal consumers). A high-I crate correctly leans concrete. A `bool` parameter and a tuple element are concrete additions. `ra_ap_proc_macro_api` is already declared at `crates/cfdb-hir-extractor/Cargo.toml:37` — no new dependency.

The return type growing to include `ProcMacroClient` is an internal-cluster change: Ca=2, both consumers in the same workspace, updated atomically. RFC §4 I7 makes the lifetime contract explicit. Not an OSS-API stability concern.

SDP: dependency direction unchanged — `cfdb-cli` → `cfdb-hir-extractor` → `ra_ap_*` + `cfdb-core`. `cfdb-hir-petgraph-adapter` calls `CallSiteEmitter::ingest_resolved_call_sites`, not `build_hir_database`, and never sees `ProcMacroClient`. No SDP issue.

ADP: `cfdb-cli` → {`cfdb-hir-extractor`, `cfdb-hir-petgraph-adapter`} → `cfdb-core`. Acyclic. No new dependency edge.

### Feature flag vs. runtime flag — CONFIRMED

`ra_ap_proc_macro_api` is already an unconditional dep (`Cargo.toml:37`). A `--features hir-proc-macro` gate would be a behavior gate over an already-linked library. Runtime flag avoids ISP fragmentation: the adapter crate never calls `build_hir_database` and should not carry a feature axis it doesn't need. Runtime flag is correct. No change request.

### NEW finding — `proc_macro_processes` inconsistency in §3.1 pseudocode

RFC §3.1 code block (`docs/RFC-043-hir-proc-macro-server.md:88–95`):

```rust
with_proc_macro_server: if proc_macros && proc_macro_server_available() {
    ProcMacroServerChoice::Sysroot
} else {
    ProcMacroServerChoice::None
},
...
proc_macro_processes: if proc_macros { 1 } else { 0 },
```

When `proc_macros=true` AND `proc_macro_server_available()` returns false (§3.3 case 1 — CI on stock rustc), this produces `{ with_proc_macro_server: None, proc_macro_processes: 1 }`. The two `LoadCargoConfig` fields that jointly express "am I using the proc-macro server?" diverge in the probe-fires-false path.

If `ra_ap_load_cargo` uses `proc_macro_processes` to decide whether to attempt spawning a subprocess independently of `with_proc_macro_server`, stock-rustc CI runs would attempt a subprocess spawn even when the probe said "fall back." The §3.3 case 1 guarantee ("No `Err` is returned; the extract proceeds in degraded mode") depends on `ra_ap_load_cargo` ignoring `proc_macro_processes` when `with_proc_macro_server=None`. The RFC does not assert this.

From the SRP lens: the function's responsibility is "construct `LoadCargoConfig` with the correct field values for the chosen proc-macro mode." In the probe-fires-false case, the chosen mode is `None + processes=0`. The inconsistency between the two fields is a failure of that sub-responsibility.

**CHANGE REQUEST 1 (minor — §3.1 pseudocode):** Factor the availability check into a local binding so both fields use the same condition:

```rust
let pm_enabled = proc_macros && proc_macro_server_available();
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
```

The §7.1 Unit test row must add: "given `proc_macros=true` AND probe=false: assert `with_proc_macro_server == None` AND `proc_macro_processes == 0`."

---

### Summary of change requests

| # | Severity | Target | Description |
|---|---|---|---|
| CR1 | MINOR | RFC §3.1 code block | Factor `proc_macro_server_available()` into `let pm_enabled`; use for both `with_proc_macro_server` and `proc_macro_processes`. Add unit test for probe-fires-false path. |

Verdict flips to RATIFY on CR1 correction. No R3 needed.

---

## D2. Tests prescription for slice 043-A

- Unit: (1) `proc_macros=false` produces `LoadCargoConfig{with_proc_macro_server: None, proc_macro_processes: 0}`. (2) `proc_macros=true, probe=true` produces `LoadCargoConfig{with_proc_macro_server: Sysroot, proc_macro_processes: 1}`; `ProcMacroClient` returned in `Ok` tuple. (3) `proc_macros=true, probe=false` (tmpdir-stubbed sysroot without `rust-analyzer-proc-macro-srv`) produces `LoadCargoConfig{with_proc_macro_server: None, proc_macro_processes: 0}`; function returns `Ok` (not `Err`); stderr warning emitted. (4) `--no-proc-macro` CLI flag parses round-trip. Place (1–3) in `crates/cfdb-hir-extractor/tests/hir_db_config.rs`; (4) in `crates/cfdb-cli/tests/extract_flags.rs`.
- Self dogfood (cfdb on cfdb): `cfdb extract --workspace . --hir` on a sysroot with `rust-analyzer-proc-macro-srv` installed. Run D3 §self-dogfood Cypher before and after 043-A; assert count increases by >= 1. PR body lists >= 3 concrete `:CallSite` qnames flipping `callee_resolved` false→true. `ci/determinism-check.sh --hir` exits 0 (§4 I1).
- Cross dogfood (graph-specs-rust @ pinned SHA `b542af3`): `ci/cross-dogfood.sh` exits 0 with post-RFC binary. No SchemaVersion bump (§4 I4–I5); descriptor change at `crates/cfdb-core/src/schema/describe/nodes.rs:280` is text-only, not in any arch-ban MATCH clause.
- Target dogfood (qbot-core @ pinned SHA): `cfdb scope --context trading` unwired count <= 767 (>= 50% reduction from pre-043 ceiling of 1534). PR body reports full table: pre-043, post-043 default, post-043 `--no-proc-macro`, per context. `--no-proc-macro` column must equal pre-043 numbers ± 5.

---

## D3. Dual-dogfood proof discipline

### 043-A self-dogfood

```cypher
MATCH (cs:CallSite)
WHERE cs.callee_resolved = true
  AND cs.file =~ '.*cfdb-hir-extractor.*'
RETURN count(cs) AS resolved_in_extractor_crate
```

Run before and after 043-A. Minimum bar: count increases by >= 1. Merge gate: >= 3 named production-code qnames in PR body. Rationale: `crates/cfdb-hir-extractor/src/call_site_emitter.rs` (20.1K) calls `Semantics<'db, RootDatabase>` methods whose receiver-type inference crosses salsa's `#[salsa::query_group]` macro; these should resolve post-043.

### 043-A cross-dogfood

Expected exit code 0 on all `.cfdb/queries/*.cypher` rules against `yg/graph-specs-rust` at SHA `b542af3`. RFC §4 I4 confirms no SchemaVersion bump; RFC-033 §4 I5 lockstep does not trigger. Exit 30 would be a true positive requiring companion-side cleanup.

### 043-A target-dogfood

| Context | Pre-043 unwired | Post-043 default | Post-043 `--no-proc-macro` | Delta |
|---|---:|---:|---:|---:|
| trading | 1534 | must be <= 767 | must be ~1534 ± 5 | >= 50% |
| infrastructure | 1086 | ? | must be ~1086 ± 5 | ? |
| (other contexts) | ... | ... | ... | ... |

Acceptance criterion: trading context <= 767. If any macro-heavy context shows < 30% reduction, RFC premise is rejected.

---

## D4. Determinism risk enumeration

D4 is rust-systems' lead domain. SOLID lens contribution: v2 has no orchestrator layer (CR1 was resolved by removing retry entirely, not adding an orchestrator). If a deny-list for non-deterministic macros is warranted, the correct architectural home is NOT `build_hir_database` — that adds a second responsibility (macro-policy enforcement) to a function whose charter is workspace loading. The correct path is a future RFC introducing a macro-filter layer. For 043-A, §4 I1 CI gate is the enforcement mechanism. Post-hoc detection keeps `build_hir_database` SRP-clean.

---

## D5. Wall-clock budget verdict

The 4x cap (RFC §3.4 / §4 I3) is correct. R1 D5 requested the cap be encoded as a CI assertion — v2 §4 I3 satisfies this: "CI gate (a `time` wrapper in determinism-check.sh) fails over budget." Invariant is now testable. The 4x cap on cfdb-self (~5s pre -> <=20s post) is conservative; RFC §3.4 expects 2–3x actual. No change request.

---

## D6. Failure-mode policy verdict

v2 §3.3 three-case policy is sound:

1. **Sysroot binary missing** — availability probe falls back to `None` + stderr warning. Named concrete consumer: CI on stock rustc (SYNTHESIS-R1 §RS-2). Satisfies the YAGNI bar.
2. **`load_workspace_at` returns `Err`** — hard fail via `HirError::LoadWorkspace`. `--no-proc-macro` is the recovery path.
3. **Lazy expansion failure** — `callee_resolved=false` per affected site; walk continues.

The YAGNI-cut features (`proc_macro_status` metadata, `--strict-proc-macro` flag) remain appropriately cut. No current keyspace consumer needs to distinguish extract modes at query time; §4 I6 descriptor caveat is the correct low-cost signal for the semantic-precision shift. No restoration warranted.
