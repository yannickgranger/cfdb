# RFC-043 verdict — clean-arch (Round 2)

**Verdict:** RATIFY
**Author:** clean-arch sub-agent
**Date:** 2026-05-18
**R1 verdict:** REQUEST CHANGES (4 findings)
**R2 disposition:** All R1 findings are moot or fully mitigated by the YAGNI trim and v2 design. No new concerns uncovered.

---

## D1. Verdict on the RFC as written

### R1 Finding 1 — `ProcMacroPolicy` enum placement (MOOT)

The wrapper enum was cut entirely. v2 uses `proc_macros: bool` directly. There is no type to misplace. The dependency rule is trivially satisfied: `cfdb-hir-extractor` imports `ra_ap_load_cargo::ProcMacroServerChoice` (outer adapter importing upstream infra — correct direction) and `cfdb-core` remains untouched. MOOT per SYNTHESIS-R1.md.

### R1 Finding 2 — Fallback retry logic should hoist to composition root (MOOT)

The retry-after-`Err` tolerant fallback was cut entirely. There is nothing to hoist. The v2 design (§3.1) has exactly one code path inside `build_hir_database`: probe availability, select `ProcMacroServerChoice`, call `load_workspace_at`, return `Ok((db, vfs, proc_macro_client))` or propagate `HirError::LoadWorkspace`. This is a single-shot adapter function — no retry, no orchestration. MOOT per SYNTHESIS-R1.md.

### R1 Finding 3 — `extract.proc_macro_status` placement clarification (MOOT)

The keyspace metadata field was cut entirely. There is nothing to place. MOOT per SYNTHESIS-R1.md.

### R1 Finding 4 — Composition root naming: use `hir.rs::extract_and_ingest_hir`, not `compose::load_hir_extractor` (RESOLVED)

RFC §3.2 now names `cfdb-cli`'s `extract` sub-command's stack frame as the owner of the returned `ProcMacroClient` and the `--no-proc-macro` flag as the CLI entry point. The v2 text does not reference a hypothetical `compose::load_hir_extractor`. Reading `compose.rs` (current file, 308 lines) confirms there is no HIR factory in `compose.rs` — the module handles only `PetgraphStore` construction as before. §3.2 correctly names the extract sub-command as the wiring site. RESOLVED.

### R1 Finding 5 — ~17 existing call-site migrations should be enumerated in §7.1 scope (REDUCED CONCERN)

The v2 signature change — `build_hir_database` now takes `proc_macros: bool` and returns `(RootDatabase, Vfs, ProcMacroClient)` — still breaks all existing call sites. RFC §7.1 v2 describes the caller contract ("caller owns the returned `ProcMacroClient` for the lifetime of the extraction walk") but does not enumerate the ~17 existing test call sites from R1's Finding 5. The implementer must update them.

However, the correct prescription for existing test call sites is now simpler than in v1: pass `proc_macros: false` (there is no enum variant to construct). The return tuple grows by one element; test call sites can destructure as `let (db, vfs, _client) = build_hir_database(root, false)?;` when they do not need proc-macro resolution. The R1 Finding 5 concern stands as an implementation note, but it is NOT a blocker for ratification — the implementer is expected to compile the crate and fix breakage. The scope description is complete enough.

**This is a minor implementation-completeness gap, not an architectural violation.** R2 verdict does not block on it.

### New concern: availability probe inside `build_hir_database` — does it violate the "pure adapter" framing?

RFC §3.1 and §3.3 place `proc_macro_server_available()` INSIDE `build_hir_database`. My R1 verdict argued that orchestration concerns (fallback policy) should hoist to the composition root. Does this narrower availability probe belong inside the adapter or at the CLI layer?

The clean-arch analysis: `proc_macro_server_available()` is a **startup-time capability check**, not a recovery-time orchestration loop. It answers "can this adapter do what the caller requested?" before attempting the operation — analogous to checking whether a TCP port is open before creating a connection. This is legitimately part of the adapter's initialization contract. The Dependency Rule is not violated: the probe touches the sysroot filesystem (infrastructure), which is already within scope for an adapter function that loads a workspace. The probe does NOT introduce retry, does NOT produce side effects that survive the function call, and does NOT tag keyspace metadata — all three of which were the substance of R1's Finding 2.

RFC §3.3 case 1 correctly describes the probe as running "before `load_workspace_at`" — making it a pre-condition gate rather than a post-failure recovery path. This is the right architectural framing: the adapter checks availability at entry and selects its configuration accordingly. The caller (`cfdb-cli`'s extract sub-command, §3.2) does not need to know about sysroot availability; it passes `proc_macros: bool` and gets back a fully-configured `(RootDatabase, Vfs, ProcMacroClient)` or an error. The abstraction boundary is clean.

**No violation. The availability probe is correctly placed inside the adapter.**

### New concern: `ProcMacroClient` lifetime — does I7 leak composition-root concerns into `cfdb-hir-extractor`?

RFC §3.1 + §4 I7 require `build_hir_database` to return a three-element tuple `(RootDatabase, Vfs, ProcMacroClient)`. The caller (CLI stack frame) owns the `ProcMacroClient` and must hold it for the duration of the VFS walk. Does this constitute a composition-root concern bleeding into the extractor?

No. The `ProcMacroClient` is a handle for a resource the extractor created (the subprocess). Returning it to the caller is the correct ownership model — the function that allocates a resource should not silently drop it (which pre-RFC was a latent bug: `_proc_macro_client` at `hir_db.rs:50` was silently discarding the handle). Returning the handle to the caller is standard Rust RAII discipline, not orchestration logic. The caller's responsibility is to hold the handle alive, not to know its internal structure. This is analogous to returning a `File` handle to the caller rather than closing it inside the constructor.

The three-element return tuple IS slightly more mechanical (callers must destructure three values), but it is the honest API shape given the upstream `load_workspace_at` contract. The RFC is correct not to introduce a wrapper struct here — that would be the YAGNI violation the trim was designed to prevent.

**No violation. "Caller owns the client tuple element" is the correct boundary.**

### Dependency direction summary

The v2 design preserves clean dependency direction throughout:

- `cfdb-cli` (entry point) → `cfdb-hir-extractor` (adapter): correct
- `cfdb-hir-extractor` → `ra_ap_load_cargo` (upstream infra): correct, expected for an adapter
- `cfdb-core` (domain): unchanged, no new imports from outer layers
- `compose.rs` (composition root for store wiring): unchanged — the RFC correctly does NOT add HIR wiring to `compose.rs`, keeping HIR wiring in the extract sub-command's stack frame where it already lives

No inner-to-outer violations introduced by v2.

---

## D2. Tests prescription for slice 043-A

```
Tests:
  - Unit: `build_hir_database` compiles with `proc_macros: true` and `proc_macros: false`.
    ProcMacroClient is returned (not discarded) — assert the third tuple element is accessible at call site.
    `proc_macro_server_available()` returns false on a tmpdir-stubbed sysroot lacking the binary (unit test with a temp $SYSROOT).
    `--no-proc-macro` CLI flag parses to `proc_macros: false`; absence of the flag parses to `proc_macros: true`. Round-trip.
    Existing test call sites (cfdb-hir-extractor/tests/*.rs) compile with `proc_macros: false` — no sysroot dependency introduced in existing test suite.

  - Self dogfood (cfdb on cfdb): `cfdb extract --workspace . --hir` on a sysroot WITH rust-analyzer-proc-macro-srv produces
    a keyspace where the count of CallSite{callee_resolved=true} is STRICTLY GREATER than the same extract with
    `--no-proc-macro`. At least 3 named (file, line, callee_qname) triples that flipped from false to true are listed in
    the PR body. `ci/determinism-check.sh` --hir mode exits 0 (G1 byte-stability holds on the macro path, §4 I1).
    cfdb-recall baseline numbers refreshed and must not regress (§4 I2). Wall-clock measurement vs pre-RFC baseline
    in PR body; must be ≤ 4× (§4 I3).

  - Cross dogfood (graph-specs-rust @ pinned SHA): `ci/cross-dogfood.sh` exits 0 with post-RFC binary against the
    current pinned graph-specs SHA (b542af3). No SchemaVersion bump (§4 I4) means no cross-fixture lockstep is
    triggered. Exit-code contract: 0 = no findings. Exit 30 = new finding on companion tree = merge blocked.

  - Target dogfood (qbot-core @ pinned SHA): `cfdb scope --context trading` unwired count with proc-macros on must be
    materially below 1534 (the pre-RFC-043 ceiling). Council-prescribed acceptance lower bound: report actual number
    in PR body; reviewer blocks merge if count exceeds 1300 (same threshold endorsed by clean-arch R1 D2). The
    `--no-proc-macro` run should reproduce the pre-RFC baseline (1534 ± noise) to confirm the flag works end-to-end.
```

---

## D3. Dual-dogfood proof discipline

### 043-A self-dogfood

Two Cypher queries against cfdb-self keyspace, both required in PR body:

**Query 1 — aggregate recall delta:**
```cypher
MATCH (cs:CallSite)
WHERE cs.resolver = 'hir' AND cs.callee_resolved = true
RETURN count(cs) AS resolved_count
```

Run against: (a) keyspace extracted with `--no-proc-macro`, (b) keyspace extracted without that flag. Assert `(b).resolved_count > (a).resolved_count`. The delta is the headline cfdb-self recall improvement.

**Query 2 — named flip witnesses (at least 3 required):**
```cypher
MATCH (cs:CallSite)
WHERE cs.resolver = 'hir'
  AND cs.callee_resolved = true
  AND cs.callee_qname IS NOT NULL
  AND cs.file STARTS WITH 'crates/cfdb-'
RETURN cs.file, cs.line, cs.callee_qname
ORDER BY cs.file, cs.line
LIMIT 20
```

The PR body must list at least 3 `(file, line, callee_qname)` triples present in query (b) that were absent or `callee_resolved=false` in keyspace (a). A count-only assertion is insufficient — the named triples are the proof that the right call sites flipped, not noise.

### 043-A cross-dogfood

`ci/cross-dogfood.sh` against `yg/graph-specs-rust` at current pinned SHA (`b542af3`). Expected exit code: 0. RFC §4 I4 (no SchemaVersion bump) + §4 I5 (no cross-fixture pin bump) guarantee that no schema contract is broken. If exit 30 fires, the cause is a false-positive in an existing ban rule against newly-resolved call sites — investigate the specific rule before declaring cross-dogfood failure.

### 043-A target-dogfood

Acceptance table in PR body:

| Context | Pre-RFC-043 unwired | Post-RFC-043 (proc-macros on) | Post-RFC-043 (--no-proc-macro) |
|---|---:|---:|---:|
| trading | 1534 | TBD | must ≈ 1534 |

Acceptance criterion: `trading` post-RFC count < 1300. If the `--no-proc-macro` column does not reproduce ~1534, the flag wiring is broken — merge blocked regardless of the primary number.

Regression guard: if any context shows `unwired` INCREASING post-RFC-043, this is a precision regression (phantom CALLS edges from macro expansion). Investigate before the PR is reviewed.

---

## D4. Determinism risk enumeration

Defer to rust-systems lens for the enumeration of specific macro crates in the cfdb/qbot-core dep closure that read time/env/pid at expansion time. The clean-arch position (from R1, unchanged in R2):

The §3.5 determinism gate (`ci/determinism-check.sh` `--hir` mode) is the correct primary mechanism. A deny-list inside `cfdb-hir-extractor` would couple the adapter to ecosystem-specific knowledge (third-party crate names), violating the stable-abstractions principle. The post-hoc CI gate is architecturally cleaner and catches all non-determinism regardless of source.

The §3.6 documented `proc_macro_cwd` risk (absolute path injection via `ra_ap_load_cargo`) is a future-RFC concern. No mitigation in 043-A is the correct call — the same-workspace determinism check is the gate; cross-workspace divergence is a future scenario that would need a concrete consumer to motivate a fix.

---

## D5. Wall-clock budget verdict

The 4× cap on cfdb-self (§4 I3: ≤ 4× pre-RFC wall-clock, i.e., ≤ ~20 s) is acceptable. The clean-arch lens does not impose a stricter cap. Wall-clock is an operational concern; the `--no-proc-macro` escape hatch gives operators control at the CLI layer without requiring the architecture to enforce timing guarantees. The RFC correctly notes that wall-clock budget enforcement for the proc-macros path is the operator's responsibility via `--no-proc-macro`, not enforced by the code.

---

## D6. Failure-mode policy verdict

The v2 failure-mode policy (§3.3) is clean and sufficient:

- **Case 1** (sysroot binary missing): availability probe → silent API fallback, loud stderr warning. This is the correct CI path. The R1 concern (tolerant fallback inside the adapter) no longer applies because v2's availability probe is a pre-condition check, not a recovery loop.
- **Case 2** (`load_workspace_at` errors after probe passed): hard fail via `HirError::LoadWorkspace`. Operator escapes with `--no-proc-macro`. This is the correct default for a data-pipeline command.
- **Case 3** (lazy expansion failure during VFS walk): continue, emit `callee_resolved=false`. Operator inspects the end-of-extract ratio. Correct — do not abort the walk on individual call-site failures.

None of the YAGNI-cut features (`ProcMacroPolicy` enum, `extract.proc_macro_status` metadata, `cfdb schema-describe` extension, `--strict-proc-macro`, retry-after-`Err`) need to be restored. The bar is "name the concrete consumer that breaks without it." No such consumer has been named for any of these features. The RS-2 consumer (CI on stock rustc images) is already addressed by the availability probe — the one tolerant-fallback mode that was explicitly justified.

The ddd-specialist's I6 mitigation (descriptor caveat in `crates/cfdb-core/src/schema/describe/nodes.rs`) is the correct minimum consumer signal for the `callee_resolved` semantics shift. Keyspace-level metadata is not needed for RFC-043's scope.

---

## Summary

All four R1 change requests are either MOOT (v1 features cut entirely: findings 2, 3) or RESOLVED (finding 4: composition root naming). Finding 5 (call-site migration scope) is a minor implementation-completeness note that does not warrant blocking ratification — the implementer will encounter and fix the compile errors. No new architectural violations are introduced by v2's availability probe (a pre-condition check, not an orchestration loop) or the three-element return tuple (standard RAII, not a composition-root leak).

**The v2 RFC is RATIFIED by the clean-arch lens.**
