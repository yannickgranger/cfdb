---
title: "RFC-032: v0.2 extractor cohort — issues #35–#51 grouped and sequenced"
status: Implemented on develop — pending vNEXT release (2026-04-24)
date: 2026-04-19
authors: cfdb-architects council (rust-systems lens)
parent: docs/RFC-cfdb.md (RFC-029 v0.1), docs/RFC-cfdb.md (RFC-029 §A1–§A6), docs/RFC-031-audit-cleanup.md
---

# RFC-032 — v0.2 extractor cohort: issues #35–#51

Issues #35–#51 were filed against cfdb before the project adopted the
RFC → spec → issues → implementation workflow. This document retroactively
provides the RFC layer: it groups the issues by delivery dependency,
explains the user value each group delivers, and makes the sequencing
constraints explicit in Rust-specific terms so implementers do not
encounter trap conditions (wrong MSRV, orphan rule violations, object
safety surprises, Cargo.toml churn in the wrong order).

**Cross-references (must be read before implementing any group):**

- RFC-029 §A1.2 — `cfdb-hir-extractor` architectural framing, MSRV
  requirement, object-safety constraint on `HirDatabase`
- RFC-029 §A1.5 — v0.2 acceptance gate items (v0.2-1 through v0.2-9)
- RFC-031 §2 — `StoreBackend` / `EnrichBackend` trait split. All issues
  that touch `StoreBackend` must land after RFC-031 §2 work merges.
- RFC-031 §3 — query-composer relocation. Issue #49 (cfdb-query-dsl)
  must land after this move to avoid importing from a path that is
  about to change.
- council/RATIFIED.md §A.5 — Phase A (syn-backed) vs Phase B (HIR-backed)
  integration split. Issues in Group C and below are Phase B.

---

## Trap index (read before sequencing)

Four sequencing traps exist in this cohort. Each is Rust-specific and
silent — they compile and appear to work, then produce a bug or a
maintenance burden later.

**Trap 1 — MSRV gap.** `cfdb-hir-extractor` requires `ra-ap-hir
≥ 0.0.328`, which uses `edition = "2024"` (stabilized in Rust 1.85).
The workspace currently pins `rust-version = "1.75"` in the root
`Cargo.toml`. Issue #39 (ra-ap-hir upgrade runbook) must bump this
to `"1.85"` before issue #40 (hir-extractor scaffold) adds any
`ra-ap-*` dependency. If #40 lands first, every developer on a
toolchain between 1.75 and 1.84 gets a confusing build error with
no explanation in the Cargo output.

**Trap 2 — exact-pin maintenance debt accrued before runbook exists.**
All 10 `ra-ap-*` sub-crates use `=0.0.N` exact-pinned version
constraints. Upgrading requires touching ≥10 lines in `Cargo.toml`
simultaneously. If the scaffold (#40) adds these pins without the
documented upgrade runbook (#39), the first contributor to hit a
breaking change has no protocol to follow. #39 precedes #40.

**Trap 3 — new pattern arm into complexity-40 nest.** Issue #42 adds
`IMPL_TRAIT_FOR_TYPE` edges, which requires new arms in
`apply_path_pattern` (line 86, cognitive complexity 40). Adding a
new match arm to a function at complexity 40 will push it higher,
making RFC-031 §5 (the refactor to complexity <15) harder to execute
cleanly. RFC-031 §5 (issue #26) must merge before issue #42 to ensure
the new arm lands in refactored code with lower nesting depth.

**Trap 4 — two parser scanners invite drift.** Issue #49 introduces
the cfdb-query DSL, which may add a third scanner alongside the two
in `cfdb-query/src/parser/mod.rs` (see RFC-031 §6). If RFC-031 §6
(issue #28 — `StringAwareScanner` extraction) does not land first,
the DSL work either duplicates the string-literal-awareness pattern a
third time or adds a dependency on an unextracted private function.
Issue #28 precedes #49.

---

## §1 — Group A: syn-level extractor additions

**Issues:** #35 (`:Item.visibility`), #36 (`#[cfg(feature)]`),
#41 (`:EntryPoint` heuristic), #42 (`IMPL_TRAIT_FOR_TYPE` edge)

**User value.** These four issues extend the syn-based extractor with
attributes and edges that Phase A queries require without waiting for
HIR. Visibility gating (`#35`) makes `list_items_matching` results
filterable by `pub` / `pub(crate)` / private — a prerequisite for
the RFC-030 anti-drift gate (graph-specs visibility assertions). Feature
flag extraction (`#36`) makes `#[cfg(feature = "...")]` gates visible
in the fact graph, enabling pattern queries that distinguish "this item
exists behind a feature flag" from "this item always exists". Entry
point heuristics (`#41`) populate `:EntryPoint` nodes for all four
initially-specified kinds (`mcp_tool`, `cli_command`, `http_route`,
`cron_job`) from registration call patterns — no annotation required.
`IMPL_TRAIT_FOR_TYPE` edges (`#42`) are the foundation for Pattern B/C
queries (which impl is canonical, which callers bypass it).

**Rust-systems constraints.**

`#41` and `#42` add new `Visit` implementations to `cfdb-extractor`.
Both must be confined to `cfdb-extractor`; neither type nor edge kind
must appear in `cfdb-core`'s public API surface (architecture test
v0.2-6 extension). Any new node kind added by #41 (`EntryPoint`) must
be added to the `cfdb-core` schema vocabulary crate, not implemented
inline in the extractor — the schema is the contract; the extractor
populates it.

`#42` is gated by **Trap 3**. It must not land before RFC-031 §5
(issue #26). The dependency is:

```
RFC-031 §5 (#26 — pattern.rs refactor) → #42 (IMPL_TRAIT_FOR_TYPE)
```

`#35`, `#36`, `#41` have no blockers within this group and can land
in any order or in parallel once RFC-031 §2 (`EnrichBackend` split,
issue #27) has merged — because the trait split affects the
`StoreBackend` signature that extractor output flows into.

**Workspace Cargo.toml impact:** none. All four issues use existing
workspace deps (`syn`, `cfdb-core`). No new `[dependencies]` lines.

**Execution order within group:**

```
RFC-031 §2 (#27) → RFC-031 §5 (#26) → #42
RFC-031 §2 (#27) → #35, #36, #41 (independent)
```

---

## §2 — Group B: git integration

**Issue:** #37 (`cfdb extract --rev`)

**User value.** The `--rev` flag enables extraction against a
historical revision, materializing a fact snapshot for any `git`
SHA without requiring a working-tree checkout. This is the
mechanism that allows `cfdb diff` (RFC-030 §3.2) to compare base
and head snapshots on PRs. Without `--rev`, the CI diff gate must
manage two separate working-tree checkouts, which is fragile and
slow.

**Rust-systems constraints.** This issue adds `git2` as a workspace
dependency. `git2` links against `libgit2` (C library). On CI this
means the runner needs `libgit2-dev` (Debian) or `libgit2` (Alpine/Nix).
The Cargo.toml addition is one line, but the CI runner provisioning
must be verified. Two implementation choices exist:

1. `git2` (vendored, `feature = ["vendored"]`) — no system dep,
   +30–60s compile, +~5MB binary size.
2. `git2` (dynamic link) — requires system `libgit2`, but compiles
   faster and is appropriate for distro packages.

For an open-source project where contributors run diverse environments,
vendored is the safer default. RFC-032 recommends vendored; the
implementer may override with a documented rationale.

`enrich_git_history` (RFC-029 §A2.2 Stage 1 table) is the primary
consumer of `--rev` output. It can be implemented without `--rev`
using subprocess `git log`, but that imposes a fork-per-file cost
that does not scale on large workspaces. `git2` enables a single
`Repository::open` + `Commit::tree` traversal. The `git2` path is
strongly preferred for correctness and performance.

**Blocking dependencies:** RFC-031 §2 (#27), for same reason as
Group A (StoreBackend signature stabilization).

**Workspace Cargo.toml impact:** one new `[workspace.dependencies]`
line: `git2 = { version = "0.20", features = ["vendored"] }`.
One new `[dependencies]` line in `cfdb-extractor/Cargo.toml`.

---

## §3 — Group C: HIR extractor bootstrap

**Issues:** #39 (ra-ap-hir upgrade runbook), #40 (cfdb-hir-extractor
scaffold)

**User value.** Group C creates the new parallel crate `cfdb-hir-extractor`
documented in RFC-029 §A1.2. Without this crate, all Phase B queries
(call graph, entry-point reachability, method dispatch resolution) are
blocked. Every issue in Groups D and E depends on this group.

**Rust-systems constraints.**

This group is governed by **Trap 1** and **Trap 2** (see Trap index).
The strict execution order is:

```
#39 (runbook + MSRV bump) → #40 (hir-extractor scaffold)
```

`#39` delivers:
1. A documented upgrade protocol at
   `docs/ra-ap-upgrade-protocol.md` — the step-by-step procedure
   for updating all 10 `=0.0.N` exact pins simultaneously, running
   the determinism test suite, and recording the proof. This file is
   the prerequisite for any future `chore/ra-ap-upgrade-*` branch.
2. A bump of `rust-version` in the root `Cargo.toml` from `"1.75"`
   to `"1.85"`. This is a **workspace-wide** change — every crate
   inherits the MSRV floor via `rust-version.workspace = true`. The
   implication: cfdb's minimum supported Rust version becomes 1.85
   for all users. This is acceptable because `ra-ap-hir` is gated
   inside `cfdb-hir-extractor` (a new crate), and the cfdb workspace
   is not published to crates.io (`publish = false`). The MSRV bump
   is justified by the HIR requirement; it is not a casual floor raise.

`#40` delivers the scaffold crate: `crates/cfdb-hir-extractor/Cargo.toml`
with the 10 `ra-ap-*` exact-pinned dependencies, an empty public API
surface, and a passing `cargo test -p cfdb-hir-extractor` run. The
scaffold does not yet populate any `:CallSite` or `CALLS` edges — that
is Group D.

**Object safety constraint (RFC-029 §A1.2).** `HirDatabase` is a
salsa query database. It is NOT object-safe (uses associated types
and generic methods that preclude `dyn HirDatabase`). Every function
in `cfdb-hir-extractor` that accepts the database must take it as
a monomorphic concrete type (`impl HirDatabase + Sized`, or a concrete
`salsa::DatabaseImpl<HirData>` struct). No function in this crate's
public API may have `dyn HirDatabase` in its signature. The architecture
test that validates "no `ra_ap_*` type in cfdb-core public signatures"
(RFC-029 §A1.2 boundary test, acceptance gate v0.2-6) must be authored
in this issue.

**Feature flag topology.** `cfdb-hir-extractor` is a new crate;
it is not feature-flagged inside an existing crate. It is gated by
the workspace's `members` list. Downstream crates (`cfdb-cli`) will
depend on it via `path = "../cfdb-hir-extractor"` when HIR-backed
commands are added. Until those commands exist, `cfdb-cli` does NOT
depend on `cfdb-hir-extractor` — this prevents the 90–150s cold
compile penalty from landing on every `cfdb-cli` build.

**Re-export ergonomics.** No façade crate re-exports from
`cfdb-hir-extractor`. Consumers import directly. This avoids the
orphan-rule trap: `cfdb-hir-extractor` defines its own extraction
traits; the adapter (`PetgraphStore`) implements them in
`cfdb-petgraph`, which depends on both `cfdb-hir-extractor` and
`cfdb-core`. The dependency arrow is:
```
cfdb-hir-extractor → cfdb-core
cfdb-petgraph      → cfdb-core + cfdb-hir-extractor
cfdb-cli           → cfdb-petgraph (and implicitly cfdb-hir-extractor
                      transitively when cfdb-petgraph adds HIR impls)
```
No orphan violation: `cfdb-petgraph` owns both the trait source
(`cfdb-hir-extractor`) and the type source (`cfdb-core::PetgraphStore`).

**Workspace Cargo.toml impact:**
- Root `Cargo.toml`: `members` gains one entry
  (`"crates/cfdb-hir-extractor"`), plus 10 new `[workspace.dependencies]`
  exact-pinned `ra-ap-*` entries, plus the `rust-version` bump.
- New file: `crates/cfdb-hir-extractor/Cargo.toml`.
- No changes to other crates' `Cargo.toml` files yet.
- Estimated churn: 13 lines across 2 files.

**Compilation cost (cold vs warm).** Group C adds ~90–150s to a cold
workspace build (per RFC-029 §A1.2 revised estimate). Incremental
builds touching only `cfdb-hir-extractor` cost ~5–10s (sccache-warm).
Builds that do NOT touch `cfdb-hir-extractor` are unaffected — Cargo's
unit-graph isolation means the `ra-ap-*` crates are not recompiled
unless something in their dep subgraph changes.

---

## §4 — Group D: HIR-dependent features

**Issues:** #43 (5 enrichment passes), #44 (vertical-split-brain
query), #45 (canonical-bypass query), #46 (enrich_bounded_context),
#47 (signature_divergent UDF), #48 (Finding classifier)

**User value.** This group delivers the core detection capability:
the three-phase CFO loop (extract → enrich → classify → query). Each
issue in this group depends on the HIR extractor scaffold from Group C
and the `EnrichBackend` trait from RFC-031 §2. Together they populate
the `:CallSite` / `CALLS` / `INVOKES_AT` / `:EntryPoint` / `:Finding`
graph nodes and emit Pattern B and Pattern C query results.

**Rust-systems constraints.**

**Dependency chain within the group:**

```
Group C (#39, #40 scaffold) →
  #43 (enrichment passes) →
    #44 (vertical-split-brain.cypher — requires CALLS edges)
    #45 (canonical-bypass.cypher — requires CALLS + IMPL_TRAIT_FOR_TYPE)
    #46 (enrich_bounded_context — requires :Item.bounded_context attribute)
      → #47 (signature_divergent UDF — requires bounded_context label)
        → #48 (Finding classifier — requires all enrichment passes complete)
```

`#42` (Group A) must also precede `#45` because `canonical-bypass.cypher`
uses `IMPL_TRAIT_FOR_TYPE` edges to identify canonical impls.

**`EnrichBackend` trait (RFC-031 §2).** The five enrichment passes in
`#43` implement the `EnrichBackend` trait on `PetgraphStore`. This
implementation must not land before RFC-031 §2 (issue #27) splits
`EnrichBackend` out of `StoreBackend` — otherwise the enrich methods
return to being bolted onto the fat trait.

**`signature_divergent` UDF (#47) — algorithm must be documented
before implementation (RFC-029 §A1.5 gate v0.2-8).** The UDF compares
two `:Item` nodes using a field-set comparison algorithm. Without a
documented algorithm, two implementers will write divergent heuristics
that agree on the ground-truth `OrderStatus` test case but diverge on
the next homonym. The deliverable is `docs/udf-signature-divergent.md`
(algorithm + discriminator) AND a unit test against the two known
ground-truth pairs (`OrderStatus`, `PositionValuation` across
`domain-trading` / `domain-portfolio`). Documentation precedes
implementation within issue #47.

**`Finding` classifier (#48) — no `fix_skill` field.** The `:Finding`
schema (RFC-029 §A2.2) explicitly forbids a `fix_skill` field.
The classifier emits `class` only; routing lives in
`SkillRoutingTable` (`.cfdb/skill-routing.toml`). Any implementation
that adds a `fix_skill` or `routing` attribute to the `:Finding` node
violates RFC-029 §A2.2 and must be rejected in code review.

**Object safety note for UDF registration (#47).** If cfdb-store-lbug
is adopted for UDF storage (per RFC-029 §A1.5 gate v0.2-6 UDF scope
clarification), the registration function must not cross the
`cfdb-core` public boundary — confirmed by the architecture test.

**Compilation cost delta.** Adding Group D issues to `cfdb-petgraph`
and `cfdb-extractor` only increases incremental build cost for those
crates. The `cfdb-hir-extractor` salsa database is not re-instantiated
at compile time. Group D adds no new `[workspace.dependencies]`.

---

## §5 — Group E: DSL

**Issue:** #49 (cfdb-query-dsl)

**User value.** A Rust builder DSL for constructing cfdb queries
programmatically, without raw string Cypher composition. Primary
consumer: skills and test fixtures that must construct queries at
runtime without the maintenance burden of string templates.

**Rust-systems constraints.**

**Trap 4 applies.** Issue #28 (RFC-031 §6, `StringAwareScanner`
extraction) must precede #49. The DSL introduces a new query path
through `cfdb-query`; if the underlying scanner primitives are not
yet unified into a shared `StringAwareScanner`, the DSL either
duplicates the string-literal-awareness logic (third copy) or creates
an undocumented internal coupling. The dependency is:

```
RFC-031 §6 (#28 — scanner unification) → #49 (cfdb-query-dsl)
```

RFC-031 §3 (move query composers from `cfdb-core` to `cfdb-query`,
issue #25) must also precede #49. After the move, the canonical
import path for query types is `cfdb_query::*`, not `cfdb_core::*`.
If #49 imports from `cfdb-core` directly, it creates a dependency on
the pre-move path that breaks when #25 lands.

```
RFC-031 §3 (#25 — composer relocation) → #49
```

**crate placement.** The DSL lives in `cfdb-query`. It does not
warrant a new crate: the public item count for the DSL is 5–15
(builder structs + one entry-point function), well within the
single-crate budget. A new crate would add a `[path]` dep, an extra
codegen unit, and a new `members` entry for no isolation benefit.

**Re-export ergonomics.** The DSL's public API (`use cfdb_query::dsl::*`)
must not re-export `cfdb-core` types by value in a way that forces
callers to also depend on `cfdb-core`. DSL builder types may wrap
core types internally but should return `cfdb_query` types at the
public boundary. This preserves the layering: callers depend on
`cfdb-query`, which depends on `cfdb-core`; callers do not need
a direct `cfdb-core` dependency to use the DSL.

**Workspace Cargo.toml impact:** none. DSL is a module within
the existing `cfdb-query` crate.

---

## §6 — Group F: skills and documentation

**Issues:** #50 (operate-module skill), #51 (RFC-029 renumbering),
#3 (cfdb-concepts shared crate — design-pending)

**User value.** #50 implements the `/operate-module` skill as
specified in RFC-029 §A3.4 (two responsibilities: threshold evaluation
+ raid plan emission). #51 renumbers RFC-029 and its addendum to the
final RFC numbers and updates all internal cross-references. #3 is
design-pending: the question of whether a `cfdb-concepts` shared crate
is needed for cross-crate concept vocabulary has not been resolved.

**Rust-systems constraints.**

`#50` has no Rust implementation surface — it is a skill markdown file
plus shell invocations. The relevant Rust constraint is that
`/operate-module` invokes cfdb via subprocess CLI (council/RATIFIED.md
§A.2), NOT as a library. If any implementation draft adds
`use cfdb_core::*` to the skill's Rust layer, that is a violation.
The skill calls `cfdb violations --context <ctx>` and reads the JSON
output.

`#51` is a mechanical rename. It must land last in the documentation
chain — after all cross-references in RFC-030, RFC-031, and RFC-032
are final. If #51 lands first, cross-references in this RFC and
RFC-031 point to stale section numbers.

`#3 (cfdb-concepts shared crate)` — **design recommendation from
Rust-systems lens:** do not create this crate in v0.2. The motivation
for a shared concept vocabulary crate is cross-crate concept sharing,
but the v0.2 bounded-context model already handles this via
`enrich_bounded_context` (issue #46) and `.cfdb/concepts/*.toml`
overrides. A new crate at this stage would:
- Add a new `[workspace.dependencies]` entry consumed by multiple
  crates, increasing the coupling surface before the vocabulary
  stabilizes.
- Risk the orphan-rule trap: if `cfdb-concepts` defines a trait
  that other crates implement, the implementor must depend on both
  `cfdb-concepts` (trait) and `cfdb-core` (types). This is valid
  Rust but adds a dependency edge that may not survive the v0.3
  schema churn around HIR-backed concept inference.
- Contradict the minimal-crate principle: 5–20 pub items per crate
  is the sweet spot; a "concepts vocabulary" crate in v0.2 likely
  contains 2–5 pub types (insufficient to justify a crate boundary).

Recommendation: defer #3 to v0.3. Reopen if empirical evidence shows
that the `.cfdb/concepts/*.toml` override mechanism is insufficient.

---

## §7 — Issue #38 (CfdbCliError / PR in flight)

**Issue:** #38 is tracked by RFC-031 §7 and is in-flight (PR open at
time of RFC-032 drafting). It is included here for completeness and to
avoid duplication with RFC-031.

No action required from RFC-032 implementers: if PR #38 merges before
Group A work begins, all handler return types already use
`CfdbCliError`. If it has not merged, implementers should rebase on
top of it.

---

## §8 — Complete sequencing diagram

All blocking dependencies, including RFC-031 prerequisites:

```
RFC-031 §1 (verify #29)
  └─ RFC-031 §2 (#27 EnrichBackend)
       ├─ RFC-031 §3 (#25 composer relocation) ─── #49 (DSL)
       │      └─ RFC-031 §4 (#23 composition root)
       ├─ RFC-031 §5 (#26 pattern.rs) ─────────── #42 (IMPL_TRAIT_FOR_TYPE)
       ├─ RFC-031 §6 (#28 scanner unification) ── #49 (DSL)
       ├─ #35 (visibility)      ┐
       ├─ #36 (cfg-feature)     ├── (independent, Group A)
       └─ #41 (EntryPoint)      ┘

#37 (git integration) — depends on RFC-031 §2 only

#39 (ra-ap-hir runbook + MSRV 1.85 bump)
  └─ #40 (cfdb-hir-extractor scaffold)
       └─ #43 (enrichment passes)
            ├─ #44 (vertical-split-brain)
            ├─ #45 (canonical-bypass) ← also requires #42
            ├─ #46 (enrich_bounded_context)
            │    └─ #47 (signature_divergent UDF, + docs)
            │         └─ #48 (Finding classifier)
            └── (no other inter-dependencies)

#50 (operate-module skill) — depends on #48 (classifier must exist)
#51 (RFC renumbering) — depends on all RFCs finalized (land last)
#3  (cfdb-concepts crate) — DEFERRED to v0.3 (see §6)
PR #38 (CfdbCliError) — independent, in-flight (see §7)
```

---

## §9 — Workspace Cargo.toml migration scope

Summary of all `Cargo.toml` changes across the cohort, to help
implementers estimate PR churn:

| Issue | Files changed | Lines (+/-) | Notes |
|---|---|---|---|
| #35, #36, #41 | 0 | 0 | Pure extractor code; no new deps |
| #42 | 0 | 0 | Pure petgraph code; no new deps |
| #37 (git2) | 2 | +2 | `[workspace.deps]` + extractor dep |
| #39 (MSRV + pins) | 2 | +12 | Root: rust-version bump + 10 ra-ap pins |
| #40 (hir-extractor) | 2 | +2 | Root members + new Cargo.toml |
| #43–#48 | 1 | +1 | cfdb-petgraph adds `cfdb-hir-extractor` dep |
| #49 (DSL) | 0 | 0 | Module within cfdb-query |
| #50, #51, #3 | 0 | 0 | Skills/docs or deferred |

**Total new `[dependencies]` lines across workspace:** 17. Concentrated
in two PRs: #39 (MSRV + pin block) and #40 (new crate). All other
PRs touch 0–1 Cargo.toml lines. This is low churn relative to the
feature delivery.

---

## §10 — Acceptance gate mapping

RFC-029 §A1.5 defines acceptance gates v0.2-1 through v0.2-9. This
table maps each gate to the issue that satisfies it:

| Gate | Issue(s) | Group |
|---|---|---|
| v0.2-1 (:EntryPoint ≥95% coverage) | #41 | A |
| v0.2-2 (vertical-split-brain reproduces #2651) | #44 | D |
| v0.2-3 (canonical-bypass reproduces #3525) | #45 | D |
| v0.2-4 (CALLS recall ≥80% on 3 crates) | #40 scaffold + #43 | C/D |
| v0.2-5a (hir-extractor cold build ≤180s) | #40 | C |
| v0.2-5b (extract ≤5 min, RSS ≤4 GB) | #43 | D |
| v0.2-5c (ra-ap-upgrade protocol dry-run) | #39 | C |
| v0.2-6 (no ra_ap_* in cfdb-core sigs) | #40 (arch test) | C |
| v0.2-7 (rust-version = "1.85") | #39 | C |
| v0.2-8 (signature_divergent documented + ground truth) | #47 | D |
| v0.2-9 (enrich_bounded_context spot-check) | #46 | D |

---

## §11 — Open questions (not blocking v0.2)

1. **`cfdb-hir-extractor` cold build measurement.** The 90–150s
   estimate (RFC-029 §A1.2) is based on counting `ra-ap-*` transitive
   crates and the presence of salsa proc-macros. It has not been
   measured against the actual dependency graph of the pinned versions.
   Gate v0.2-5a (`cargo clean && time cargo build -p cfdb-hir-extractor
   ≤ 180s`) produces the empirical number. If the actual cold build
   exceeds 180s, the gate fails and the 180s ceiling must be revisited
   with a council vote before v0.2 ships.

2. **`cfdb-cli` dep on `cfdb-hir-extractor`.** Currently
   `cfdb-cli/Cargo.toml` does not depend on `cfdb-hir-extractor`
   (the crate does not exist yet). When HIR-backed CLI commands land
   (#44, #45), `cfdb-cli` will need to add the dep. At that point the
   full 90–150s cold compile penalty becomes part of `cfdb-cli`'s
   build. A feature flag (`hir`) on `cfdb-cli` that gates the HIR dep
   is worth evaluating at that stage.

3. **`ra-ap-rustc_type_ir` versioning.** This sub-crate versions
   independently from the other 9 `ra-ap-*` crates (2–3 releases per
   week per RFC-029 §A1.2). The upgrade runbook (#39) must explicitly
   cover how to handle a `ra-ap-rustc_type_ir` release that does not
   align with the rest of the `ra-ap-*` pin block.

---

*RFC-032 — drafted by rust-systems (Rust-systems lens), 2026-04-19.*
*All file:line citations verified against HEAD on branch*
*`docs/rfc-030-anti-drift` (commit 250aac4 + RFC-031 merge).*

---

## Landing trail

All v0.2 cohort slices (Groups A–D) are CLOSED on `agency:yg/cfdb`:

- **Group A — syn-level extractor additions:** #35, #36, #41, #42
- **Group B — git integration:** #37
- **Group C — HIR bootstrap:** #39, #40
- **Group D — HIR-dependent features:** #43, #44, #45, #46, #47, #48, #49, #50, #51

`SchemaVersion::V0_2_0` shipped via #86. Later v0.2.x minor bumps
(V0_2_1 – V0_2_3) land the enrichment passes (#106, #107, #108, #109,
#110) and are covered by CHANGELOG.md.

Status flipped from `Draft` → `Implemented on develop` as part of the
monthly gap-audit cleanup (cfdb #258, filed 2026-04-24). Release of
the landed batch is tracked under cfdb #257 (v0.4.0).
