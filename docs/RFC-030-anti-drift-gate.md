---
title: RFC-030 — Anti-drift gate: adopt graph-specs + cfdb self-dogfood
status: Draft (revision 1 — 2026-04-19)
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
(concept ownership, public surface, port signatures); cfdb audits what
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

3. **Specs for consuming workspaces (qbot-core or others).** This RFC
   scopes specs to the cfdb workspace only. Other workspaces that adopt
   cfdb may write their own specs; that is their authors' concern.

4. **Immediate adoption of all graph-specs levels.** This RFC adopts
   concept-level checks initially, with signature-level as a planned
   follow-on (see §5). Relationship and bounded-context levels are
   deferred to when graph-specs ships them.

5. **Automatic remediation.** The CI gate blocks; it does not fix.
   cfdb's invariant "never modifies Rust files" (RFC-029 §4) applies
   here without exception.

6. **A metric ratchet or allowlist.** Per CLAUDE.md §6 rule 8 and
   council/RATIFIED.md §A.9, no `expected_violations.json` whitelist,
   no per-finding waiver, no ceiling file. A violation is fixed by
   editing the spec or fixing the code in the same PR.

7. **Retroactive spec compliance for issues #22–#29.** Those are
   absorbed by RFC-031. This RFC defines the gate; RFC-031 schedules the
   work to pass it.

---

## §3 Decision

This RFC adopts three interlocking mechanisms. All three are mandatory;
none is optional.

### §3.1 Adoption 1 — graph-specs CI gate

**Decision:** adopt `graph-specs` as a CI gate on every PR to cfdb.

**CLI invocation:**

```bash
graph-specs check --specs specs/concepts/ --code crates/
```

**Semantics:**

- `specs/concepts/` contains one markdown file per workspace crate
  (see §6 for layout). graph-specs walks every `.md` file under that
  directory.
- `--code crates/` points graph-specs at the workspace crate sources.
  It walks every `*.rs` file under each crate's `src/` directory and
  extracts top-level `pub struct`, `pub enum`, `pub trait`, `pub type`
  declarations.
- graph-specs builds two graphs — one from specs, one from code — and
  reports every named concept that appears in one but not the other.
- **Exit code non-zero = merge blocked.** No baseline. No allowlist.
  No "informational only" mode for new violations.

**Gate position in CI:** runs after `cargo test --workspace` and
`cargo clippy --workspace -- -D warnings`. A compilation failure must
not mask a spec violation; both must be visible in CI output.

**Responsibility for authoring specs:** architects (the council that
ratifies RFCs) write the initial specs as part of Task #4. Implementers
update the spec in the same PR as any code change that crosses a
concept boundary. The spec is the contract; the code is the
implementation.

### §3.2 Adoption 2 — cfdb self-audit gate

**Decision:** run cfdb against its own workspace on every PR.

**CLI invocation:**

```bash
cfdb extract --workspace . --db .cfdb/db --keyspace cfdb-self
cfdb violations --db .cfdb/db --keyspace cfdb-self \
  --rule examples/queries/hsb-by-name.cypher
```

The first command extracts cfdb's own workspace into a fresh keyspace.
The second command runs a violation rule against that keyspace and
reports matches. One `cfdb violations` invocation per rule file; the CI
step loops over the rules in `examples/queries/`.

**Note on `--rule` flag:** `--rule` takes a single `.cypher` file, not
a glob. The CI script iterates:

```bash
for rule in examples/queries/*.cypher; do
  cfdb violations --db .cfdb/db --keyspace cfdb-self --rule "$rule"
done
```

**Semantics:**

- Any violation classified as `canonical_bypass` or `context_homonym`
  **blocks merge** (per council/RATIFIED.md §A.8 BLOCK routing).
- Any violation classified as `duplicated_feature`, `unfinished_refactor`,
  or `random_scattering` **warns** but does not block.
- The extract is always a fresh run against HEAD. Pre-existing violations
  in untouched scope are advisory; violations in the scope touched by
  the PR are blocking.

**Diff gate (future):** a `cfdb diff --db .cfdb/db --a <base-ks>
--b <head-ks>` command is present in the CLI but is a Phase A stub
(no violation-delta output yet). A dedicated issue will track adding
`--new-violations-block` semantics that exit non-zero only when the
diff introduces net-new violations. Until that lands, the per-PR gate
re-runs all rules on HEAD and the reviewer inspects the diff.

**Snapshot storage:** `.cfdb/snapshots/<sha>.jsonl.gz` committed as a
determinism fixture (tier-2 per council/RATIFIED.md §A.10). The
`.cfdb/` directory is gitignored except for `snapshots/`.

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

**The boundary is non-overlapping by design** (this mirrors graph-specs'
own `specs/` vs. `docs/` separation per `specs/dialect.md` — the tool
practices what it prescribes):

- `docs/` files are for humans and council agents reading them as
  narrative. They contain motivations, rejected alternatives, risk
  registers, and cross-references to external issues. They are never
  walked by `graph-specs check`.
- `specs/` files are for the CI gate. They contain concept declarations
  and optional port signatures — and nothing else structural that the
  gate depends on. Prose is encouraged (graph-specs ignores it) but
  must not substitute for structural declarations.

A spec file must not explain why a boundary exists; that belongs in the
corresponding RFC. An RFC file must not contain machine-parseable spec
headings that are intended to drive the gate; that belongs in `specs/`.

**This separation prevents a common failure mode:** if rationale and
contract are co-located, one decays when the other is updated. RFCs
accumulate addenda; specs drift. Keeping them in separate directories
with different owners (architects write both, but the spec is the
living contract while the RFC is the frozen rationale) forces explicit
acknowledgment when a conceptual boundary changes.

---

## §5 Dialect subset

cfdb adopts the graph-specs markdown dialect. The dialect specification
lives at `specs/dialect.md` in the `graph-specs-rust` repository; this
section records which levels cfdb enables now and which it plans to
enable later.

### §5.1 How the dialect works

graph-specs builds a concept graph by parsing two sources independently:

**From markdown specs (`specs/concepts/*.md`):**
- Level-2 (`##`) and level-3 (`###`) headings become concept nodes. The
  heading text is the concept name; inline backticks are stripped;
  generic parameters are removed (`## Graph<T>` becomes `Graph`).
- Fenced ` ```rust ` code blocks inside a concept's section carry the
  signature for signature-level comparison (v0.2 of the dialect, already
  implemented in graph-specs).
- Bullets with the prefixes `- implements: X`, `- depends on: X`,
  `- returns: X` declare relationship edges (v0.3 of the dialect,
  currently parsed but not yet diffed).
- All other prose, tables, blockquotes, images, and non-`rust` code
  blocks are ignored by the reader.

**From Rust code (`crates/**/*.rs`):**
- Top-level `pub struct`, `pub enum`, `pub trait`, `pub type`
  declarations in each `*.rs` file become concept nodes.
- Items inside `#[cfg(test)]`, items nested inside `pub mod`, `impl`
  blocks, `fn`, `const`, `static`, `use`, and `macro_rules!` are not
  concept nodes.

**Equivalence check:** the diff engine reports:
- Concepts in specs but not in code (stale spec — a type was removed
  without updating the spec).
- Concepts in code but not in specs (undeclared type — a type was added
  without a spec entry).

The canonical worked example of a spec file in this dialect is
`specs/concepts/core.md` in the graph-specs-rust repository.

### §5.2 Levels adopted

**Now (this RFC):** concept-level only. Every top-level public type
must appear as a `##` or `###` heading in the crate's spec file, and
every heading must correspond to a type in the code. No concept may
exist only in one of the two graphs.

**Planned follow-on (separate RFC, no issue yet):** signature-level.
Port traits such as `StoreBackend` and `Reader` already carry fenced
`rust` blocks in the initial spec files committed by Task #4. Once cfdb
adopts signature-level checking, those blocks will be diffed against the
actual trait signatures. The spec files are already structured for this;
enabling the level requires only a CI flag change and a new RFC entry.

**Deferred:** relationship-level (`- implements:`, `- depends on:`,
`- returns:` bullets) and bounded-context checks are not yet diffed by
graph-specs. cfdb will adopt them when graph-specs ships them as a
stable gate level.

### §5.3 Upgrade path

When graph-specs ships a new gate level as stable, cfdb adopts it via a
dedicated RFC that:

1. Documents which new level is adopted.
2. Updates spec files to use the new syntax where applicable.
3. Verifies CI passes on the main branch before the RFC merges.

No silent dialect upgrades. Every new level activation requires an RFC
entry.

---

## §6 Per-crate spec layout

### §6.1 File per crate

One spec file per workspace crate, at:

```
specs/concepts/<crate-name>.md
```

Six initial spec files (committed in Task #4):

```
specs/concepts/cfdb-core.md
specs/concepts/cfdb-query.md
specs/concepts/cfdb-petgraph.md
specs/concepts/cfdb-extractor.md
specs/concepts/cfdb-recall.md
specs/concepts/cfdb-cli.md
```

`cfdb-cli` is a binary crate with no public Rust API surface. Its spec
covers the types that the CLI module system exports (e.g. `EnrichVerb`)
and will grow to cover the composition root type prescribed by RFC-031.

When a new crate is added to the workspace (e.g. `cfdb-hir-extractor`
per RFC-029 §A1.2), the RFC introducing it must include a corresponding
spec file as a required deliverable. The gate fails on any crate whose
public types are not covered by a spec.

### §6.2 Spec file structure

A spec file is plain graph-specs-dialect markdown. The template below
follows the same structure as `specs/concepts/core.md` in the
graph-specs-rust repository:

```markdown
---
crate: <crate-name>
rfc: RFC-030
status: draft | approved
---

# Spec: <crate-name>

One sentence describing the crate's bounded responsibility.

## ConceptName

One sentence describing what this type is. Prose is ignored by the
reader but is load-bearing for humans and agent sessions reading the
spec before starting work.

## PortName

Description of a port trait — what it abstracts and who implements it.

```rust
pub trait PortName {
    fn method(&self, arg: ArgType) -> Result<ReturnType, ErrorType>;
}
```

(The fenced rust block is optional at concept level and reserved for
signature-level checking. Include it for port traits so the spec is
ready when signature-level is adopted.)

### SubConcept

A sub-heading (`###`) groups concepts that belong together. The reader
treats `##` and `###` identically as concept nodes; the hierarchy is
for human readability only.
```

### §6.3 Authoring rules

1. **One heading per public type.** Every top-level `pub struct`, `pub
   enum`, `pub trait`, `pub type` in a crate must appear as a `##` or
   `###` heading in its spec file. No omissions; no extras.
2. **Generics stripped in the heading.** `pub struct Graph<N, E>` →
   `## Graph`. The reader normalises this automatically, but authors
   should write stripped names to match what the diff will report.
3. **Fenced rust blocks for port traits.** Include the trait signature
   in a fenced ` ```rust ` block inside the trait's section. This is
   optional today (concept level) and will be diffed when signature
   level is adopted.
4. **Prose is for humans.** Write a short description under each
   heading. The reader ignores it; agent sessions read it before
   coding. A spec file with no prose is valid but is a missed
   opportunity to document the design rationale concisely.
5. **No rationale prose substitutes for structure.** A paragraph
   explaining why a type exists does not satisfy the heading
   requirement. The heading must exist even if it has no prose.
6. **Spec and code change together.** A PR that adds or removes a
   public type must update the spec in the same commit. A PR that renames
   a type must rename the heading and the type atomically.
7. **RFC citation in frontmatter.** The `rfc:` frontmatter field names
   the RFC that ratified the concept boundary. Frontmatter is ignored by
   graph-specs but is load-bearing for traceability; do not omit it.

---

## §7 Consequences

### §7.1 What improves

- **Self-consistent documentation.** cfdb's architecture is documented
  at two levels: narrative (RFC docs) and contract (specs). A new
  contributor or agent session reads the spec to understand crate
  boundaries in seconds and the RFC to understand why they exist.
- **Regression prevention.** The CI gate makes concept drift visible at
  PR time, not at code-review time. A reviewer no longer needs to hold
  the entire crate dependency graph in their head.
- **Dogfood credibility.** A tool that cannot pass its own gates is not
  credible. cfdb passing both the graph-specs gate and its own self-audit
  is the minimum viable proof that the toolchain works.
- **Bootstrapped spec library.** Task #4 produces six spec files that
  serve as reference implementations for workspaces adopting cfdb.
- **Agent session acceleration.** Per the graph-specs README: "Session N
  cleans up a split-brain and specs the context. Session N+1 reads the
  spec, understands what exists, and wires into it instead of creating a
  parallel implementation." The spec is durable architectural memory.

### §7.2 What it costs

- **CI time.** `cfdb extract --workspace .` on the cfdb workspace is
  fast (six small crates, no HIR dependency in v0.1). Estimated
  addition: +15–30s on a warm cache. `graph-specs check` adds another
  5–10s. Total CI budget increase: under 45s. Acceptable.
- **Spec authoring burden.** Task #4 requires architects to write six
  spec files. This is a one-time cost amortized over the life of the
  project. Spec updates on subsequent PRs are small (one heading added
  per new public type).
- **Learning curve.** Contributors unfamiliar with the dialect must read
  `specs/dialect.md` and `specs/concepts/core.md` from the
  graph-specs-rust repository before authoring spec entries. Both are
  short; the dialect is intentionally minimal.
- **False positives on moves.** The concept-level check cannot
  distinguish a type being moved from a type being deleted plus a new
  type added. A PR that moves a type across crates will produce two
  violations (missing from old spec, undeclared in new spec) unless the
  spec and code change atomically. Mitigation: the §6.3 authoring rules
  mandate atomic commits.

### §7.3 Who authors specs

Architects write specs when ratifying RFCs. The council that approved
RFC-029 (and this RFC-030) is the authoring body for the initial six
specs. For subsequent crates (e.g., `cfdb-hir-extractor` introduced by
RFC-029 §A1.2), the RFC that introduces the crate must include a
corresponding spec as a required deliverable.

### §7.4 What breaks if graph-specs changes its dialect

graph-specs `develop` is the authoritative dialect reference. cfdb CI
pins to a specific released binary, not `develop`, so dialect upgrades
are opt-in. When a new graph-specs release ships a breaking dialect
change, cfdb adopts it via the §5.3 upgrade path: a dedicated RFC,
updated spec files, CI passing before merge.

---

## §8 Acceptance gates

This RFC is satisfied when **all five** of the following are true:

| # | Gate | Measurable |
|---|---|---|
| G1 | `specs/concepts/` contains one approved spec file per workspace crate (six files: cfdb-core, cfdb-query, cfdb-petgraph, cfdb-extractor, cfdb-recall, cfdb-cli) | `ls specs/concepts/*.md \| wc -l` = 6 |
| G2 | `graph-specs check --specs specs/concepts/ --code crates/` exits 0 on the main branch | CI run link in the PR merging Task #4 output |
| G3 | `cfdb extract --workspace . --db .cfdb/db --keyspace cfdb-self` followed by `cfdb violations --db .cfdb/db --keyspace cfdb-self --rule <rule>` for each rule in `examples/queries/` exits 0 (no blocking violations) on the main branch | CI run link in the PR merging cfdb self-audit gate |
| G4 | The CI workflow file (`ci/`) invokes both gates (§3.1 and §3.2) and fails the build on non-zero exit | Verified by a PR that introduces a deliberate spec violation, confirms CI blocks, then reverts |
| G5 | No allowlist or ratchet file exists in the repository | `grep -r "expected_violations" . \| wc -l` = 0 and `find . \( -name "*.allowlist" -o -name "*-baseline.json" \) \| wc -l` = 0 |

Gates G1 and G2 are satisfied by Task #4 (specs committed on this
branch). Gate G3 is blocked until the cfdb violations CI step is wired
(tracked by a follow-up issue). Gate G4 is blocked until G2 and G3 pass.
Gate G5 is a continuous invariant, not a one-time check.

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
  new debt of the same kind from accumulating after the cleanup lands.

### External

- `graph-specs-rust` repository (`yg/graph-specs-rust`, `develop`
  branch) — the vaccine tool.
  - `README.md` — purpose, the four equivalence levels, use cases
    including the "cfdb pattern" pairing.
  - `specs/dialect.md` — the machine-parseable spec format. Authoritative
    reference for what the markdown reader parses and ignores.
  - `specs/concepts/core.md` — the canonical worked example of a
    concept-level spec file. The initial cfdb specs in `specs/concepts/`
    follow the same structure.
- CLAUDE.md §6 rule 8 — "no metric ratchets" rule. Applies to the
  spec compliance gate: no allowlist, no ceiling, no waiver mechanism.
- CLAUDE.md §4 — the RFC-to-spec-to-issue-to-impl workflow codified
  in §3.3 of this RFC is an application of the outside-in development
  methodology documented there.
