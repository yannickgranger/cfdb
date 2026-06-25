# RFC-052 — Opt-in LLM enrichment (`:Item.summary` / `:Item.tags`)

- **Status:** DRAFT — **PARKED** pending an explicit maintainer decision. This is the one borrow that changes cfdb's *character* (deterministic tool → tool with a non-deterministic layer) and **must not proceed without explicit blessing + a full council pass**. (Borrowed candidate **C6** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
- **Issue:** none (will not be filed without the §1 decision).
- **Schema impact:** new `:Item.summary` / `:Item.tags` attributes under a new `EnrichLlm` provenance, **excluded from the `G1` canonical-dump sha256** (the `test_coverage` / `G6` precedent). Minor `SchemaVersion` bump.
- **Companion:** required (schema surface) — lockstep `graph-specs-rust` fixture bump.
- **Origin:** `Understand-Anything`'s headline feature — the `file-analyzer` agent's plain-English summaries + semantic tags.

---

## 1. Problem & the decision required

`:Item.doc_text` is only as good as the author's doc comments. `Understand-Anything`'s most-valued output is the LLM **summary** ("what this is and *why it matters*") and **tags** (semantic clustering) — the layer that turns a structural graph into something a newcomer can read. A cfdb consumer building a `chat`/onboarding experience would want these.

**The decision required of the maintainer (not the architects):** cfdb is, by charter, deterministic, recall-gated, and byte-stable. An LLM enrichment is **fundamentally non-deterministic**. Admitting it — even fenced and opt-in — is a philosophical shift in what cfdb *is*. The architects can harden the *mechanism*; only the maintainer can bless the *direction*. **This RFC stays parked until that blessing is explicit.**

## 2. Scope (only if blessed)

An **opt-in, off-by-default** enrichment that writes `:Item.summary` (1–2 sentences) and `:Item.tags` (3–5), under `EnrichLlm` provenance, **excluded from `G1`**. Never runs unless explicitly invoked; the default extraction + every existing gate stays 100% deterministic.

## 3. Design (sketch — only if blessed)

### 3.1 The fence — `G6`-style `G1` exclusion
`summary`/`tags` are declared toolchain/model-scoped and **excluded from the canonical-dump sha256**, exactly as `:Item.test_coverage` already is (`specs/concepts/cfdb-core.md`, `G6`). The determinism check (`ci/determinism-check.sh`) and the recall gate are unaffected because they operate on the `G1`-included surface, which this never enters.

### 3.2 The verb-ceiling problem
`EnrichBackend` is **closed at 7 verbs** (`RFC-031 §2`). A new `enrich_summaries` verb breaches the ceiling and requires explicit council approval, OR it folds into an existing verb's internal decomposition (the pattern `cfdb-petgraph`'s `enrich_metrics` 3-module split set). The draft prefers folding over a new verb, to avoid expanding the closed surface for a non-deterministic feature.

### 3.3 Provenance + honesty
A new `Provenance::EnrichLlm` variant marks every LLM-written attribute. `SchemaDescribe` declares these attributes non-deterministic and model-scoped, so no consumer mistakes them for ground-truth facts. The model id used is recorded alongside the keyspace by the *caller* (cfdb does not bless a specific model).

## 4. Invariants

- **`G1` preserved by exclusion.** The non-deterministic attributes never enter the canonical dump (§3.1). If they cannot be cleanly excluded, the RFC is dead.
- **Opt-in.** Default path unchanged; deterministic gates keep guarding it.
- **No recall claim.** LLM attributes are explicitly *not* recall-gated (there is no ground-truth for a summary) — they are declared best-effort in `SchemaDescribe`.
- **Verb ceiling honoured** (§3.2).

## 5. Architect lenses

> **DRAFT — review only after maintainer blessing (§1).** Pre-seeded, conditional on a "go":
- **clean-arch:** the LLM call is the most infrastructure-heavy adapter cfdb would have — confirm it cannot leak a model/SDK type into `cfdb-core`; the `EnrichBackend` port stays pure.
- **ddd:** is a `summary` a fact about the code or an opinion about it? (It is an opinion — hence the provenance fence.)
- **solid:** fold-into-existing-verb vs. new verb (§3.2).
- **rust-systems:** non-determinism quarantine — proving via test that no `EnrichLlm` attribute can reach the `G1` sha256 path.

## 6. Non-goals

- Making LLM enrichment default, or required by any gate.
- Domain→flow→step extraction (a different UA feature; cfdb's `:Context`/`:Concept` already covers most of it — `studies/002 §3 C6` note).
- Blessing a specific model or provider.
- Any change to the deterministic default extraction or its gates.

## 7. Issue decomposition

**Deliberately omitted while parked.** No issue is filed until the maintainer blesses the direction (§1). If blessed, the first slice is the `G1`-exclusion proof (a test that an `EnrichLlm` attribute cannot enter the canonical dump) — the fence must be proven *before* any summary is generated, with the standard `Tests:` block.
