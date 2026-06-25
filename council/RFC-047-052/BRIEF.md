# Council BRIEF — RFC-047..052 (Understand-Anything borrow batch)

**Convened:** session 2026-06-25 (continuation). **Base:** `origin/develop` @ `ed27cd0`.
**Mechanism:** agent-team council (4 lens teammates, mailbox + shared task list) per `CLAUDE.md §2.3` + global `§2b`. Teammates challenge each other directly; this is not isolated fan-out.
**Inputs under review:** `docs/RFC-047..052`, grounded by `studies/002-borrowed-from-understand-anything.md` (the borrow memo) and `studies/003-cfdb-understand-discovery.md` (the dogfood discovery).

---

## 1. What this council decides

Six draft RFCs borrowed from `Understand-Anything` (UA). They are **not** equal in maturity. Your verdict scope per RFC:

| RFC | Title | Disposition sought | Verdict vocabulary |
|---|---|---|---|
| 047 | `cfdb impact` / blast-radius | **Full review** — lowest risk, no schema | RATIFY / REQUEST CHANGES / REJECT |
| 048 | Incremental extraction (profile-first) | **Full review** — but likely ratify only 48-A (the profile); 48-B/C are conditional | RATIFY / REQUEST CHANGES / REJECT, *per slice* |
| 049 | Framework-aware entry-point detectors | **Full review** — reuses `:EntryPoint`, no schema in v1 | RATIFY / REQUEST CHANGES / REJECT |
| 050 | Architectural-layer (tier) overlay | **Full review** — the only schema-bumping RFC; rests on a missing edge | RATIFY / REQUEST CHANGES / REJECT |
| 051 | Non-code / IaC / DDL extraction | **Triage only** — author parked it (no consumer + no recall ground-truth) | KEEP-PARKED / KILL / UNPARK-WITH-CONDITIONS |
| 052 | Opt-in LLM enrichment (`summary`/`tags`) | **Triage only** — changes cfdb's deterministic *character*; needs the **maintainer's** blessing, not the architects' | KEEP-PARKED / KILL + harden-the-mechanism-conditionally |

**On 052 specifically:** the architects do **not** ratify direction here. The go/no-go on admitting a non-deterministic layer into cfdb is a charter decision reserved to the maintainer (`RFC-052 §1`). Your job is (a) confirm the *fence* (G1 exclusion) is sound IF blessed, and (b) recommend keep-parked vs. kill. Do not return RATIFY for 052.

A full RFC is ratified only when **all four lenses RATIFY** (or a single author-documented override is recorded in `RATIFIED.md`, `CLAUDE.md §2.3`). Until then it stays DRAFT and no `forge_create_issue` happens.

---

## 2. House rules (binding on every verdict)

1. **Verify every factual claim against `file:line`.** The RFCs cite cfdb internals; do not trust them — open the file. The verified-facts appendix (§4) is your starting set, but extend it. A verdict resting on an unverified claim is invalid (see memory: *council foundation claims need verification*).
2. **No self-certification.** You analyse and vote; you do not implement. (`CLAUDE.md global §5`.)
3. **You also prescribe tests.** Each issue slice in a ratified RFC's §7 carries a 4-row `Tests:` block (Unit / Self-dogfood / Cross-dogfood / Target-dogfood, `CLAUDE.md §2.5`). If a slice's block is wrong or a row should be `none — rationale:`, say so in your verdict — implementers deliver against your prescription.
4. **No metric ratchets, ever** (`CLAUDE.md global §6 rule 8`). If any RFC smuggles a baseline/ceiling/allowlist for a metric, that is a REJECT on sight.
5. **Schema discipline.** Any new node/edge label, attribute, or `SchemaVersion` bump drags a lockstep `graph-specs-rust` `.cfdb/cross-fixture.toml` PR (`CLAUDE.md §3`/§5). Flag every schema-surface change and whether the RFC acknowledges the lockstep.
6. **Stubs are not arrows** (memory). A `_synthesized`/`_stub`/`_external` discriminator means two concepts were conflated. RFC-049 §3.3 claims to honour this — verify.
7. **Tool backlog ≠ client chores** (memory). cfdb owns the agnostic capability + its own dogfood + the graph-specs lockstep — *not* a downstream client's adoption. RFC-051's parked-on-no-consumer status and RFC-047/049's "cfdb returns node sets, rendering is the consumer's job" rest on this. Verify nothing leaks a client concern into cfdb's surface.

---

## 3. The contested questions (where you MUST challenge each other)

Independent per-lens verdict files are Phase A. Phase B is **direct cross-challenge by mailbox** on these load-bearing questions. Each has named lenses who must engage:

- **Q1 — RFC-050 sourcing: enrich-time Cargo read (resolution A) vs. materialise a `DEPENDS_ON` crate edge (resolution B).** `§3.1`. cfdb has **no crate→crate edge today** (verified §4.2). A is the smallest surface (one attribute); B is richer but a bigger bump and arguably its own RFC. *Engage:* **rust-systems ⇄ solid-architect ⇄ clean-arch.** (rust-systems: longest-path tier computation + acyclicity; solid: SDP/SAP of materialising the DAG; clean-arch: where the `Cargo.toml` read lives.)
- **Q2 — Verb ceiling.** `EnrichBackend` is **closed at 7 verbs** (verified §4.1). RFC-050 §3.3 wants to *extend* `enrich_bounded_context` to also emit `tier`; RFC-052 §3.2 wants to fold `summary`/`tags` into an existing verb rather than add `enrich_summaries`. Is extending an existing verb the right OCP move, or a god-pass / SRP violation? *Engage:* **solid-architect ⇄ clean-arch.**
- **Q3 — RFC-048 is the profile feasible AND is incremental enrichment even possible under G1?** The reframe says parsing is ~0.575 s, so the lever is incremental *enrichment* (reachability BFS, dup-cluster, recall) — but those are *global* facts and `G1` demands byte-identical dumps. Is 48-B a real engineering target or a dead end that makes "cache the rustdoc JSON" the true RFC? *Engage:* **rust-systems (lead) ⇄ clean-arch (where the cache/instrumentation lives) ⇄ ddd (fingerprint is a build mechanism, not a schema concept — confirm).**
- **Q4 — RFC-049 registry placement.** Where does the `FrameworkDetector` registry live — `cfdb-extractor` (Rust), the per-language extractor crates, or a shared seam? Detectors must not reach across language-extractor boundaries. Is registering a detector the OCP extension point (vs. editing a match arm)? *Engage:* **clean-arch ⇄ rust-systems ⇄ solid-architect.** (rust-systems also owns: does `clap`-derive recognition read the *derive input* via `syn`, not macro-expanded output cfdb doesn't have? `§3.2`/`§5`.)
- **Q5 — DDD concept-ownership across 049 + 050.** Is "tier/layer" a concept cfdb's bounded context **owns**, or extraction-time provenance? Same for "framework". Run the **split-brain test**: `:Context` = ownership; `tier` = architectural role — genuinely orthogonal, or does one imply the other? `RFC-050 §3.4`. Homonym check: `:Layer`/`tier` vs. UA's generic web layers; framework "route" vs. the existing HTTP `:EntryPoint` kind. *Engage:* **ddd-specialist (lead), all others respond.**

If a challenge converges, record the agreed position in your file. If it does not, record the disagreement explicitly — the lead synthesises and may run an R2.

---

## 4. Verified facts (lead-checked against `develop` @ `ed27cd0` — extend, don't trust)

### 4.1 Verb ceiling — CONFIRMED
`crates/cfdb-core/src/enrich.rs:91` `pub trait EnrichBackend`. Exactly **7** `enrich_*` methods: `enrich_git_history` (`:100`), `enrich_rfc_docs` (`:115`), `enrich_deprecation` (`:130`), `enrich_bounded_context` (`:144`), `enrich_concepts` (`:163`), `enrich_reachability` (`:177`), `enrich_metrics` (`:189`). The "closed at 7" rule is methodology (`RFC-031 §2`) — adding an 8th needs explicit council blessing. `StoreBackend` is at `crates/cfdb-core/src/store.rs:63`.

### 4.2 No crate→crate dependency edge — CONFIRMED (RFC-050's prerequisite is real)
`crates/cfdb-core/src/schema/labels.rs` edge constants: `IN_CRATE` (`:118`, item→crate), `IN_MODULE`, `HAS_FIELD/VARIANT/PARAM/CONST_TABLE`, `TYPE_OF`, `IMPLEMENTS`, `IMPLEMENTS_FOR`, `RETURNS`, `BELONGS_TO` (`:132`, crate→context), `CALLS` (`:135`), `INVOKES_AT` (`:136`), `EXPOSES` (`:139`), `REGISTERS_PARAM` (`:140`), `LABELED_AS`, `CANONICAL_FOR`, `EQUIVALENT_TO`, `REFERENCED_BY`, `HAS_ARG`. **There is no `crate -[DEPENDS_ON]-> crate` edge.** `:Crate` node exists (`labels.rs:25`). RFC-050 §3.1 must resolve how tiers are sourced (Q1).

### 4.3 `:EntryPoint` surface — CONFIRMED
`labels.rs:50` `ENTRY_POINT = "EntryPoint"`; edges `EXPOSES` (`:139`), `REGISTERS_PARAM` (`:140`). Attributes/kinds described in `crates/cfdb-core/src/schema/describe/nodes/structural.rs` + `.../call_graph.rs` + `descriptors.rs` — verify the exact `kind`/`handler_qname`/`name`/`params` shape RFC-049 reuses.

### 4.4 Provenance — CONFIRMED `#[non_exhaustive]`
`crates/cfdb-core/src/schema/descriptors.rs:25` `enum Provenance`: `Extractor`, `EnrichRfcDocs`, `EnrichMetrics`, `EnrichGitHistory`, `EnrichConcepts`, `EnrichReachability`, `Reserved`. **No `EnrichLlm`** — RFC-052 proposes it as a new variant. `test_coverage` is an `EnrichMetrics` attribute (`:40`) — RFC-052 §3.1 claims a "G6-style G1-exclusion" precedent here; **verify the actual exclusion mechanism** in `specs/concepts/cfdb-core.md` (the `G6` clause) and the determinism dump path before accepting that the fence works.

### 4.5 Studies as ground truth for the borrow
- `studies/003 §2` derives cfdb's real crate DAG (tier 0 `cfdb-core` → tier 3 `cfdb-cli`, acyclic). RFC-050's tier semantics must match this.
- `studies/003 §1`: UA parsed cfdb's 352-file code tree in **0.575 s** — the evidence that reframed RFC-048 away from parse-skip.
- `studies/002 §2`: the table of what cfdb already has (don't re-invent) and §4 (explicitly excluded: edge weights, LLM auto-fix, tours, embeddings — these are off-charter and must not creep back in).

---

## 5. Output contract

Write your verdict to `council/RFC-047-052/<lens>.md` where `<lens> ∈ {clean-arch, ddd-specialist, solid-architect, rust-systems}`. Structure:

```
# <Lens> verdict — RFC-047..052

## Verdict table
| RFC | Verdict | One-line reason |
| 047 | RATIFY / REQUEST CHANGES / REJECT | ... |
| 048-A | ... | (per-slice for 048) |
| ...
| 051 | KEEP-PARKED / KILL / UNPARK-IF | ... |
| 052 | KEEP-PARKED / KILL (never RATIFY) | ... |

## Per-RFC analysis
<for each RFC in your purview: the hardened §5 lens content — your position, evidence at file:line,
 the genuine open question, any REQUEST-CHANGES condition stated as a concrete, checkable amendment>

## Contested-question positions (Q1..Q5 you engaged)
<your position + who you challenged + the outcome (converged/disagreed)>

## Test-surface prescription notes
<any correction to a slice's 4-row Tests: block>
```

**Be concrete.** A REQUEST CHANGES must name the exact amendment that would flip it to RATIFY (the RFC-046 council's standard: e.g. "drop `&Path` from the core port" → applied → RATIFY). Cite `file:line` for every claim about cfdb internals.

When done: `SendMessage` to `main` with your verdict table + a one-paragraph summary, and mark your task complete.
