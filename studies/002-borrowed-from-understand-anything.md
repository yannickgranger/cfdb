# Study 002 — Capabilities worth borrowing from `Understand-Anything`

**Status:** exploratory backlog memo (feeds the RFC pipeline; nothing here is ratified).
**Author:** session 2026-06-25.
**Source under study:** [`Egonex-AI/Understand-Anything`](https://github.com/Egonex-AI/Understand-Anything) (plugin v2.8.1, MIT), cloned to `~/workspaces/Understand-Anything`, built and dogfooded against cfdb's own tree (Node 22 + pnpm 10).
**Purpose:** the user asked to "understand how it understands" and "enrich the cfdb backlog with smart features borrowed from this repo." This memo records the mechanism, maps it onto cfdb's *current* vocabulary, and proposes a ranked set of **RFC candidates**. Per `CLAUDE.md §1`, new capability is RFC-first — so this is a backlog of RFC topics, **not** a set of forge issues. Each candidate must clear the architect council before any issue is filed.

---

## 1. How `Understand-Anything` (UA) understands

UA is a **Claude-Code (multi-agent) plugin**, not a binary. Its "understanding" is a two-layer sandwich:

**Layer 1 — deterministic floor.** A TypeScript core (`packages/core`) parses ~12 code languages with **Tree-sitter** and ~12 **non-code** formats with hand-written regex/brace parsers (Dockerfile, docker-compose, Kubernetes, Terraform, SQL DDL, GraphQL, OpenAPI, Protobuf, YAML, GitHub Actions, env, Makefile). Output: structural facts — functions/classes/imports/exports/call-graph for code; services/resources/tables/endpoints/pipelines for infra. A **framework registry** (React/Express/Django/Rails/Spring/…) deterministically maps manifest keywords → layer hints + entry-point patterns. **Structural fingerprints** (SHA-256 + signature comparison) drive a **change-classifier** for incremental re-analysis.

**Layer 2 — LLM ceiling.** A 6-agent pipeline (`project-scanner → file-analyzer → architecture-analyzer → domain-analyzer → tour-builder → graph-reviewer`) adds what static analysis can't: plain-English **summaries**, semantic **tags**, **complexity** ratings, a business **domain→flow→step** hierarchy, **architectural layers**, **guided tours**, and inferred semantic edges. A Zod schema + an alias table (`func→function`, `extends→inherits`, …) + a 4-tier auto-fix launders non-deterministic LLM output into a canonical `knowledge-graph.json`.

**Graph shape:** 22 node types (`file, function, class, module, concept, config, document, service, table, endpoint, pipeline, schema, resource, domain, flow, step, article, entity, topic, claim, source`) and 35 edge types across 8 categories (structural / behavioral / data-flow / dependency / semantic / infrastructure / schema-data / knowledge), each edge carrying a `direction` and a `weight ∈ [0,1]`.

**The key contrast with cfdb:** UA is **LLM-first, presentation-oriented, non-deterministic**; cfdb is **deterministic-first, recall-gated against rustdoc, sha256-byte-stable** (`G1`). So most of UA's *ideas* are borrowable, but a borrowed idea is only on-charter for cfdb if it can be made deterministic (or explicitly fenced as an opt-in, `G1`-excluded enrichment).

---

## 2. What cfdb already has (don't re-invent)

cfdb's current vocabulary (`specs/concepts/cfdb-core.md`) already covers — and in several places **exceeds** — UA's graph:

| UA capability | cfdb equivalent (already shipped) |
|---|---|
| `function`/`class`/`module`/`file` nodes | `:Item` (kind: struct/enum/trait/impl/fn/const/…), `:Module`, `:File`, `:Crate` |
| `imports`/`contains`/`inherits`/`implements`/`calls` edges | `IN_MODULE`/`IN_CRATE`, `HAS_FIELD`/`HAS_VARIANT`/`HAS_PARAM`, `IMPLEMENTS`/`IMPLEMENTS_FOR`, `CALLS` |
| call-site granularity | `:CallSite` + `INVOKES_AT` + `:Argument`/`HAS_ARG` (RFC-043/045) — finer than UA's edge-level `calls` |
| `endpoint`/entry detection | `:EntryPoint` (kind: MCP/CLI/HTTP/cron) + `EXPOSES` + `REGISTERS_PARAM` |
| `domain`/architectural grouping | `:Context` (bounded context) + `BELONGS_TO`; `:Concept` + `LABELED_AS`/`CANONICAL_FOR` |
| `complexity` rating | `:Item.cyclomatic` (a real metric, not an LLM guess) |
| graph validation / auto-fix | the **recall gate** (`cfdb-recall` vs `rustdoc --output-format=json`) — a stronger, ground-truthed analog |
| doc summaries | `:Item.doc_text` (the actual doc comment, verbatim) |

cfdb **also** has facts UA has no concept of: reachability BFS (`reachable_from_entry`, `reachable_from_production_entry`), git churn (`git_commit_count`, `git_last_author`), `unwrap_count`, `dup_cluster_id`, `deprecation_since`, `cfg_gate`, `visibility`, `signature_hash`. **cfdb is the more rigorous graph; UA is the more approachable one.** The borrow target is UA's *breadth and ergonomics*, not its core model.

---

## 3. RFC candidates (ranked)

Ranking = (value × on-charter fit × consumer pull). Each candidate names the invariants it must honour: `G1` byte-stability, recall ground-truth, the `SchemaVersion` + graph-specs lockstep (`CLAUDE.md §3/§5`), the **closed-at-7 `EnrichBackend`** surface, and the **11-verb API ceiling**.

### C1 — `cfdb impact` / blast-radius query  ⭐ top pick
- **Borrowed from:** UA `understand-diff` (changed files → trace edges → affected components).
- **Problem:** "If I change `fn X`, what transitively breaks?" is the single most-asked code-graph question and cfdb already holds every fact to answer it — it just has no first-class affordance.
- **cfdb mapping:** a **query/CLI affordance**, *not* new facts and *not* a new trait verb. Walk `(:Item)<-[:CALLS*]-(caller)` (optionally bounded by `INVOKES_AT`/`:CallSite`) from items whose `signature_hash` changed between two `git` refs, intersected with `reachable_from_production_entry`. Ships as a canned query + a thin `cfdb impact --since <ref>` CLI wrapper.
- **Invariants:** read-only (`G2`); no schema change; no `EnrichBackend`/verb-ceiling impact (composes existing verbs per `RFC-036 §3`). Determinism trivially preserved.
- **Consumer pull:** **high** — qbot-core / agentry want "what's the blast radius of this PR" directly.
- **YAGNI/scope:** small. The risk is over-scoping into a diff renderer (a *client* concern) — keep cfdb's output to node sets + paths.

### C2 — Incremental extraction (fingerprint → change-classify → reuse)
- **Borrowed from:** UA `fingerprint.ts` + `change-classifier.ts` + `staleness.ts`.
- **Problem:** cfdb re-extracts the whole workspace every run; this doesn't scale to large monorepos (qbot-core, agentry).
- **cfdb mapping:** content-hash + `:Item.signature_hash` comparison to skip files whose structure is unchanged; `git diff <ref>..HEAD` to bound the candidate set. UA's `SKIP / PARTIAL / ARCHITECTURE / FULL` decision tree is the model.
- **Invariants — the hard one:** must preserve **`G1`**: an incremental extract MUST produce a byte-identical canonical dump to a full re-extract of the same workspace SHA. This is the make-or-break test and the recall corpus must assert it. Pure quality-driven (no consumer is blocked today), so it competes on engineering value, not pull.
- **Scope:** medium-large; touches the extractor's file-walk + a persistence delta layer.

### C3 — Framework-aware entry-point detectors
- **Borrowed from:** UA `framework-registry.ts` (per-framework `entryPoints` + `layerHints`).
- **Problem:** `:EntryPoint` recall depends on recognising framework idioms. cfdb hand-detects MCP/CLI/HTTP/cron; real targets use Axum/Actix/`clap`-derive (Rust), Symfony/Laravel (PHP), Nest/Express (TS) routing that cfdb may miss.
- **cfdb mapping:** a registry of deterministic, per-framework `:EntryPoint` detectors — **fits the existing label**, no new node type. Each detector is recall-gated (extractor ≡ a hand-curated fixture of that framework's routes).
- **Invariants:** additive recall extension; no schema bump if no new attributes. Honours the "stubs are not arrows" rule — only emit an `:EntryPoint` when a real handler is resolvable.
- **Consumer pull:** medium; improves every downstream "callers of / reachable-from-entry" query on framework-heavy targets.

### C4 — Deterministic architectural-layer overlay
- **Borrowed from:** UA `layer-detector.ts` (directory/path pattern → layer) + `architecture-analyzer`.
- **Problem:** cfdb groups by `:Crate`/`:Context` but has no *intra-crate* layer notion (api / service / data / ui / infra).
- **cfdb mapping:** a deterministic overlay — module-path/dir-pattern rules → a `:Layer` concept-style label (or extend `enrich_bounded_context`). Mirrors UA's regex pattern table; deterministic, no LLM.
- **Invariants:** if a new `:Layer` label, it's an OCP registration against the `nodes.rs` descriptor (open `Label` newtype) + minor `SchemaVersion` bump + **lockstep graph-specs PR**. Prefer reusing `:Concept`/`enrich_concepts` to avoid a new label (split-brain test: is "layer" a genuinely independent concept from "bounded context"? Probably yes — context = ownership, layer = role).
- **Consumer pull:** medium.

### C5 — Non-code / infrastructure-as-code + DDL extraction
- **Borrowed from:** UA's 12 non-code parsers (Dockerfile, Terraform, K8s, SQL DDL, GraphQL, OpenAPI, Protobuf, GitHub Actions, …) → `service`/`resource`/`table`/`pipeline`/`schema` nodes + infra edges (`deploys`/`serves`/`provisions`/`triggers`/`migrates`/`routes`/`defines_schema`).
- **Problem:** cfdb stops at code (Rust/PHP/TS). A real system's behaviour also lives in its IaC, CI, and schema files.
- **cfdb mapping:** new node labels (`:Resource`, `:Table`, `:Pipeline`, `:Service`) + infra edges, deterministic (regex/tree-sitter).
- **Invariants & blockers:**
  - **Recall ground-truth gap.** cfdb's recall gate is "extractor ≡ rustdoc-json." Non-code facts have **no rustdoc analog** — a new ground-truth (hand-curated fixtures per format) must be designed *before* these facts are trustworthy. This is the real cost.
  - Biggest schema surface of any candidate → multiple minor `SchemaVersion` bumps + graph-specs lockstep each.
- **Consumer pull:** **none named today.** Per the user's own boundary ("cfdb owns the agnostic capability + its own dogfood + the graph-specs companion lockstep only"), this should **not** be filed until a consumer (e.g. agentry needing deploy-topology facts) pulls it. Park as a capability vision, not a near-term RFC.

### C6 — (Charter-tension) opt-in LLM enrichment: `summaries` / `tags`
- **Borrowed from:** UA's headline feature — `file-analyzer` plain-English summaries + semantic tags.
- **Problem:** `doc_text` is only as good as the author's doc comments; UA-style summaries explain *why a thing matters*, which `cfdb chat`-style consumers would love.
- **cfdb mapping:** an **opt-in, `G1`-excluded** enrichment writing `:Item.summary` / `:Item.tags`, under a new `EnrichLlm` provenance — the exact pattern already used for `test_coverage` (toolchain-scoped, excluded from the canonical-dump sha256 per `G6`).
- **Invariants & blockers — read before pursuing:**
  - **Determinism:** fundamentally non-deterministic. Only admissible if excluded from `G1` (declare under a `G6`-style clause) and clearly marked in `SchemaDescribe`.
  - **Verb ceiling:** the `EnrichBackend` surface is **closed at 7**; an `enrich_summaries` verb breaches it and needs explicit council approval, OR it folds into an existing verb's internal decomposition.
  - **Philosophical:** this is the one borrow that changes cfdb's *character* (deterministic tool → tool-with-an-LLM-layer). **Should not proceed without the user's explicit blessing** and a full council pass. Flagged, not recommended-by-default.

---

## 4. Explicitly excluded (and why)

- **Guided tours / onboarding guides / the web dashboard.** These are **client/presentation concerns**. Per the user's standing boundary ("cfdb should not know about its clients") and the "tool backlog ≠ client chores" feedback, they belong in a *consumer* of cfdb, not in cfdb's backlog. cfdb should expose the facts (layers, reachability, entry points) that a tour-builder *consumer* would query.
- **Edge `weight ∈ [0,1]` floats.** Redundant with cfdb's deterministic `CALLS.resolved` discriminator. A confidence float invites a non-deterministic ratchet; the boolean resolution flag is the on-charter shape.
- **LLM graph auto-fix / alias normalisation.** Solves a problem cfdb doesn't have — cfdb's facts are deterministic and the recall gate is a stronger correctness signal than UA's Zod auto-fix.
- **Embedding cosine search.** Depends on a non-deterministic embedding model; a *consumer* concern. (A thin deterministic `cfdb search` fuzzy-name CLI over node `name`/`qname`, à la UA's Fuse.js layer, is a possible **minor** affordance but not a capability RFC.)

---

## 5. Recommended next steps

1. **C1 (`cfdb impact`) first** — highest leverage, zero schema risk, reuses shipped facts, real consumer pull. Draftable as an RFC immediately.
2. **C3 (framework entry-point detectors, RFC-049)** — natural recall-gated extension of an existing label.
3. **C4 (layer overlay, RFC-050)** — but it rests on a fact cfdb doesn't model yet: there is **no crate→crate `DEPENDS_ON` edge** (surfaced in [`studies/003`](003-cfdb-understand-discovery.md)). Resolve that prerequisite first, then the split-brain test vs `:Context`.
4. **C2 (incremental extraction, RFC-048) — REFRAMED + DEMOTED.** The UA discovery parsed cfdb's whole 352-file tree in **0.575 s**, so parsing is almost certainly *not* the bottleneck. RFC-048 is now profile-first: measure where `extract`'s wall-clock actually goes before building anything; the real lever (if any) is incremental *enrichment*, not parse-skip.
5. **C5 / C6** — **park.** C5 needs a named consumer + a non-rustdoc ground-truth design; C6 needs explicit user blessing because it changes cfdb's deterministic character.

Each surviving candidate goes through `docs/RFC-NNN-<slug>.md` → 4-lens architect council → ratified issue decomposition with the `Tests:` block (`CLAUDE.md §2.5`), before any `forge_create_issue`.

## 6. Candidate → draft-RFC map

Draft RFCs authored this session (status DRAFT, §5 architect lenses are stubs for next-session hardening):

| Candidate | Draft RFC | Status |
|---|---|---|
| C1 impact / blast-radius | [`docs/RFC-047-impact-blast-radius.md`](../docs/RFC-047-impact-blast-radius.md) | DRAFT — top pick, no schema change |
| C2 incremental extraction | [`docs/RFC-048-incremental-extraction.md`](../docs/RFC-048-incremental-extraction.md) | DRAFT — **REFRAMED**, profile-gated (discovery shows parsing isn't the bottleneck) |
| C3 framework entry-points | [`docs/RFC-049-framework-entry-points.md`](../docs/RFC-049-framework-entry-points.md) | DRAFT — self-dogfoodable on `clap` |
| C4 layer overlay | [`docs/RFC-050-layer-overlay.md`](../docs/RFC-050-layer-overlay.md) | DRAFT — schema bump + lockstep |
| C5 non-code / IaC | [`docs/RFC-051-non-code-extraction.md`](../docs/RFC-051-non-code-extraction.md) | DRAFT — **PARKED** (no consumer + no ground-truth) |
| C6 LLM enrichment | [`docs/RFC-052-llm-enrichment.md`](../docs/RFC-052-llm-enrichment.md) | DRAFT — **PARKED** (charter shift, needs blessing) |
