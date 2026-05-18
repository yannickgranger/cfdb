# RFC-043 — RATIFIED 2026-05-18

**RFC:** `docs/RFC-043-hir-proc-macro-server.md`
**Originating issue:** [`yg/cfdb#398`](https://agency.lab:3000/yg/cfdb/issues/398)
**Convener:** a0 (session 2026-05-18, worktree `rfc/043-hir-proc-macro-server`)
**Team:** `rfc-043-hir-proc-macro-council` (`TeamCreate`, 4 lens teammates, CLAUDE.md §2b — agent team, not parallel sub-agents)

## Verdict: 4/4 RATIFY (R2)

| Lens | Sub-agent | R1 verdict | R1→R2 disposition | R2 verdict |
|---|---|---|---|---|
| Clean architecture | `clean-arch` | REQUEST CHANGES (4 findings) | All MOOT or RESOLVED by v2 trim | **RATIFY** |
| Domain-driven design | `ddd-specialist` | REQUEST CHANGES (3 findings: C1/C2/C3) | C1 ADDRESSED via §4 I6 descriptor caveat; C2 non-blocking; C3 MOOT (`proc_macro_status` cut) | **RATIFY** |
| SOLID + components | `solid-architect` | REQUEST CHANGES (CR1 SRP, CR2 OCP) | CR1 RESOLVED (fallback cut); CR2 MOOT (`ProcMacroPolicy` enum cut); 1 new minor CR (factor probe binding) applied | **RATIFY** |
| Rust systems | `rust-systems` | REQUEST CHANGES (RS-1 BLOCKING, RS-2 BLOCKING, RS-3/4/5 NON-BLOCKING) | RS-1 CLEARED (signature returns `Option<ProcMacroClient>`); RS-2 CLEARED (`proc_macro_server_available()` probe + §3.3 case 1); RS-3 documented in §3.6; 2 minor notes applied (Option type, 2× warning threshold) | **RATIFY** |

## Process timeline

- **v1 RFC drafted** by convener with 8 sections per CLAUDE.md §2.2.
- **R1 council spawned** via `TeamCreate` + 4 `Agent` teammates. R1 verdicts written to `council/RFC-043/v1-pre-trim-verdicts/`.
- **R1 in-flight:** user directive received — apply YAGNI + reuse pre-trim on the v1 RFC. Convener shut down in-flight R1 agents (3 had completed verdicts before shutdown was processed; rust-systems completed shortly after).
- **v2 RFC** trimmed: `ProcMacroPolicy` wrapper enum, `--strict-proc-macro` flag, `extract.proc_macro_status` keyspace metadata, `cfdb schema-describe` extension, retry-after-`Err` tolerant fallback, synthetic determinism fixture, 4-slice decomposition — all cut.
- **rust-systems R1 BLOCKING issues** RS-1 and RS-2 surfaced after the trim was applied. These were correctness concerns (lifetime + binary availability), not speculation — selectively restored a NARROW availability fallback (`proc_macro_server_available()` probe at startup only, not retry-after-`Err`) and updated `build_hir_database` return type to `(RootDatabase, Vfs, Option<ProcMacroClient>)`.
- **SYNTHESIS-R1.md** written documenting v1→v2 deltas and the principled basis for the trim.
- **R2 council spawned** with new lens-name suffix (`-r2`); ddd-specialist-r2 stuck in idle without writing → replaced by ddd-specialist-r2b single-shot.
- **R2 verdicts** all RATIFY. Three minor implementation notes applied to v2 RFC:
  - solid-architect CR1: factor `proc_macro_server_available()` into shared `pm_enabled` binding so `with_proc_macro_server` and `proc_macro_processes` cannot diverge in the probe-fires-false path.
  - rust-systems note: return type `Option<ProcMacroClient>` (upstream-faithful — `load_workspace_at`'s third element).
  - rust-systems note: tiered wall-clock evaluation (≤2× pass, ≤4× warn, >4× reject).
  - ddd-specialist implementer note: §4 I6 descriptor must mention silent-fallback indistinguishability with `--no-proc-macro`.

## Rework rounds (the council doing its job)

- **clean-arch R1**: caught the fallback-retry SRP boundary violation and the inaccurate `KeyspaceFile` reference. Both moot after trim.
- **ddd-specialist R1**: caught the `callee_resolved=true` homonym between pre/post-043 keyspaces (C1). v2 mitigation via §4 I6 descriptor caveat. C2 (phantom call-site filter) named as a code site to test rather than a new invariant.
- **solid-architect R1**: caught the SRP violation that the YAGNI trim was already going to address; R2 surfaced the probe-binding inconsistency CR1.
- **rust-systems R1**: caught TWO blocking systems-level issues (subprocess lifetime, sysroot binary availability) that the YAGNI trim would have shipped broken. RS-1 and RS-2 are the council's headline finding — they are correctness, not speculation, and the trim selectively restored what was load-bearing.

## Tests prescription (synthesized from 4-lens D2)

Slice 043-A (single vertical slice):

```
Tests:
  - Unit:
    (a) build_hir_database(workspace_root, proc_macros=false) returns Ok((db, vfs, None))
        with LoadCargoConfig{with_proc_macro_server: None, proc_macro_processes: 0}.
    (b) build_hir_database(workspace_root, proc_macros=true) on a sysroot with
        rust-analyzer-proc-macro-srv installed returns Ok((db, vfs, Some(client)))
        with LoadCargoConfig{with_proc_macro_server: Sysroot, proc_macro_processes: 1}.
    (c) build_hir_database(workspace_root, proc_macros=true) on a tmpdir-stubbed sysroot
        WITHOUT the binary returns Ok((db, vfs, None)) with
        LoadCargoConfig{with_proc_macro_server: None, proc_macro_processes: 0};
        stderr warning emitted naming the missing binary path. NO Err.
    (d) cfdb extract --no-proc-macro CLI flag parses round-trip; presence forces (a) path
        regardless of probe outcome.
    (e) VfsPath::Virtual filter assertion in call_site_emitter: synthetic test
        constructs a virtual-path file in the Vfs and verifies zero :CallSite nodes
        are emitted naming it.
    (f) The :CallSite.callee_resolved descriptor text includes the two §4 I6 sentences
        (epistemic precision shift + silent-fallback indistinguishability).
  - Self dogfood (cfdb on cfdb):
    `cfdb extract --workspace . --hir` on a sysroot with proc-macro-srv installed.
    Cypher: MATCH (c:CallSite) WHERE c.callee_resolved = true RETURN count(c) AS n.
    Assert post-043 n > pre-043 n by ≥ N (lower bound: 3 named flips listed in PR body
    via spot-check Cypher: MATCH (c:CallSite) WHERE c.callee_qname = $qname
    RETURN c.callee_resolved AS resolved). `ci/determinism-check.sh --hir` exits 0.
    Wall-clock ≤ 2× pre-RFC pass-with-note; ≤ 4× pre-RFC pass-with-warning
    (document dominant macro/crate); > 4× reject (§4 I3).
  - Cross dogfood (graph-specs-rust @ pinned SHA b542af3):
    `ci/cross-dogfood.sh` exits 0 with post-RFC binary. No SchemaVersion bump (§4 I4–I5);
    descriptor change at crates/cfdb-core/src/schema/describe/nodes.rs is text-only,
    not in any arch-ban MATCH clause.
  - Target dogfood (qbot-core @ pinned SHA):
    `cfdb extract --workspace /path/to/qbot-core --db .cfdb/db --keyspace qbot-core-043 --hir`
    then `cfdb enrich-reachability` then `cfdb scope --context trading --format json`.
    Acceptance: unwired count ≤ 767 (≥ 50% additional reduction from pre-043 1534 ceiling
    measured in #378). PR body reports the full table: pre-043, post-043 default,
    post-043 --no-proc-macro per context. The --no-proc-macro column MUST equal pre-043
    numbers ± 5 (regression guard on the escape hatch). If realized reduction < 30%,
    the RFC's premise (proc-macros are the dominant bottleneck) is rejected and a
    follow-up RFC files the next-tier improvement.
```

## Downstream

- **Lockstep:** no `yg/graph-specs-rust` `.cfdb/cross-fixture.toml` bump triggered (§4 I4–I5). RFC-043 does not bump `SchemaVersion`.
- **Implementation backlog:** one issue 043-A to be filed with `Refs: docs/RFC-043-hir-proc-macro-server.md` + verbatim Tests block above.
- **No 043-B / 043-C / 043-D:** the single-slice decomposition is by design (SYNTHESIS-R1.md §convener-pre-trim-pass). Empirical close-out lands in 043-A's PR body, not a separate slice.

## Closes / unblocks

- Closes (on 043-A merge): [`yg/cfdb#398`](https://agency.lab:3000/yg/cfdb/issues/398).
- Materially closes the 12.5%/12.8% `unwired` reduction ceiling measured in #378 (RFC-042's empirical close-out).

## Override clause

None used. 4/4 RATIFY achieved through one round of trim + one R2 round. No author-documented override per CLAUDE.md §2.3.
