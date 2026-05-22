# RFC-042 council brief — test/bench :EntryPoint kinds + scope --production-only

**Status:** PENDING — convened 2026-05-17 against `docs/RFC-042-test-bench-entry-points.md`
(merged via PR #379, currently DRAFT at §5).

**RFC SHA on develop:** `8ee73fc` (RFC text), `53c4daf` (HEAD).

**Convener:** captain (a0 session 2026-05-17, worktree `work/378-test-binaries-as-entry`)

**Originating issue:** [`yg/cfdb#378`](https://agency.lab:3000/yg/cfdb/issues/378)

---

## 1. What you are ratifying

The RFC text is at `docs/RFC-042-test-bench-entry-points.md` in this worktree. **Read it in full
before rendering a verdict.** Council MUST cite RFC §section markers in evidence — generic
"looks fine" is not a verdict.

Short summary:

1. Extend `:EntryPoint.kind` with two open-enum variants — `"test"` and `"bench"`.
2. `cfdb-hir-extractor` detects them via attribute probes (`#[test]`, `#[tokio::test]`,
   cucumber `#[given]/#[when]/#[then]`, `#[bench]`) PLUS file-location fallback (`tests/`,
   `benches/`).
3. `cfdb scope` gains `--production-only` flag; `enrich_reachability` runs the BFS twice
   (Option (A) in §3.3) to materialize parallel `reachable_from_production_entry` attr.
4. New `classifier-unwired-production.cypher`; classifier rule switched at the
   orchestrator layer based on the flag.
5. SchemaVersion stability invariant: kind is open-set, doc-only edit at
   `crates/cfdb-core/src/schema/describe/nodes.rs:296`; no `yg/graph-specs-rust`
   lockstep bump required.
6. Three vertical slices in §7: 042-A (extractor + fixture), 042-B (scope flag + dual-BFS
   + classifier rule), 042-C (empirical close-out on `qbot-core`).

---

## 2. Why this RFC matters operationally

The bug it fixes is documented and measurable: `cfdb scope --context trading --keyspace qbot-core`
reports **2057 unwired items**. Spot-audit of the first hundred shows ≥38% are reachable from
test code (`#[test]`, BDD step defs, `#[bench]`) and only "unreachable" because
`cfdb-hir-extractor` recognizes only `{cli_command, mcp_tool, http_route, cron_job, websocket}`
as entry-point kinds.

Downstream consumers `/sweep-epic` and `/operate-module` consult `cfdb scope`; the false-positive
rate poisons their inventory-driven cleanup, producing revert storms (a tested-and-benchmarked
broker classified "unwired" gets deleted; next release regression-tests fail).

The RFC-029 v0.2 §A2 distribution prediction said "unwired 4%"; qbot-core measures 24% — the
under-counting prediction is the on-ramp this RFC fixes.

---

## 3. Council scope — what we are deciding

Per CLAUDE.md §2.3 each lens renders a verdict (`RATIFY` / `REQUEST CHANGES` / `REJECT`) with
evidence, plus prescribes the `Tests:` 4-row block for each slice (`Unit`, `Self dogfood`,
`Cross dogfood`, `Target dogfood`). Per CLAUDE.md §2.5 the `Cross dogfood` row exists because
cfdb and graph-specs-rust are a paired toolchain.

**Convener note:** the operator has explicitly named the dual-dogfood discipline as a council
deliverable. Each lens MUST therefore answer:

### 3.1 Standard lens question (per the table in CLAUDE.md §2.3)

| Lens | Question |
|---|---|
| `clean-arch` | StoreBackend trait purity, crate dependency direction, composition root |
| `ddd-specialist` | Node/Edge vocabulary, bounded contexts, homonym detection on `:EntryPoint{kind}` |
| `solid-architect` | Crate granularity, SRP on evaluator vs extractor, stable abstractions for `cfdb-core` |
| `rust-systems` | syn / ra_ap_hir parsing strategy, petgraph internals, feature flags, trait object safety |

### 3.2 Cross-cutting deliverables (every lens contributes)

**D1. Verdict** on the RFC as written. Cite RFC §sections.

**D2. Tests prescription** for each slice (042-A, 042-B, 042-C). 4-row block per slice.
Lens prescribes the row content most relevant to its perspective; other rows may be marked
`(defer to <other-lens>)` and the convener synthesizes.

**D3. Dual-dogfood proof discipline.** For slice 042-A and 042-B, articulate the EXACT
shape of:
- `Self dogfood (cfdb on cfdb)` — what query result on cfdb's own keyspace proves the
  feature works. Be concrete: name the Cypher, the expected lower-bound count, the
  rationale.
- `Cross dogfood (cfdb on graph-specs-rust at pinned SHA)` — what regression on
  `yg/graph-specs-rust` `.cfdb/queries/*.cypher` proves the feature is zero-false-positive
  against the companion at its current pinned SHA. The RFC §4 invariant says SchemaVersion
  does NOT bump, so this should be a no-op regression check, not a SchemaVersion-lockstep
  follow-up.

**D4. Graph-specs-rust update against real code (convener-mandated council deliverable).**
The new `:EntryPoint{kind ∈ {test, bench}}` distinction enables a NEW class of anti-drift
rule that graph-specs-rust could ship in its own `.cfdb/queries/`: "production code reachable
only from test entry points = dead-in-production." Each lens proposes ONE concrete Cypher
query (or schema check) that graph-specs-rust could add, expressed from its perspective:

- `clean-arch` — a layer-purity rule (e.g., domain code reached only from tests = misplaced
  layer marker).
- `ddd-specialist` — a vocabulary rule (e.g., aggregate-root methods reached only from test
  drivers = anaemic aggregate).
- `solid-architect` — an SRP / abstraction rule (e.g., trait impls with no production caller
  but heavy test surface = leaked abstraction).
- `rust-systems` — a feature-flag / trait-object rule (e.g., `dyn Trait` constructions whose
  only `impl` is reached only from tests = unused vtable entry, fixable to monomorphisation).

Each proposal MUST:
1. Be expressible in cfdb's current Cypher subset (no DSL extensions).
2. Cite a concrete file:line in `agency:yg/graph-specs-rust` at its current pinned SHA
   (`.cfdb/cross-fixture.toml`) where the rule would either fire OR demonstrate zero-finding.
3. Note whether the rule is intended as zero-violation policy on graph-specs-rust (i.e.
   the rule lands AND graph-specs-rust ships clean), or is intended to find existing
   findings that need cleanup (i.e. the rule lands AS PART OF a graph-specs-rust cleanup PR).

The convener will synthesize the four proposals into at most ONE recommended graph-specs-rust
follow-up PR, filed against `yg/graph-specs-rust` after RFC-042 implementation lands on cfdb.

### 3.3 Out of scope for council

- Re-litigating §6 non-goals (criterion_group! macro, HTTP/cron parity in tests, separate
  `:Test`/`:Bench` labels). These are author-decided and listed for completeness — REJECT
  is appropriate only if the lens argues the non-goal is load-bearing for the RFC's
  correctness, which is a high bar.
- SchemaVersion strategy. §4 invariant fixes this.
- Whether to ship at all. The 2057-unwired finding is operational evidence; council debates
  shape, not necessity.

---

## 4. Reference material

- **RFC text:** `docs/RFC-042-test-bench-entry-points.md` (this worktree).
- **Predecessor RFCs:**
  - RFC-029 (v0.2 :EntryPoint vocabulary, the under-counting prediction).
  - RFC-032 (v0.2 extractor, the `scan_file` dispatch shape this extends).
  - RFC-037 (schema-producer alignment, textual-attribute heuristic contract).
- **Touch sites named in RFC:**
  - `crates/cfdb-hir-extractor/src/entry_point_emitter.rs` (FN dispatch branch — currently
    lines 173-188 emit `mcp_tool` only).
  - `crates/cfdb-hir-extractor/src/entry_point_emitter/registers_param.rs` (probe helpers,
    where `has_test_attr` / `has_bench_attr` would live alongside `has_tool_attr`).
  - `crates/cfdb-core/src/schema/describe/nodes.rs:296` (kind enum descriptor text).
  - `crates/cfdb-petgraph/src/enrich/reachability.rs:80-91` (degraded-path warning preserved).
  - `crates/cfdb-cli/src/scope.rs::scope` (CLI signature gains `production_only: bool`).
- **Companion:** `yg/graph-specs-rust` `.cfdb/cross-fixture.toml` pins cfdb at a SHA;
  pin is NOT bumped by this RFC (open-enum, no SchemaVersion change).
- **Operational consumers harmed by the bug:** `/sweep-epic`, `/operate-module`.
- **Methodology refs:** `CLAUDE.md` §1 (RFC-first), §2.3 (council), §2.5 (Tests template),
  §3 (dogfood enforcement), §5 (schema discipline).

---

## 5. Verdict format (write to `council/RFC-042/verdicts/<lens>.md`)

```markdown
# RFC-042 verdict — <lens-name>

**Verdict:** RATIFY | REQUEST CHANGES | REJECT
**Author:** <lens-name> sub-agent
**Date:** 2026-05-17

## D1. Verdict on the RFC as written

<2-4 paragraphs, citing RFC §sections. If REQUEST CHANGES, enumerate change requests with
RFC §section + proposed edit shape.>

## D2. Tests prescription

### Slice 042-A — extractor :EntryPoint{kind=test|bench} + fixture
- **Unit:** <pure-function assertions; what shape of test, what coverage>
- **Self dogfood (cfdb on cfdb):** <Cypher query + expected lower-bound on this repo>
- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA):** <zero-regression assertion shape>
- **Target dogfood (qbot-core at pinned SHA):** <PR-body metric reporting shape>

### Slice 042-B — scope --production-only + dual-BFS + classifier rule
- **Unit:** <…>
- **Self dogfood:** <…>
- **Cross dogfood:** <…>
- **Target dogfood:** <…>

### Slice 042-C — empirical close-out on qbot-core
- **Tests:** none — rationale: cross-repo empirical report, not code. (Per RFC §7.)

## D3. Dual-dogfood discipline notes

<Any lens-specific concerns about the Self/Cross dogfood prescriptions above. E.g.
"the Self dogfood lower-bound count is fragile because <reason>; suggest <alternative>."
Empty if no concerns.>

## D4. Graph-specs-rust update against real code

**Proposed Cypher (one):**
```cypher
<rule body>
```

**Filed at (or proposed for):** `.cfdb/queries/<rule-name>.cypher` on `yg/graph-specs-rust`.

**Citation against current graph-specs-rust pinned SHA:** <file:line OR "zero-finding
expected — rule is preventative policy">.

**Intent:** zero-violation policy from day one | cleanup-driving (finds existing findings)

**Rationale (one paragraph):** <why this lens proposes this specific rule; cite an
anti-drift pattern from the lens's perspective.>
```

---

## 6. Deliberation procedure

1. Each lens reads RFC-042 in full + this brief.
2. Each lens writes its verdict file to `council/RFC-042/verdicts/<lens>.md` (this worktree).
3. After all four verdicts are written, the convener synthesizes into
   `council/RFC-042/SYNTHESIS-R1.md`.
4. If all four are RATIFY, the convener edits RFC-042 §5.1-5.4 to record verdicts and
   writes `council/RFC-042/RATIFIED.md`. Slice issues 042-A/B/C are then filed per §7.
5. If any lens is REQUEST CHANGES, the RFC author (captain) addresses the change requests,
   the affected lens re-reviews, and round 2 begins. RFC is NOT ratified until all four
   are RATIFY (or a single author-documented override is recorded in `RATIFIED.md`).
6. If any lens is REJECT and the rejection is load-bearing (RFC's correctness depends on
   the rejected element), the RFC is sent back to drafting; convener pings the operator
   for direction.

---

## 7. Convener's anti-bias notes for the council

- **Discovery bias check.** RFC-042 was authored before any discovery artifact was written.
  No `.discovery/378.md` exists. The RFC's §3.1 touch-site claim
  (`entry_point_emitter.rs:173-188`) WAS verified by the convener — the FN dispatch branch
  currently emits only `mcp_tool`. Lenses do not need to re-verify; they may cite the
  RFC's file:line references as authoritative for current state.

- **Decision archaeology already in the package.** `.context/378.md` carries the routing
  decision: implementation work belongs to slice issues 042-A/B/C, not directly to #378.
  Issue #378 itself becomes the 042-C empirical close-out. Lenses do not need to debate this.

- **Self-certification trap.** The RFC author and the convener are the same captain.
  Lenses are the independent check on confirmation bias. **If a lens sees a problem the
  RFC text minimises or hand-waves, REQUEST CHANGES is the right verdict — do not soften
  it because the captain wrote the text.**

- **Cross dogfood is not theatre.** Per CLAUDE.md §3 + RFC-033, every ban-rule or
  schema-touching change is contracted to be zero-false-positive against graph-specs-rust
  at its pinned SHA. The RFC §4 invariant says SchemaVersion does not bump, which means
  the cross dogfood SHOULD be a no-op regression — but `enrich_reachability` writes two
  new `:Item` attributes (`reachable_from_production_entry`, `reachable_production_entry_
  count`) on every keyspace including the graph-specs-rust one. Lenses MUST think through
  whether ANY query in `yg/graph-specs-rust/.cfdb/queries/*.cypher` reads these attribute
  names — if yes, behavior changes on the companion side too. The convener has not
  pre-checked this; flag if found.

---

End of brief.
