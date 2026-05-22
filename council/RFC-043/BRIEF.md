# RFC-043 council brief — enable proc-macro server in `cfdb-hir-extractor`

**Status:** PENDING — convened 2026-05-18 against `docs/RFC-043-hir-proc-macro-server.md` (DRAFT in this worktree).

**RFC SHA on develop:** `ba0b7d7` (branch base, post-#416).

**Convener:** a0 (session 2026-05-18, worktree `.claude/worktrees/398-rfc-043`).

**Originating issue:** [`yg/cfdb#398`](https://agency.lab:3000/yg/cfdb/issues/398) — RFC stub: `cfdb-hir-extractor` disables proc-macro server.

**Empirical antecedent:** [`yg/cfdb#378`](https://agency.lab:3000/yg/cfdb/issues/378) 042-C close-out comment (2026-05-18) measures 12.5% `unwired` reduction from RFC-042 alone on `qbot-core` `--context trading`; the remaining 87% is the gap this RFC addresses.

**Reuse/YAGNI pre-trim done:** The RFC is intentionally minimal — single `bool` parameter, single CLI flag, hard fail (no tolerant fallback), cfdb-self as the determinism corpus (no synthetic fixture), one vertical slice. Earlier drafts proposed a `ProcMacroPolicy` wrapper enum, `--strict-proc-macro` companion flag, `extract.proc_macro_status` keyspace metadata, schema-describe extension, and tolerant-fallback retry logic — all cut because they shadow upstream `ra_ap_load_cargo::ProcMacroServerChoice` or solve hypothetical problems. Council MAY argue any cut needs to be restored, but the default is "ship what's drafted."

---

## 1. What you are ratifying

The RFC text is at `docs/RFC-043-hir-proc-macro-server.md` in this worktree (~210 lines, 8 sections). **Read it in full before rendering a verdict.** Council MUST cite RFC §section markers in evidence — generic "looks fine" is not a verdict.

Short summary:

1. `crates/cfdb-hir-extractor/src/hir_db.rs::build_hir_database` gains a `proc_macros: bool` parameter that selects `ProcMacroServerChoice::Sysroot` (`true`, default) or `::None` (`false`).
2. `cfdb extract` gains one CLI flag: `--no-proc-macro` (default `false`, i.e., proc-macros ON).
3. Failure mode: hard fail. `load_workspace_at` `Err` propagates through existing `HirError::LoadWorkspace`. Operator re-runs with `--no-proc-macro` if the macro path is broken.
4. `ci/determinism-check.sh` extended with a `--hir` extract pair against cfdb-self (G1 byte-stability on the macro path).
5. `cfdb-recall` baseline refreshed in the same PR.
6. No SchemaVersion bump (§4 I4). No graph-specs cross-fixture lockstep (§4 I5).
7. **One vertical slice** in §7: 043-A (flip + CLI escape + determinism extension + qbot-core empirical close-out in the same PR body).

---

## 2. Why this RFC matters operationally

RFC-042 closed the test/bench EntryPoint vocabulary gap. The 042-C empirical close-out (issue #378, comment 2026-05-18):

| Workspace · context | Pre-042 unwired | Post-042 default | Post-042 production-only | Reduction |
|---|---:|---:|---:|---:|
| qbot-core · trading | 1754 | 1534 | 1754 | 12.5% |
| qbot-infrastructure · infrastructure | 1246 (reported) | 1086 (reported) | — | 12.8% |

The ~13% ceiling is the receiver-type-resolution gap. Without this RFC, `unwired` stays at 87% false-positive on macro-heavy workspaces — every cleanup workflow has a 6:1 noise:signal ratio.

Downstream consumers: `/operate-module`, `/sweep-epic`, the §−1 resorbing-loop rule in qbot-core CLAUDE.md.

---

## 3. Council scope — what we are deciding

Per CLAUDE.md §2.3 each lens renders a verdict (`RATIFY` / `REQUEST CHANGES` / `REJECT`) with evidence, plus prescribes the `Tests:` 4-row block for the slice (`Unit`, `Self dogfood`, `Cross dogfood`, `Target dogfood`). Per CLAUDE.md §2.5 the `Cross dogfood` row exists because cfdb and graph-specs-rust are a paired toolchain.

### 3.1 Standard lens question (per the table in CLAUDE.md §2.3)

| Lens | Question | RFC §reference |
|---|---|---|
| `clean-arch` | Composition root contract for the new `bool` parameter + CLI flag; crate dependency direction unchanged? | §3.1, §3.2 |
| `ddd-specialist` | Vocabulary impact of macro-resolved `:CallSite{callee_resolved=true}`; the `callee_path` stability under `#[async_trait]`-style desugars (does the textual path match `syn`-visible name?) | §5.2 |
| `solid-architect` | SRP on `build_hir_database` post-`bool`-param; stable abstractions for `cfdb-hir-extractor`'s public surface | §5.3 |
| `rust-systems` | `proc_macro_processes = 1` lifetime + threading; sysroot availability of `rust-analyzer-proc-macro-srv`; determinism risk (which ecosystem macros depend on time/env/wd); whether the I1 CI gate is sufficient or a deny-list is needed | §5.4, §3.4, §3.5 |

### 3.2 Cross-cutting deliverables (every lens contributes)

**D1. Verdict** on the RFC as written. Cite RFC §sections. If `REQUEST CHANGES`, enumerate each change with proposed edit shape.

**D2. Tests prescription** for slice 043-A. 4-row block. Lens prescribes the row content most relevant to its perspective; other rows may be marked `(defer to <other-lens>)` and the convener synthesizes.

**D3. Dual-dogfood proof discipline.** For 043-A, articulate the EXACT shape of:
- `Self dogfood (cfdb on cfdb)` — what query result on cfdb's own keyspace proves the feature works. Concrete: name the Cypher, the expected lower-bound count, the rationale.
- `Cross dogfood (cfdb on graph-specs-rust at pinned SHA)` — confirm zero regression on existing `arch-ban-*.cypher` rules.
- `Target dogfood (qbot-core)` — the post-RFC `unwired` count on `--context trading`. Council MUST propose a lower-bound acceptance number (or argue why it should be left to PR-body reporting).

**D4. Determinism risk enumeration (`rust-systems` lead, all lenses contribute).** Which proc-macros in the cfdb / qbot-core dep closure are known to:
- Read `chrono::Utc::now()` at expansion time
- Read `std::env::var("BUILD_TIME")` / `SOURCE_DATE_EPOCH` / `CARGO_*` env vars
- Read file mtimes
- Pids, hostnames, randomness

Council MUST decide: is the §3.5 CI gate sufficient signal (gate fires post-hoc, operator fixes the non-deterministic macro), or is a deny-list needed in 043-A?

**D5. Wall-clock budget verdict.** RFC §3.4 names 4× cap on cfdb-self (~5s → ≤20s). Is 4× the right cap, or should it be 2× / 3×? If stricter and 043-A's prototype exceeds it, the RFC is rejected pending a tuning variant.

**D6. Failure-mode policy verdict.** RFC §3.3 explicitly REJECTS tolerant fallback / status metadata / schema-describe extension as YAGNI. Council MAY argue any of these need to come back, but must cite a concrete operational scenario where hard-fail + `--no-proc-macro` is insufficient. The bar for resurrecting a cut feature is "name the consumer that breaks without it."

### 3.3 Out of scope for council

- Re-litigating §6 non-goals. REJECT is only appropriate if the lens argues a non-goal is load-bearing for correctness.
- The single-slice decomposition. RFC §7.1 is a vertical slice (code + dogfood evidence in one PR); REJECT-on-shape requires evidence the slice cannot ship atomically.
- Whether to ship at all. The 13% ceiling is operational evidence; council debates shape, not necessity.

---

## 4. Reference material

- **RFC text:** `docs/RFC-043-hir-proc-macro-server.md` (this worktree).
- **Predecessor RFCs:**
  - RFC-029 (v0.2 :EntryPoint vocabulary distribution prediction).
  - RFC-042 (test/bench :EntryPoint kinds — empirical layer below this RFC).
- **Originating issues:**
  - [`#398`](https://agency.lab:3000/yg/cfdb/issues/398) — RFC-stub origin.
  - [`#378`](https://agency.lab:3000/yg/cfdb/issues/378) — 042-C close-out with empirical 12.5% ceiling.
- **Touch sites:**
  - `crates/cfdb-hir-extractor/src/hir_db.rs:40-48` — `LoadCargoConfig` declaration; the flag flip happens here.
  - `crates/cfdb-cli/src/extract.rs` (or equivalent) — `--no-proc-macro` flag definition.
  - `ci/determinism-check.sh` — extended with `--hir` extract pair.
- **Empirical antecedents:**
  - `agency:yg/cfdb` `.proofs/378-empirical.txt` — qbot-core trading 1534/1754 numbers.
  - `agency:yg/cfdb` issue #398 body — qbot-infrastructure 1246/1086 numbers.
- **Companion:** `yg/graph-specs-rust` `.cfdb/cross-fixture.toml` pins cfdb at SHA `b542af3` (V0_4_0). Pin NOT bumped by this RFC.
- **Methodology refs:** `CLAUDE.md` §1 (RFC-first), §2.3 (council), §2.5 (Tests template), §3 (dogfood enforcement).

---

## 5. Verdict format (write to `council/RFC-043/verdicts/<lens>.md`)

```markdown
# RFC-043 verdict — <lens-name>

**Verdict:** RATIFY | REQUEST CHANGES | REJECT
**Author:** <lens-name> sub-agent
**Date:** 2026-05-18

## D1. Verdict on the RFC as written

<2-4 paragraphs, citing RFC §sections. If REQUEST CHANGES, enumerate change requests with RFC §section + proposed edit shape.>

## D2. Tests prescription for slice 043-A

- Unit: <or `(defer to <lens>)`>
- Self dogfood: <or `(defer to <lens>)`>
- Cross dogfood: <or `(defer to <lens>)`>
- Target dogfood: <or `(defer to <lens>)`>

## D3. Dual-dogfood proof discipline

### 043-A self-dogfood
<concrete Cypher + expected count + rationale>

### 043-A cross-dogfood
<concrete regression check + expected exit code>

### 043-A target-dogfood
<concrete acceptance number / table shape>

## D4. Determinism risk enumeration

<known time/env/wd-dependent macros in the cfdb / qbot-core dep closure; deny-list candidacy verdict>

## D5. Wall-clock budget verdict

<is 4× the right cap on cfdb-self? Stricter? Empirical reasoning>

## D6. Failure-mode policy verdict

<is §3.3 hard-fail + --no-proc-macro the right policy? If any of the YAGNI-cut features need restoring, name the concrete consumer that breaks without them>
```

---

## 6. Convener notes

- The RFC is intentionally minimal. Reuse/YAGNI pre-trim cut the `ProcMacroPolicy` enum, `--strict-proc-macro` flag, `extract.proc_macro_status` metadata, `cfdb schema-describe` extension, tolerant fallback retry, synthetic determinism fixture, and multi-slice decomposition. Council pushback on these cuts is allowed BUT requires citing a concrete operational scenario (D6 specifies the bar).
- Slice 043-A is the empirical gate. The PR body reports the qbot-core `--context trading` post-RFC unwired count. Council prescribes the lower bound (D3 target-dogfood). The convener will reject the PR if the realized number is above the prescribed lower bound.
- The convener will collect verdicts, write SYNTHESIS-R1.md, apply REQUEST CHANGES edits to the RFC, and (if needed) convene R2. The pattern follows RFC-041 / RFC-042.
