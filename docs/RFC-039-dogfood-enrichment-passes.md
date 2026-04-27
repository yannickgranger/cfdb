# RFC-039 — dogfood the 7 enrichment passes

Status: **Ratified (R2, 2026-04-27) — 4/4 RATIFY: clean-arch, ddd-specialist, solid-architect, rust-systems.**
Parent EPIC: #338 (dogfood hardening — query smoke + enrichment invariants + recall enforcement).
Companion issues filed pre-RFC: #339 (Phase A — query liveness smoke), #340 (Phase C — recall as Gitea status).

R1 council outcome: 4× REQUEST CHANGES (17 distinct change requests across 4 lenses). All applied in R2.
R2 council outcome: **4× RATIFY**. Verdicts captured inline in §5.

---

## 1. Problem

The 7 enrichment passes shipped under #43 (slices 43-B…43-G) and RFC-036 §3.3 (issue #203) all have real implementations on `PetgraphStore` (`crates/cfdb-petgraph/src/enrich/{git_history,rfc_docs,bounded_context,concepts,reachability,metrics}.rs`, plus `enrich_deprecation` populated extractor-side per slice 43-C). None of them have a CI assertion of the postcondition each pass exists to maintain.

Concretely: a pass that silently regresses — e.g. a refactor that breaks `enrich_concepts::run`'s TOML loader so it walks no files and emits zero `:Concept` nodes — would report `EnrichReport { ran: true, attrs_written: 0, edges_written: 0, warnings: [] }`. That report is **indistinguishable** from a genuine no-op (the TOML directory was empty). Today's CI accepts both as a pass.

The drift cost is asymmetric:
- The pass writer knows the contract and writes tests against it.
- The pass user (a downstream skill: `/operate-module`, `/audit-all`, `/boy-scout --from-inventory`, the qbot-core consumer of `cfdb scope`) discovers the regression when their own gate flakes — long after the offending PR merged.

The 2026-04-25 #43 epic closed on "implementation lands"; this RFC closes the residual "implementation provably lands what it claims" gap.

The cfdb / graph-specs duo (RFC-033) makes this gap costlier than usual: graph-specs vendors cfdb as a pinned git dep and consumes its enrichment output. A silent enrichment regression becomes a silent graph-specs verdict regression on every downstream PR.

---

## 2. Scope

### Deliverables

1. **Seven new ban-rule-form Cypher files** under `.cfdb/queries/self-enrich-<pass>.cypher`, one per pass. The `self-` prefix mirrors the project's existing "self-dogfood" / "self-hosted ban rules" vocabulary (per ddd-specialist Q4) and distinguishes these framework regression tests from `arch-ban-*` user-code policy rules. Each file's row set is the violation set: zero rows = pass invariant holds, ≥1 row = violation.

2. **Seven thresholds as `const` in `tools/dogfood-enrich/`** — the percentage cutoffs (95% bounded-context coverage, 80% reachability, etc.) live as named consts in the leaf binary's source per `CLAUDE.md` §6 row 5. **No baseline file. No allowlist.** (Per solid-architect SAP analysis: `tools/dogfood-enrich/` is a leaf crate with Ca=0; placing consts there is SAP-compliant, vs. placing them in `cfdb-cli` which is highly efferent.)

3. **CI step extension in `.gitea/workflows/ci.yml`** — after the existing self-audit step, runs the 4 default-feature self-enrich dogfoods on cfdb-self via `tools/dogfood-enrich`. The 3 feature-gated dogfoods route to a parallel nightly job (`.gitea/workflows/enrich-dogfood-nightly.yml`, new file) that posts `enrich-dogfood/<pass>` Gitea commit statuses (mirroring the recall pattern in issue #340).

4. **New determinism script `ci/dogfood-determinism.sh`** — covers the 7 self-enrich queries. Per rust-systems: extending `ci/predicate-determinism.sh` is structurally impossible because `cfdb check-predicate` and `cfdb violations` are different subcommands with different param schemas. A dedicated script is cleaner and amortizes one extract over all 7 queries.

5. **README.md "Dogfood enforcement" row** — one new row in the §3-style table naming "Enrichment-pass postconditions" as a CI gate, citing this RFC and the per-pass query files.

### Non-deliverables

- **No `SchemaVersion` bump.** These queries read existing schema only.
- **No new fact type, node label, edge label, or attribute.**
- **No new CLI verb or `--flag` on `cfdb`.** Specifically: NO addition of `--param` to `cfdb violations` (rust-systems verified the flag is absent at `crates/cfdb-cli/src/main_command/args.rs:378-402`; threshold substitution happens inside `tools/dogfood-enrich` before subprocess invocation).
- **No cross-dogfood enrichment** (running enrich-* against graph-specs / qbot-core). Out of scope; potential follow-up RFC if the per-tree enrichment cost becomes worth paying.
- **No tightening of any individual threshold.** Initial values are the floor at which cfdb-self currently passes; subsequent tightening is a separate reviewed PR per `CLAUDE.md` §6.
- **No coverage of recall** (issue #340) or query liveness smoke (issue #339). Those are sibling phases under EPIC #338.

---

## 3. Design

### 3.1 The 7 invariants

| Pass | Invariant | Cypher shape (rows = violations) | Default vs nightly |
|---|---|---|---|
| `enrich-deprecation` | Every `#[deprecated]` symbol grep'd from source has `:Item.is_deprecated = true` | `MATCH (i:Item) WHERE i.qname IN $deprecated_qnames AND i.is_deprecated = false RETURN i.qname` | Default |
| `enrich-rfc-docs` | (a) `count(:RfcDoc) >= count(docs/RFC-*.md)` AND (b) `count(:Item)-[:REFERENCED_BY]->(:RfcDoc) > 0` | Two-row sentinel pattern: row 1 if `count(:RfcDoc)` < ground truth; row 2 if zero edges | Default |
| `enrich-bounded-context` | **Combined-pipeline invariant** (see §3.1.1): ≥`MIN_BC_COVERAGE_PCT` (initial: 95) of `:Item` have non-null `bounded_context` after extract+enrich. Denominator: `count(:Item)`. | Sentinel on `nulls(:Item.bounded_context) / count(:Item)` ratio | Default |
| `enrich-concepts` | (a) `count(:Concept) == count(distinct context names across all .cfdb/concepts/*.toml)` AND (b) `count(:LABELED_AS) > 0` AND (c) **conditional**: IF `$declared_canonical_crate_count > 0` THEN `count(:CANONICAL_FOR) > 0`. (See §3.1.2.) | Three-sentinel pattern, one row per failing condition; sentinel (c) bound by `$declared_canonical_crate_count` parameter | Default |
| `enrich-reachability` | ≥`MIN_REACHABILITY_PCT` (initial: 80) of `:Item{kind:Fn}` reachable from any `:EntryPoint`. Denominator: `count(:Item{kind:Fn})`. | Sentinel on `unreachable(:Item{kind:Fn}) / count(:Item{kind:Fn})` ratio | Nightly (requires `hir` feature) |
| `enrich-metrics` | ≥`MIN_METRICS_COVERAGE_PCT` (initial: 95) of `:Item{kind:Fn}` have non-null `cyclomatic` AND `unwrap_count`. Denominator: `count(:Item{kind:Fn})`. | Sentinel on `nulls(:Item{kind:Fn}.cyclomatic) / count(:Item{kind:Fn})` ratio | Nightly (requires `quality-metrics` feature) |
| `enrich-git-history` | ≥`MIN_GIT_COVERAGE_PCT` (initial: 95) of `:Item` have non-null **`git_last_commit_unix_ts`** (the actual emitted attribute per `crates/cfdb-petgraph/src/enrich/git_history.rs:51`, NOT the originally-cited `commit_age_days` which does not exist). Denominator: `count(:Item)`. | Sentinel on `nulls(:Item.git_last_commit_unix_ts) / count(:Item)` ratio | Nightly (requires `git-enrich` feature) |

**Sentinel pattern.** Cypher does not natively support `IF count > threshold THEN return-row`; the four ratio-based queries express the threshold as a `WITH total, nulls WHERE nulls * 100 > total * (100 - $threshold)` clause that returns one row when the bound is violated and zero rows when it holds. The threshold value is bound as a `$parameter` from the harness (see §3.5 — the `tools/dogfood-enrich` binary string-substitutes the threshold const into the Cypher template before submitting via the `cfdb-petgraph` query executor; if the executor supports named parameter binding on `violations`-style queries, that is preferred and the substitution is at parameter-binding time rather than string-templating).

**Note on "ratio" as homonym (per ddd-specialist):** the four ratio-based invariants use **different denominators** — `enrich-bounded-context` and `enrich-git-history` use `count(:Item)` (all kinds); `enrich-reachability` and `enrich-metrics` use `count(:Item{kind:Fn})` (functions only). Each sentinel above carries its explicit fraction in the table to avoid the homonym.

#### 3.1.1 enrich-bounded-context: combined-pipeline scope (per ddd-specialist)

The `enrich_bounded_context` pass is a **delta-patch**, not a producer (`crates/cfdb-petgraph/src/enrich/bounded_context.rs:12-20` — at fresh-extract time it is a no-op because the extractor already populated `:Item.bounded_context`). The 95% coverage invariant therefore measures the **combined extract+enrich pipeline state**, not the enrich pass's delta. A pass that patches zero items on a fresh extract is correct; this sentinel catches the case where the overall pipeline's coverage falls below the floor.

The pass's own correctness postcondition (items whose stored context diverged from current TOML+heuristic resolution were patched) is not directly testable as a row-count sentinel — the right test surface for that is unit tests on the pass itself, out of scope for this RFC.

#### 3.1.2 enrich-concepts: invariant corrections (per ddd-specialist)

The R1 draft had two factual errors in the `enrich-concepts` invariants:

1. **Ground truth was "TOML file count" or "[concepts] entry count".** Both are wrong. `cfdb_concepts::declared_contexts` (`crates/cfdb-concepts/src/lib.rs:112-118`) deduplicates by context **name** — one TOML file with `name = "cfdb"` produces exactly one `:Concept` regardless of how many `crates = [...]` it carries. The corrected ground truth is `count(distinct context names across all .cfdb/concepts/*.toml)`.

2. **`count(:CANONICAL_FOR) > 0` was unconditional.** `ContextMeta.canonical_crate` is `Option<String>` (`crates/cfdb-concepts/src/lib.rs:65`); a workspace with zero `canonical_crate` declarations legitimately emits zero `:CANONICAL_FOR` edges. Corrected: the sentinel fires only when `$declared_canonical_crate_count > 0` (the harness counts declared `canonical_crate` values from the TOML scan and binds the count as a query parameter before executing the dogfood query).

### 3.2 Threshold const location

Per `CLAUDE.md` §6 row 5 ("No metric ratchets") and project `CLAUDE.md` §3 (no baseline files), threshold values live as `const` in tool source.

**Resolution (Q1):** consts live in **`tools/dogfood-enrich/`** (the leaf harness binary).

Rationale (synthesized from clean-arch + solid-architect verdicts):

- **`cfdb-core` is wrong** (clean-arch): the inner ring has zero infrastructure deps; CI-gate policy is application-layer concern. Injecting dogfood thresholds there contaminates the dependency direction.
- **`cfdb-cli` is wrong** (solid-architect): `cfdb-cli` is highly efferent (Ce >> Ca: pulls cfdb-core, cfdb-petgraph, cfdb-query, cfdb-extractor, cfdb-concepts). Its instability metric I is near 1.0. Placing CI policy there violates CCP — CI-only policy changes would force rebuilds of the user-facing binary for unrelated reasons.
- **`tools/dogfood-enrich/` is correct** (solid-architect): a leaf crate (Ca=0, Ce=minimal — `cfdb-core` consts + standard lib + `clap`/`serde`). Stability is a non-issue because nothing depends on it. The CI YAML calls `cargo build -p dogfood-enrich` once and invokes the binary directly. SAP-compliant.

### 3.3 CI matrix decision

PR-time job (default features only) runs the 4 default-feature self-enrich dogfoods:
- `enrich-deprecation` (extractor-time, no feature flag)
- `enrich-rfc-docs` (FS scan, no flag)
- `enrich-bounded-context` (TOML overlay, no flag)
- `enrich-concepts` (TOML materialization, no flag)

Parallel nightly job (new file: `.gitea/workflows/enrich-dogfood-nightly.yml`) builds `cfdb-cli` AND `tools/dogfood-enrich` with `--features hir,git-enrich,quality-metrics` (single combined build per rust-systems compile-cost analysis: the 31 `ra_ap_*` crates dominate and are shared across features; one build is cheaper than three per-feature jobs and shares sccache state with PR-time CI). Runs the remaining 3:
- `enrich-reachability` (`hir` feature — confirmed canonical name at `crates/cfdb-cli/Cargo.toml:52`)
- `enrich-metrics` (`quality-metrics`)
- `enrich-git-history` (`git-enrich`)

**sccache wiring (per rust-systems).** The new nightly workflow's Setup step copies `.gitea/workflows/ci.yml:61-71` verbatim — installing sccache from `/cache/cargo/bin/sccache`, setting `SCCACHE_REDIS_ENDPOINT=redis://192.168.1.107:6380`, `RUSTC_WRAPPER`, and key prefix `cfdb-1.93`. With shared Redis state, the combined `--features hir,git-enrich,quality-metrics` build is warm after the first run.

Nightly posts Gitea commit statuses (`enrich-dogfood/reachability`, `enrich-dogfood/metrics`, `enrich-dogfood/git-history`) on develop HEAD per the same convention as #340. **Soft-warning first cycle**: status posts but is not in the required-checks set. Second cycle onward, statuses gate merge.

### 3.4 Smoke vs invariant separation

The Phase A query smoke test (issue #339) asserts query **liveness** — every `.cypher` file parses + executes against cfdb-self regardless of row count. RFC-039 asserts **invariants** — specific row-count bounds.

The two run as **separate CI steps**: smoke first (cheap, no enrich-* runs needed), invariants second (requires the enrich pass to mutate the keyspace before the dogfood query reads it). A failure in smoke does NOT mask a failure in invariants — both surface independently in CI.

The smoke step skips files matching `.cfdb/queries/self-enrich-*.cypher` (they require parameter binding from the runner harness; the smoke step has no harness). This skip is documented inline in the smoke step per #339 AC-6.

**Determinism.** A new `ci/dogfood-determinism.sh` (per rust-systems) covers the 7 self-enrich queries. The script: (a) extracts cfdb-self into a single tmpdir keyspace, (b) runs all 7 enrich passes against it, (c) runs each `cfdb violations --rule .cfdb/queries/self-enrich-*.cypher` (driven by `tools/dogfood-enrich` so threshold params are bound) twice, (d) diffs stdout. One combined extract feeds all 7 queries — cheaper than the per-predicate pattern in `predicate-determinism.sh`. **This is not an extension of `predicate-determinism.sh`** — that script invokes `cfdb check-predicate` (different subcommand, different param schema); a shared script would require a conditional code path. Keep them separate.

### 3.5 Runner harness shape — Option α (RATIFIED via R1 council)

**Resolution (Q2):** Option α — new binary at `tools/dogfood-enrich/`. Rationale (synthesized from clean-arch + solid-architect):

- **Option γ is broken** (rust-systems + solid-architect): the R1 draft proposed `cfdb violations --rule … --param threshold:literal:<N>`. The `--param` flag is verified absent on `Command::Violations` at `crates/cfdb-cli/src/main_command/args.rs:378-402`. Adding `--param` to `cfdb violations` is scope creep. γ is rejected.
- **Option β fails CCP** (solid-architect): a `cfdb dogfood-enrich` subcommand puts CI-only policy thresholds in the same binary as user-facing verbs (`extract`, `scope`, `violations`). Those change for different reasons (schema evolution, query syntax, vs. CI tightening). Adding to `Command` enum at `args.rs:19-490` couples CI-only changes to user-facing binary rebuilds.
- **Option α resolves both objections** (with subprocess design that satisfies clean-arch's Dependency Rule concern): `tools/dogfood-enrich/` is a standalone binary that follows the established `tools/check-prelude-triggers/` pattern — `Ca=0`, `Ce=minimal` (`clap`, `serde`, `cfdb-core` for shared types only). It does NOT link `cfdb-cli` as a library. It invokes `cfdb` as a **subprocess** (`./target/release/cfdb violations --rule <materialized-tempfile.cypher>`), reading the exit code. This subprocess relationship is the same one `ci/cross-dogfood.sh` already uses — no library dependency, no internal-API leak, no dependency-direction reversal.

#### 3.5.1 tools/dogfood-enrich design

```
tools/dogfood-enrich/
├── Cargo.toml          # [package] name = "dogfood-enrich", deps: clap, serde, cfdb-core
└── src/
    ├── main.rs         # clap dispatch; subprocess to ./target/release/cfdb
    ├── thresholds.rs   # pub const MIN_BC_COVERAGE_PCT: u32 = 95; etc.
    ├── passes.rs       # struct PassDef { name, query_template_path, threshold_const, feature_required }
    └── runner.rs       # template substitution + subprocess invocation + exit-code policy
```

**Invocation contract:**
```
dogfood-enrich --pass <name> --db <dir> --keyspace <ks>
  → exit 0  if zero violation rows
  → exit 30 if violations
  → exit  1 on runtime error (subprocess fail, missing file, parse error)
```

**Threshold substitution.** `runner.rs` reads the cypher template, performs string substitution (`{{ threshold }}` → `MIN_BC_COVERAGE_PCT`), writes to a tempfile, invokes `cfdb violations --rule <tempfile>`, captures exit code. **No `--param` extension to `cfdb` is required.** When `cfdb-petgraph`'s query executor adds support for named parameter binding on violations-style queries (out of scope for this RFC), a future PR may switch from string templating to bound parameters; the harness shape stays the same.

---

## 4. Invariants

**I1 — Determinism.** Each `self-enrich-*.cypher` produces byte-identical row sets across two consecutive runs on the same keyspace. Enforced by **a new `ci/dogfood-determinism.sh`** (not an extension of `predicate-determinism.sh` — see §3.4).

**I2 — Recall (ground-truth alignment).** For passes whose invariant compares against an external ground truth (e.g. `enrich-deprecation` greps `#[deprecated]` from the workspace; `enrich-concepts` counts distinct TOML names), the ground truth and cfdb's extracted view are computed from the **same** workspace tree, in the **same** CI step, with **same** filesystem state. No tree drift.

**I3 — No-ratchet.** Threshold values are `const` declarations in `tools/dogfood-enrich/src/thresholds.rs`. Tightening is a separate reviewed PR. **A PR that adds a `.dogfood-baseline.toml`, `.dogfood-allowlist.toml`, or any file whose purpose is to record "current violation count = N, fail if > N" is rejected on sight.**

**I3.1 — Permanent-null tracking (per ddd-specialist).** A crate whose `bounded_context` is permanently null after both extract and enrich phases (heuristic cannot resolve, no TOML override) **must be tracked in a filed issue**; the threshold may not silently absorb it. The 95% coverage floor is a pragmatic CI-pass minimum, not an "OK to leave 5% broken" license.

**I4 — Keyspace backward-compat.** No `SchemaVersion` bump. The 7 queries read existing schema only.

**I5 — Feature-flag isolation.** Default builds dogfood the 4 default-feature passes. Feature-gated dogfood requires the matching feature flag — the nightly job builds `cfdb-cli` + `tools/dogfood-enrich` with all three flags enabled and dogfoods all 7 (default + gated). PR-time runs only the 4 default-feature dogfoods; nightly catches feature-gated regressions.

**I5.1 — Feature-presence guard (per rust-systems).** The nightly harness MUST verify the binary was built with the feature before asserting its dogfood query. Concretely: `tools/dogfood-enrich` first invokes `cfdb enrich-<pass>` and checks `EnrichReport.ran == true` in the JSON output; if `ran == false` (feature absent at build time, dispatched to the off-feature path returning `ran:false` at `crates/cfdb-petgraph/src/enrich_backend.rs:178-262`), the harness exits non-zero with a clear "feature missing" message **before** running the null-ratio sentinel (which would otherwise silently report 100% null coverage and look like a real regression).

**I6 — Smoke independence.** Failure in the Phase A smoke step does not skip the RFC-039 invariant step; CI runs both unconditionally and surfaces both failures.

---

## 5. Architect lenses — verdicts

R1 council (2026-04-27) returned 4× REQUEST CHANGES (17 distinct change requests across 4 lenses).
R2 council (2026-04-27) returned **4× RATIFY**. RFC is ratified.

### 5.1 Clean architecture (`clean-arch`) — R1: REQUEST CHANGES

**Verdict R1:** REQUEST CHANGES. Three changes requested:
1. Reject Option γ (uncontracted shell↔Rust seam via `emit-dogfood-thresholds`); prefer **Option β** on Dependency Rule grounds.
2. Reject `cfdb-core` for threshold consts (would inject CI knowledge into the inner ring); confirm `cfdb-cli`.
3. Confirm `.cfdb/queries/` directory; reject `.cfdb/dogfood/` split.

**R2 disposition:**
- (1) **Partially adopted, partially superseded.** clean-arch's rejection of γ is adopted (γ is removed). However, solid-architect's CCP analysis showed Option β puts CI-only policy in the user-facing `cfdb` binary, coupling unrelated change-reasons. The compromise is **Option α with subprocess-only invocation** (§3.5): standalone binary in `tools/dogfood-enrich/` that does NOT link `cfdb-cli` (no Dependency Rule reversal) and does NOT pollute `cfdb --help` (no CCP violation). clean-arch's Dependency-Rule concern about α (that a `tools/` binary calling `cfdb violations` would need to "depend on `cfdb-cli` as a library") is invalidated by the subprocess design — the same relationship `ci/cross-dogfood.sh` already uses. **Re-review needed to confirm clean-arch accepts subprocess-only α.**
- (2) **Superseded by SAP analysis.** Threshold consts move to `tools/dogfood-enrich/src/thresholds.rs` instead of `cfdb-cli`. Both lenses' constraints satisfied: not in `cfdb-core` (clean-arch) and not in highly-efferent `cfdb-cli` (solid-architect SAP).
- (3) **Adopted as-is.** `.cfdb/queries/self-enrich-*.cypher` (single namespace, prefix-disambiguated).

**Verdict R2:** **RATIFY**. Subprocess-only Option α invalidates the original Dependency Rule concern; `tools/dogfood-enrich` depends on `cfdb-core` (types only) + subprocess call to `cfdb` binary — strictly outer-to-inner compile-time graph. One non-blocking observation: I5.1 guard catches "feature missing at build time" but not "pass not yet run against keyspace"; defensively running the enrich pass before the sentinel (already implied by §3.3) covers it — no RFC change needed.

### 5.2 Domain-driven design (`ddd-specialist`) — R1: REQUEST CHANGES

**Verdict R1:** REQUEST CHANGES. Six changes requested:
1. Rename `dogfood-enrich-*` → `self-enrich-*` (align with project's "self-dogfood" / "self-hosted" vocabulary).
2. `enrich-concepts` ground truth: `count(distinct context names across all TOMLs)`, not file count or `[concepts]` entry count.
3. `enrich-concepts` `:CANONICAL_FOR` sentinel: conditional on `$declared_canonical_crate_count > 0`.
4. Replace "ratio" homonym with explicit fractions (different denominators across the 4 ratio-based invariants).
5. Clarify `enrich-bounded-context` invariant scope: tests combined extract+enrich pipeline, not the pass's delta.
6. Add permanent-null tracking language: crates with permanent-null `bounded_context` tracked in filed issue, not silently absorbed by threshold.

**R2 disposition:**
- (1) Adopted globally. All `dogfood-enrich-*` filename references replaced with `self-enrich-*` (§2 deliverable 1, §3.1 table, §3.3, §3.4, §7.x ACs, §4 I1, §I6). Note: the **harness binary name remains `tools/dogfood-enrich`** because it is the dogfood-enforcer, not a self-test artifact; only the `.cypher` files carry the `self-` prefix.
- (2) Adopted in §3.1 + §3.1.2.
- (3) Adopted in §3.1 + §3.1.2; harness contract in §3.5.1 binds `$declared_canonical_crate_count` from TOML scan.
- (4) Adopted in §3.1 table — explicit `nulls(:Item) / count(:Item)` vs `nulls(:Item{kind:Fn}) / count(:Item{kind:Fn})` notation.
- (5) Adopted in new §3.1.1.
- (6) Adopted as new invariant **I3.1** in §4.

**Verdict R2:** **RATIFY**. All 6 changes verified applied; vocabulary across the 7 invariants is unambiguous with respect to bounded contexts, denominator populations, and concept-node ground truth.

### 5.3 SOLID + component principles (`solid-architect`) — R1: REQUEST CHANGES

**Verdict R1:** REQUEST CHANGES. Four changes requested:
1. Option γ broken (`--param` absent on `cfdb violations`); revise lean to **Option α**.
2. Pre-fill Issue 0 AC with the harness shape (now Option α).
3. Clarify §3.1 threshold substitution mechanism (string-substitute vs Cypher named-parameter binding).
4. Sentinel pattern is **not** a DRY violation (denominators differ); keep 7 distinct queries.
5. Issue 0 stays a separate prerequisite (REP); do not merge into Issue 1.

**R2 disposition:**
- (1) Adopted in §3.5 (Option α RATIFIED via R1 council).
- (2) Adopted in §7.1 Issue 0 AC (pre-filled).
- (3) Adopted in §3.1 sentinel-pattern note + §3.5.1 (string-substitute primary path; Cypher named-param binding as future-compatible alternative if/when `cfdb-petgraph` adds support).
- (4) Adopted as RFC stance — no DRY remediation needed.
- (5) Adopted; §7 keeps Issue 0 separate.

**Verdict R2:** **RATIFY**. All 6 changes verified applied. Naming split (`self-enrich-*` / `arch-ban-*` / `tools/dogfood-enrich`) is now CCP-compliant — each component changes for one reason.

### 5.4 Rust systems (`rust-systems`) — R1: REQUEST CHANGES

**Verdict R1:** REQUEST CHANGES. Four changes requested:
1. Fix factual error: `enrich-git-history` emits `git_last_commit_unix_ts`, NOT `commit_age_days` (§3.1 row was wrong).
2. Confirm `hir` feature flag canonical name (verified at `crates/cfdb-cli/Cargo.toml:52`); remove all hedges.
3. Replace "extend `predicate-determinism.sh`" with new `ci/dogfood-determinism.sh` (different subcommand, incompatible param schemas).
4. Add explicit feature-presence guard to I5: harness must check `EnrichReport.ran == true` before running the dogfood query, else exits non-zero with "feature missing" message.

**Side-resolutions from rust-systems analysis (informational, not change requests):**
- Combined nightly build (1 cargo invocation with all 3 features) is cheaper than 3 per-feature jobs.
- `enrich-metrics` dogfood incremental cost is essentially free relative to the pass itself (syn re-parse dominates).
- No trait-object-safety concern (`EnrichBackend` is never used as `dyn`).
- sccache wiring requires copying `ci.yml:61-71` into the new nightly workflow verbatim.

**R2 disposition:**
- (1) Adopted in §3.1 row for `enrich-git-history` (attribute name corrected with file:line citation).
- (2) Adopted: `hir` is the canonical name, all hedges removed in §3.3 + §7.6 AC-2.
- (3) Adopted in §2 deliverable 4 + §3.4 + §I1.
- (4) Adopted as new invariant **I5.1** in §4.
- Side-resolutions adopted in §3.3 (single combined nightly build; sccache wiring directive; soft-warning first cycle).

**Verdict R2:** **RATIFY**. All 4 changes verified applied. Spot-check confirms `Command::Violations` at `args.rs:378-402` has no `--param` flag (factually correct for §2 non-deliverables); `tools/check-prelude-triggers/` confirmed as the `tools/dogfood-enrich/` precedent.

---

## 6. Non-goals

- Cross-dogfood enrichment against graph-specs / qbot-core (separate follow-up if signal warrants).
- Tightening individual thresholds — initial values are the floor at which develop currently passes; tightening is per-pass reviewed PR.
- Any new schema element (covered by §2 non-deliverables).
- Recall measurement (covered by issue #340).
- Query liveness smoke (covered by issue #339).
- Per-PR dogfood of feature-gated passes (cost-prohibitive; nightly catches them).
- **Adding `--param` to `cfdb violations`** (rust-systems verified absent; threshold substitution lives inside `tools/dogfood-enrich`).

---

## 7. Issue decomposition

Each pass = 1 vertical-slice issue. Each issue carries the four-row `Tests:` template per project §2.5 (RFC-033 §3.5). All 7 link back to this RFC via `Refs: docs/RFC-039-dogfood-enrichment-passes.md` and to EPIC #338.

**Note on prerequisites.** Issues 1–7 each depend on Issue 0 (the harness scaffolding — Option α per §3.5). Issue 0 ships the `tools/dogfood-enrich` binary, the threshold-const module, and the `ci/dogfood-determinism.sh` script; subsequent issues only add the per-pass `.cypher` file + CI step entry + threshold const. solid-architect (REP / Reuse-Release Equivalence) confirmed Issue 0 must stay separate, not merged into Issue 1.

### 7.1 Issue 0 — harness scaffolding (Option α, ratified)

**Title:** `ci: dogfood-enrich harness — tools/dogfood-enrich binary, thresholds module, dogfood-determinism.sh (RFC-039 §3.5)`

**AC** (pre-filled per solid-architect change-request 2):
- AC-1: Adds `tools/dogfood-enrich/Cargo.toml` (deps: `clap`, `serde`, `serde_json`, `cfdb-core`).
- AC-2: Adds `tools/dogfood-enrich/src/main.rs` accepting `--pass <name> --db <dir> --keyspace <ks>`. Exits 0 on zero rows, 30 on violations, 1 on runtime error (subprocess fail, missing file, parse error).
- AC-3: Adds `tools/dogfood-enrich/src/thresholds.rs` with `pub const` for each of the 7 thresholds (placeholder values; per-pass issues set the real values).
- AC-4: Adds `tools/dogfood-enrich/src/passes.rs` enumerating `PassDef { name, query_template_path, threshold_const, feature_required }` for the 7 passes.
- AC-5: Adds `tools/dogfood-enrich/src/runner.rs` — template substitution (`{{ threshold }}` → const value), tempfile materialization, subprocess invocation of `./target/release/cfdb violations --rule <tempfile>`, exit-code mapping.
- AC-6: Implements I5.1 feature-presence guard — runs `cfdb enrich-<pass>` first, parses JSON output, checks `ran == true`, exits 1 with "feature missing" message if not.
- AC-7: Adds `ci/dogfood-determinism.sh` — single combined extract → 7 enrich passes → each `dogfood-enrich --pass X` twice → diff stdout. Empty-glob-OK (no .cypher files exist at this stage; script asserts the harness contract independently).
- AC-8: README.md amended with one new row in the §3 dogfood enforcement table naming "Enrichment-pass postconditions" and citing this RFC.
- AC-9: No `.cypher` files added in this issue (those land in Issues 1-7).
- AC-10: CI step in `.gitea/workflows/ci.yml` invokes `tools/dogfood-enrich --pass <name>` for each of the 4 default-feature passes (placeholders, will activate as Issues 1-4 ship).

**Tests:**
- Unit: thresholds module + pass-def enumeration + template-substitution helper tested.
- Self dogfood (cfdb on cfdb): harness invoked with placeholder pass file emits expected exit code.
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): harness runs against companion at pinned SHA with placeholder; exit 30 on any rule row blocks merge.
- Target dogfood: PR body reports harness invocation time on cfdb-self.

### 7.2 Issue 1 — `enrich-deprecation` self-dogfood

**Title:** `ci: self-enrich-deprecation — :Item.is_deprecated count vs source ground truth (RFC-039 §3.1)`

**AC:**
- AC-1: `.cfdb/queries/self-enrich-deprecation.cypher` returns 0 rows on cfdb-self.
- AC-2: A test fixture with a known-broken extractor (deliberately drops one `#[deprecated]`) returns ≥1 row.
- AC-3: CI step wired into `.gitea/workflows/ci.yml` PR-time job.
- AC-4: README.md dogfood table extended with a row for this pass.

**Tests:**
- Unit: pure helper that grep's `#[deprecated]` from a workspace returns the expected count on a fixture.
- Self dogfood: cfdb extracts cfdb-self → `cfdb enrich-deprecation` → harness → 0 rows.
- Cross dogfood: extend `ci/cross-dogfood.sh` to also run this dogfood query against the companion at pinned SHA; exit 30 on rows.
- Target dogfood: PR body reports source-ground-truth deprecated-count + cfdb-extracted count.

### 7.3 Issue 2 — `enrich-rfc-docs` self-dogfood

**Title:** `ci: self-enrich-rfc-docs — :RfcDoc count >= docs/RFC-*.md count, REFERENCED_BY edges > 0 (RFC-039 §3.1)`

**AC:**
- AC-1: `.cfdb/queries/self-enrich-rfc-docs.cypher` returns 0 rows on cfdb-self.
- AC-2: Removing all `:RfcDoc` from a fixture returns 1 row.
- AC-3: Removing all `REFERENCED_BY` edges returns 1 row.
- AC-4: CI step + README row.

**Tests:**
- Unit: pure helper that counts `docs/RFC-*.md` files returns the expected count on a fixture.
- Self dogfood: same path as Issue 1.
- Cross dogfood: same.
- Target dogfood: PR body reports `:RfcDoc` count + edge count + ground-truth file count.

### 7.4 Issue 3 — `enrich-bounded-context` self-dogfood

**Title:** `ci: self-enrich-bounded-context — combined-pipeline coverage ≥MIN_BC_COVERAGE_PCT (RFC-039 §3.1.1)`

**AC:**
- AC-1: Const `MIN_BC_COVERAGE_PCT` initialized to 95.
- AC-2: Query returns 0 rows on cfdb-self with the const at 95.
- AC-3: A fixture with 10% null `bounded_context` returns 1 row.
- AC-4: CI step + README row.
- AC-5: Permanent-null crates (per I3.1) tracked in a filed issue with `Refs: RFC-039 §I3.1`; the issue number is referenced in this PR's body.

**Tests:**
- Unit: sentinel-pattern Cypher unit test in `cfdb-petgraph` against a fixture with known nulls/total.
- Self dogfood: same path as Issue 1.
- Cross dogfood: same.
- Target dogfood: PR body reports `nulls(:Item) / count(:Item)` ratio.

### 7.5 Issue 4 — `enrich-concepts` self-dogfood

**Title:** `ci: self-enrich-concepts — :Concept count == TOML distinct names, LABELED_AS > 0, conditional CANONICAL_FOR (RFC-039 §3.1.2)`

**AC:**
- AC-1: Three-sentinel query on cfdb-self returns 0 rows.
- AC-2: Each of the three failure modes reproducible on a fixture:
  - sentinel (a): `:Concept` count drops below distinct-TOML-name count → 1 row;
  - sentinel (b): zero `LABELED_AS` edges → 1 row;
  - sentinel (c): `$declared_canonical_crate_count > 0` AND zero `CANONICAL_FOR` edges → 1 row;
  - sentinel (c) does NOT fire when `$declared_canonical_crate_count == 0`.
- AC-3: `tools/dogfood-enrich` binds `$declared_canonical_crate_count` from TOML scan before running the query (per §3.1.2).
- AC-4: CI step + README row.

**Tests:**
- Unit: pure TOML-parser helper count-matches the cfdb-self `.cfdb/concepts/*.toml` distinct-name set; separately counts declared `canonical_crate` values.
- Self dogfood: same path as Issue 1.
- Cross dogfood: same.
- Target dogfood: PR body reports the four counts (distinct names, declared canonical_crate count, `:LABELED_AS` count, `:CANONICAL_FOR` count).

### 7.6 Issue 5 — `enrich-reachability` self-dogfood (NIGHTLY, `hir`)

**Title:** `ci: self-enrich-reachability nightly — ≥MIN_REACHABILITY_PCT reachable from :EntryPoint (RFC-039 §3.1)`

**AC:**
- AC-1: Const `MIN_REACHABILITY_PCT` initialized to 80.
- AC-2: Nightly job builds `cfdb-cli` + `tools/dogfood-enrich` with `--features hir,git-enrich,quality-metrics` (single combined build per §3.3).
- AC-3: Posts Gitea commit status `enrich-dogfood/reachability` on develop HEAD.
- AC-4: First cycle soft-warning, second cycle gates merge.
- AC-5: I5.1 feature-presence guard verified in harness output before sentinel runs.
- AC-6: CI step + README row.

**Tests:**
- Unit: BFS-reachability ratio computation on a fixture graph.
- Self dogfood: nightly extract cfdb-self with HIR → `cfdb enrich-reachability` → harness query.
- Cross dogfood: nightly extract companion at pinned SHA with HIR.
- Target dogfood: nightly artifact records `unreachable(:Item{kind:Fn}) / count(:Item{kind:Fn})` ratio.

### 7.7 Issue 6 — `enrich-metrics` self-dogfood (NIGHTLY, `quality-metrics`)

**Title:** `ci: self-enrich-metrics nightly — ≥MIN_METRICS_COVERAGE_PCT non-null cyclomatic + unwrap_count (RFC-039 §3.1)`

**AC:** (mirror of Issue 5 with `MIN_METRICS_COVERAGE_PCT = 95`, feature `quality-metrics`).

**Tests:** (mirror of Issue 5).

### 7.8 Issue 7 — `enrich-git-history` self-dogfood (NIGHTLY, `git-enrich`)

**Title:** `ci: self-enrich-git-history nightly — ≥MIN_GIT_COVERAGE_PCT non-null git_last_commit_unix_ts (RFC-039 §3.1)`

**AC:** (mirror of Issue 5 with `MIN_GIT_COVERAGE_PCT = 95`, feature `git-enrich`, attribute `git_last_commit_unix_ts` per the rust-systems factual correction).

**Tests:** (mirror of Issue 5).

---

## 8. Open council questions — closed by R1 verdicts

All open questions resolved by R1 council verdicts:

- **Q1 (clean-arch §5.1):** threshold consts in `cfdb-core` or `cfdb-cli`? → **Resolved:** `tools/dogfood-enrich` (per SAP analysis; neither `cfdb-core` nor `cfdb-cli`).
- **Q2 (solid-architect §5.3):** runner harness Option α / β / γ? → **Resolved: Option α** (subprocess-only standalone binary).
- **Q3 (rust-systems §5.4):** canonical name of the HIR feature flag? → **Resolved: `hir`** (verified at `crates/cfdb-cli/Cargo.toml:52`).
- **Q4 (ddd-specialist §5.2):** `dogfood-enrich-*` vs `arch-ban-*` naming split? → **Resolved:** `self-enrich-*` (filename prefix); `arch-ban-*` retained for user-code policy. Vocabulary distinction explicit.
- **Q5 (ddd-specialist §5.2):** 95% with no exceptions vs 100% with exception list? → **Resolved:** 95% as initial CI floor + invariant **I3.1** (permanent-null crates tracked in filed issue).

---

## 9. References

- Parent EPIC: #338
- Sibling phases: #339 (Phase A query smoke), #340 (Phase C recall status)
- Pass implementations: `crates/cfdb-petgraph/src/enrich/{git_history,rfc_docs,bounded_context,concepts,reachability,metrics}.rs`
- `enrich-deprecation` source: `cfdb-extractor::extract_deprecated_attr`
- `EnrichBackend` trait: `crates/cfdb-core/src/enrich.rs`
- `git_last_commit_unix_ts` attribute: `crates/cfdb-petgraph/src/enrich/git_history.rs:51` (rust-systems verification)
- `Command::Violations` (no `--param` flag): `crates/cfdb-cli/src/main_command/args.rs:378-402` (rust-systems + solid-architect verification)
- `tools/check-prelude-triggers/Cargo.toml` — Option α precedent (clean-arch + solid-architect verification)
- `cfdb_concepts::declared_contexts`: `crates/cfdb-concepts/src/lib.rs:112-118` (ddd-specialist verification)
- `ContextMeta.canonical_crate: Option<String>`: `crates/cfdb-concepts/src/lib.rs:65` (ddd-specialist verification)
- HIR feature flag canonical name: `crates/cfdb-cli/Cargo.toml:52` (rust-systems verification)
- sccache wiring: `.gitea/workflows/ci.yml:61-71` (rust-systems)
- RFC-cfdb addendum §A2.2 (the seven enrichment-pass row table)
- RFC-036 §3.3 (`enrich-metrics` producer / issue #203)
- RFC-033 §3.5 (four-row Tests template)
- RFC-030 §3.2 (X-ray gate / self-audit step pattern)
- `CLAUDE.md` §3 dogfood enforcement table
- `CLAUDE.md` §6 row 5 ("No metric ratchets")
- Project `CLAUDE.md` §2.5 (Tests prescription template)
- Project `CLAUDE.md` §3 (no baseline files)
