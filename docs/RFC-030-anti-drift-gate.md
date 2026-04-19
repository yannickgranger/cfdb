---
title: RFC-030 — Anti-drift gate: adopt graph-specs + cfdb self-dogfood
status: Draft
date: 2026-04-19
authors: cfdb-architects council (clean-arch lens)
parent: docs/RFC-cfdb.md (RFC-029 v0.1), docs/RFC-cfdb-v0.2-addendum-draft.md (RFC-029 addendum)
---

# RFC-030 — Anti-drift gate: adopt graph-specs + cfdb self-dogfood

---

## §1 Context

### §1.1 The problem this RFC addresses

cfdb is an open-source tool. Its stated mission is to detect structural
drift in Rust workspaces — duplicate concepts, canonical bypasses,
unwired logic, forbidden patterns. That mission is credible only if cfdb
itself does not drift. An X-ray machine that cannot scan itself is not
trustworthy.

As of 2026-04-19, cfdb's own workspace lacks:

1. **Concept-level specs** — no machine-readable contract for what each
   crate is responsible for. Any contributor (human or agent) can add a
   new concept to `cfdb-extractor` that belongs in `cfdb-core`, or move
   a query primitive into `cfdb-cli`, and no automated gate will catch
   it before merge.

2. **Self-audit** — cfdb can run its own violation rules against any
   workspace. It does not run them against itself. The 9 patterns
   documented in RFC-029 §3 apply to cfdb's own codebase just as they
   apply to qbot-core's.

3. **Anti-drift CI gate** — no PR gate currently blocks a contribution
   that introduces a new split-brain, a context homonym, or a canonical
   bypass into cfdb itself.

### §1.2 The sibling project

`graph-specs-rust` (agency forge: `yg/graph-specs-rust`, track `develop`
always) is cfdb's complement. cfdb is the **X-ray** — it detects
existing drift in a workspace. graph-specs-rust is the **vaccine** — it
blocks new drift from entering via a CI gate that compares machine-readable
markdown specs against the actual code structure.

The two tools pair: graph-specs declares what each crate *should* contain
(concept ownership, public surface, dependencies); cfdb audits what
the crate *does* contain. Together they close the loop:

```
graph-specs specs/  ─── declares intent ───────────────────────────────┐
                                                                        │
cfdb extract .    ─── audits reality ──── cfdb violations ─── CI gate ─┘
                                                 ↑ blocks merge if drift
```

### §1.3 The user directive

On 2026-04-19 the project owner stated: **"no more split-brains / drift"**
and established the workflow: **architects write RFC → spec → issues →
implementation**. This RFC is the rationale layer. Specs come in Task #4
(per-crate concept specs). Issues #22–#29 and #35–#51 are the
implementation backlog.

The user also noted: **"this is open-source code, all planet will be
watching you."** The quality bar is public-CV level. The anti-drift gate
must be credible, not ceremonial.

### §1.4 Why now

The cfdb workspace has six crates as of this RFC:

| Crate | Responsibility |
|---|---|
| `cfdb-core` | Fact types, query AST, StoreBackend trait, schema vocabulary |
| `cfdb-query` | Cypher-subset parser (chumsky) + Rust builder API |
| `cfdb-petgraph` | StoreBackend impl on `petgraph::StableDiGraph` |
| `cfdb-extractor` | Rust workspace → facts via `syn` + `cargo_metadata` |
| `cfdb-recall` | Extractor recall audit vs. `rustdoc-json` ground truth |
| `cfdb-cli` | `cfdb` binary — entry point for all API verbs |

The crate decomposition is deliberate and tested (recall gate at 95%,
`cfdb-core` at 100%). This is the right moment to lock concept ownership
before v0.2 work (issues #35–#51) adds HIR extraction, new verbs, and a
bounded-context enrichment pass — all of which will strain crate
boundaries if those boundaries are not documented first.

---

## §2 Non-goals

This RFC does **not** propose:

1. **A new cfdb query pattern.** graph-specs integration is not a new
   Pattern J or K in RFC-029 §3. It is a CI workflow decision, not a
   schema extension.

2. **Changes to the cfdb API or schema.** The 16 verbs ratified in
   council/RATIFIED.md (including `list_items_matching` as the 16th)
   are unchanged by this RFC. RFC-030 adds no new verb and no new
   node/edge type.

3. **Specs for the consuming workspace (qbot-core or others).** This
   RFC scopes specs to the cfdb workspace only. Other workspaces that
   adopt cfdb may write their own specs; that is their authors' concern.

4. **Exhaustive graph-specs dialect coverage.** This RFC adopts the
   concept-level subset of the graph-specs dialect. Signature-level
   and relationship-level checks are deferred until the graph-specs
   `develop` branch ships them as stable.

5. **Automatic remediation.** The CI gate blocks; it does not fix.
   cfdb's invariant "never modifies Rust files" (RFC-029 §4) applies
   here without exception.

6. **A metric ratchet or allowlist.** Per CLAUDE.md §6 rule 8 and
   council/RATIFIED.md §A.9, no `expected_violations.json` whitelist,
   no per-finding waiver, no ceiling file. A violation is fixed by
   editing the spec or fixing the code in the same PR.

7. **Retroactive spec compliance for issues #22–#29.** Those are
   absorbed by RFC-031. This RFC defines the gate; RFC-031 and RFC-032
   schedule the work to pass it.

---

## §3 Decision

This RFC adopts three interlocking mechanisms. All three are mandatory;
none is optional.

### §3.1 Adoption 1 — graph-specs CI gate

**Decision:** adopt `graph-specs` as a CI gate on every PR to cfdb.

**CLI invocation:**

```bash
graph-specs check \
  --specs specs/concepts/ \
  --code crates/ \
  --format github-actions
```

**Semantics:**

- `specs/concepts/` contains one markdown file per workspace crate
  (see §6 for layout).
- `graph-specs check` compares declared concept ownership, dependency
  direction, and public surface against the actual crate structure.
- `--format github-actions` emits annotations readable by the GitHub
  Actions workflow renderer.
- **Exit code non-zero = merge blocked.** No baseline. No allowlist.
  No "informational only" mode for new violations.

**Gate position in CI:** runs after `cargo test --workspace` and
`cargo clippy --workspace -- -D warnings`. A compilation failure must
not mask a spec violation; both must be visible.

**Responsibility for authoring specs:** architects (the council that
ratifies RFCs) write the initial specs as part of Task #4. Implementers
update the spec in the same PR as any code change that crosses a
concept boundary. The spec is the contract; the code is the
implementation.

### §3.2 Adoption 2 — cfdb self-audit gate

**Decision:** run cfdb against its own workspace on every PR.

**CLI invocation:**

```bash
cfdb extract --workspace . --output .cfdb/snapshots/
cfdb violations --rules examples/queries/ --keyspace .cfdb/snapshots/
```

The first command extracts cfdb's own workspace into a fresh snapshot.
The second command runs every `.cypher` rule file in `examples/queries/`
against that snapshot and reports violations.

**Semantics:**

- Any violation classified as `canonical_bypass` or `context_homonym`
  **blocks merge** (per council/RATIFIED.md §A.8 BLOCK routing).
- Any violation classified as `duplicated_feature`, `unfinished_refactor`,
  or `random_scattering` **warns** but does not block.
- New violations introduced by the PR (not present in the base-branch
  snapshot) are always blocking regardless of class, because they
  represent regression from current state. Pre-existing violations
  in untouched scope are advisory.

**Diff gate:**

```bash
cfdb diff .cfdb/snapshots/<base-sha>.jsonl.gz \
           .cfdb/snapshots/<head-sha>.jsonl.gz \
  --new-violations-block
```

The `--new-violations-block` flag exits non-zero only when the diff
introduces net-new violations. Pre-existing violations in untouched
scope are not re-reported (per council/RATIFIED.md §A.8 last row).

**Snapshot storage:** `.cfdb/snapshots/<sha>.jsonl.gz` is committed to
the repository as a determinism fixture (tier-2 per council/RATIFIED.md
§A.10). The `.cfdb/` directory is gitignored except for `snapshots/`.

### §3.3 Adoption 3 — RFC-to-spec-to-issue-to-impl workflow

**Decision:** establish the RFC → spec → issues → implementation
workflow as the mandatory authoring pipeline for all cfdb architectural
changes, starting with this RFC.

**The four stages:**

| Stage | Artifact | Owner | Gate |
|---|---|---|---|
| RFC | `docs/RFC-NNN-*.md` | Architects council | Council approval |
| Spec | `specs/concepts/<crate>.md` | Architects council | graph-specs CI passes |
| Issues | Forge issues #N–#M | Team lead | Linked to RFC in issue body |
| Implementation | Code changes in `crates/` | Implementers | All CI gates pass |

**Rules:**

1. A spec change must reference the RFC that motivated it. No spec
   evolves without a rationale document.
2. An implementation that crosses a concept boundary must update the
   spec in the same PR. Split PRs (spec first, code later) are
   permitted; merged code without a matching spec is not.
3. The RFC is the rationale. The spec is the contract. They are
   **non-overlapping artifacts** — see §4.

---

## §4 Registry boundary

Two documentation directories exist in the cfdb workspace:

| Directory | Purpose | Format | Machine-readable? |
|---|---|---|---|
| `docs/` | Rationale — WHY decisions were made, historical context, council verdicts, alternatives considered | Freeform markdown | No |
| `specs/` | Contract — WHAT each crate is responsible for, in the graph-specs dialect | Structured graph-specs markdown | Yes — consumed by `graph-specs check` |

**The boundary is non-overlapping by design:**

- `docs/` files are for humans and council agents reading them as
  narrative. They contain motivations, rejected alternatives, risk
  registers, and cross-references to external issues. They are never
  parsed by `graph-specs check`.
- `specs/` files are for the CI gate. They contain concept declarations,
  dependency assertions, and public surface claims — and nothing else.
  They do not contain rationale prose.

A spec file must not explain why a boundary exists; that belongs in the
corresponding RFC. An RFC file must not contain machine-parseable spec
syntax; that belongs in `specs/`.

**This separation prevents a common failure mode:** if rationale and
contract are co-located, one decays when the other is updated. RFCs
accumulate addenda; specs drift. Keeping them in separate directories
with different owners (architects write both, but the spec is the
living contract while the RFC is the frozen rationale) forces explicit
acknowledgment when a conceptual boundary changes.

---

## §5 Dialect subset

cfdb adopts the **concept-level subset** of the graph-specs dialect.

### §5.1 What the concept-level subset covers

```
concept <ConceptName>
  owned_by: <crate-name>
  must_not_depend_on: [<crate-name>, ...]
  visibility: public | crate-private
  examples: [<qname>, ...]
```

This subset answers three questions per concept:

1. **Ownership** — which crate is the canonical home of this concept?
   Drift is detected when the concept appears in a crate that does not
   own it.
2. **Dependency direction** — which crates must not be imported by the
   owning crate? This enforces the dependency rule (inner layers do not
   import outer layers).
3. **Visibility** — is this concept part of the public API or an
   internal implementation detail?

### §5.2 What is deferred

The following graph-specs features are deferred until the graph-specs
`develop` branch ships them as stable:

- **Signature-level checks** — asserting that a public function's
  parameter and return types conform to declared contracts. Deferred
  because signature checking requires type resolution beyond what the
  current graph-specs dialect supports without HIR.
- **Relationship-level checks** — asserting that a specific
  `CALLS` or `IMPLEMENTS` edge must or must not exist. Deferred until
  cfdb-hir-extractor (RFC-029 §A1.2) ships and graph-specs can consume
  the call graph.
- **Cross-crate concept co-ownership** — for the Shared Kernel pattern
  where two crates legitimately share a concept. Deferred; the v0.1
  dialect treats shared ownership as a violation, which is correct for
  cfdb's current crate structure.

### §5.3 Upgrade path

When graph-specs `develop` ships signature-level or relationship-level
checks, cfdb adopts them in a dedicated RFC that:

1. Documents which new checks are adopted.
2. Adds or updates spec files to use the new syntax.
3. Verifies CI passes before merging.

No silent dialect upgrades. Every new check type requires an RFC entry.

---

## §6 Per-crate spec layout

### §6.1 File per crate

One spec file per workspace crate, at:

```
specs/concepts/<crate-name>.md
```

Six initial spec files (authored in Task #4):

```
specs/concepts/cfdb-core.md
specs/concepts/cfdb-query.md
specs/concepts/cfdb-petgraph.md
specs/concepts/cfdb-extractor.md
specs/concepts/cfdb-recall.md
specs/concepts/cfdb-cli.md
```

`cfdb-cli` is a binary crate with no public Rust API surface but has
a well-defined CLI contract (the 16 verbs); its spec covers concept
ownership and the entry-point contract, not public symbols.

### §6.2 Spec file structure (template)

```markdown
---
crate: <crate-name>
rfc: RFC-030 (+ any crate-specific RFC)
status: draft | approved
---

# Spec: <crate-name>

## Owned concepts

<!-- Concepts that this crate owns. Any other crate containing
     these concepts triggers a violation. -->

concept <ConceptName>
  owned_by: <crate-name>
  visibility: public
  examples: [<qname1>, <qname2>]

## Dependency assertions

<!-- Crates that this crate must NOT import. Violations indicate
     a dependency rule inversion. -->

must_not_depend_on: [<crate-a>, <crate-b>]

## Public surface contract

<!-- Minimum set of public items that must exist.
     Removal of any item in this list is a breaking change. -->

required_public: [<qname1>, <qname2>]
```

### §6.3 Authoring rules

1. **One RFC citation per spec file.** The frontmatter `rfc:` field
   names the RFC that ratified the concept boundary. Multiple RFCs
   may be cited (comma-separated) if the spec spans multiple decisions.
2. **No rationale prose in spec files.** Rationale belongs in `docs/`.
   A spec file should be parseable by a tool that does not read English.
3. **Spec files are committed before the CI gate is enabled.** The CI
   gate is enabled (in `ci/` workflow YAML) only after all six initial
   spec files exist and `graph-specs check` passes on main.
4. **A spec update without a code change is allowed.** Clarifying
   concept ownership without moving code is a legitimate evolution.
5. **A code change without a spec update is blocked** if the change
   introduces a concept into a crate that does not own it per the spec.

---

## §7 Consequences

### §7.1 What improves

- **Self-consistent documentation.** cfdb's architecture is now
  documented at two levels: narrative (RFC docs) and contract (specs).
  A new contributor can read the spec to understand crate boundaries
  and the RFC to understand why they exist.
- **Regression prevention.** The CI gate makes concept drift visible
  at PR time, not at code-review time. A reviewer no longer needs to
  hold the entire crate dependency graph in their head.
- **Dogfood credibility.** A tool that cannot pass its own gates is not
  credible. cfdb passing both the graph-specs gate and its own self-audit
  is the minimum viable proof that the toolchain works.
- **Bootstrapped spec library.** Task #4 produces six spec files that
  serve as reference implementations for workspaces adopting cfdb.

### §7.2 What it costs

- **CI time.** `cfdb extract --workspace .` on the cfdb workspace is
  fast (six small crates, no HIR dependency in v0.1). Estimated
  addition: +15–30s on a warm cache. `graph-specs check` adds another
  5–10s. Total CI budget increase: under 45s. Acceptable.
- **Spec authoring burden.** Task #4 requires architects to write six
  spec files. This is a one-time cost amortized over the life of the
  project. Spec updates on subsequent PRs are small (one or two concept
  declarations added per PR).
- **Learning curve.** Contributors unfamiliar with the graph-specs
  dialect must read `specs/concepts/core.md` in the graph-specs-rust
  repository before authoring specs. The dialect is small and the
  reference file is the learning material.
- **False positives.** The concept-level subset is coarse. It will
  flag any struct or function that appears in the wrong crate, even if
  the placement is intentionally temporary during a refactor. Mitigation:
  a PR that moves a concept across crates updates both spec and code
  atomically; the gate passes because the spec reflects intent.

### §7.3 Who authors specs

Architects write specs when ratifying RFCs. The council that approved
RFC-029 (and this RFC-030) is the authoring body for the initial six
specs. For subsequent crates (e.g., `cfdb-hir-extractor` introduced by
RFC-029 §A1.2), the RFC that introduces the crate must include a
corresponding spec as a required deliverable.

### §7.4 What breaks if graph-specs or cfdb changes dialect

- If graph-specs changes its dialect in a breaking way, the `specs/`
  files may require updates. The graph-specs `develop` branch is the
  authoritative dialect reference; cfdb pins to a specific release tag,
  not `develop`, for CI stability.
- If cfdb changes its violation output format, the `cfdb violations`
  invocation in §3.2 may require flag updates. The cfdb CLI is
  self-dogfooded, so any breaking CLI change discovered via self-audit
  is a bug to fix before the breaking change merges.

---

## §8 Acceptance gates

This RFC is satisfied when **all five** of the following are true:

| # | Gate | Measurable |
|---|---|---|
| G1 | `specs/concepts/` contains one approved spec file per workspace crate (six files: cfdb-core, cfdb-query, cfdb-petgraph, cfdb-extractor, cfdb-recall, cfdb-cli) | `ls specs/concepts/*.md | wc -l` = 6 |
| G2 | `graph-specs check --specs specs/concepts/ --code crates/` exits 0 on the main branch | CI run link in the PR merging Task #4 output |
| G3 | `cfdb extract --workspace . && cfdb violations --rules examples/queries/` exits 0 (no blocking violations) on the main branch | CI run link in the PR merging cfdb self-audit gate |
| G4 | The CI workflow file (`ci/`) invokes both gates (§3.1 and §3.2) and fails the build on non-zero exit | Verified by a PR that introduces a deliberate spec violation, confirms CI blocks, then reverts |
| G5 | No `expected_violations.json` or equivalent allowlist exists in the repository | `find . -name "expected_violations*" -o -name "*.allowlist" | wc -l` = 0 |

Gates G1 and G2 are satisfied by Task #4.
Gates G3 and G4 are satisfied by the issues tracking §3.2 implementation.
Gate G5 is an invariant that must hold continuously, not a one-time check.

---

## §9 References

### Internal

- `docs/RFC-cfdb.md` — RFC-029 v0.1 ratified 2026-04-13. Defines the
  9 problem patterns, the 16 API verbs (including `list_items_matching`
  as the 16th per council/RATIFIED.md §A.14), the fact schema, and the
  determinism invariants that the self-audit CI gate must not violate.
- `docs/RFC-cfdb-v0.2-addendum-draft.md` — RFC-029 addendum, council
  second-pass GREEN 2026-04-14. Defines the six debt-cause classes
  (§A2.1), the CI BLOCK/WARN routing table (§A.8 in council/RATIFIED.md),
  and the no-allowlist rule (§A.9 in council/RATIFIED.md).
- `council/RATIFIED.md` — convergent decisions from the cfdb wiring
  council 2026-04-14. §A.8 defines BLOCK/WARN routing. §A.9 forbids
  metric ratchets. §A.10 defines the three-tier artifact storage model.
- `KNOWN_GAPS.md` — cfdb-recall gap log. The 95% recall threshold
  (defined as `DEFAULT_THRESHOLD` const in `cfdb-recall/src/lib.rs`)
  is the model for the no-ratchet rule applied to spec compliance in
  this RFC.
- Issues #22–#29 — orphan audit issues absorbed by RFC-031. These
  represent known architectural debt discovered in the cfdb workspace
  audit; the anti-drift gate (this RFC) is the mechanism that prevents
  new debt of the same kind from accumulating.

### External

- `graph-specs-rust` repository (`yg/graph-specs-rust`, `develop`
  branch) — the vaccine tool. `specs/dialect.md` documents the
  machine-parseable spec format. `specs/concepts/core.md` is the
  reference implementation of a concept-level spec file.
- CLAUDE.md §6 rule 8 — "no metric ratchets" rule. Applies to the
  spec compliance gate: no allowlist, no ceiling, no waiver mechanism.
- CLAUDE.md §4 — the RFC-to-spec-to-issue-to-impl workflow codified
  in §3.3 of this RFC is an application of the outside-in development
  methodology documented there.
