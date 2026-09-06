# RFC-052 — Opt-in LLM enrichment (`:Item.summary` / `:Item.tags`)

- **Status:** **COUNCIL-TRIAGED → KEEP-PARKED (never RATIFY by architects)** pending an explicit maintainer decision. This is the one borrow that changes cfdb's *character* (deterministic tool → tool with a non-deterministic layer) and **must not proceed without explicit blessing + a full council pass**. **The G1 fence this RFC relies on does not exist as code** (see §3.1) — the first slice would have to *build* it, making 052 harder than drafted. Verdicts: [`council/cfdb-047-impact-blast-radius-052/`](../council/cfdb-047-impact-blast-radius-052/). (Borrowed candidate **C6** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
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

### 3.1 The fence — must be BUILT, not reused (council correction, 3-of-3 verified)
The draft claimed `summary`/`tags` could be "excluded from the canonical-dump sha256, exactly as `:Item.test_coverage` already is." **This is false at the implementation level.** `crates/cfdb-petgraph/src/canonical_dump.rs:45-139` has **no exclusion list** — `props_to_json` (`:133-139`) serializes *every* prop, and `node_envelope_json` inserts the whole map. `test_coverage` is byte-stable today **only because it is never populated by default** (`enrich/metrics/mod.rs` default `coverage_json: None`; the self-dogfood test states the exclusion is "observed trivially"), and bare `cfdb extract` never runs `enrich_metrics`. The G6 clause in `specs/concepts/cfdb-core.md:209` is a **documented contract with no enforcing code** — it works by *non-population*, not by a dump-time filter.

An LLM `summary` is, by definition, **populated** on the items it summarizes — so it has no "happens to be absent" free pass. The moment it is written to a node's `Props`, `canonical_dump` serializes it and two runs with a non-deterministic model produce different dumps → `G1` violated. **Therefore the first slice (if ever blessed) must *build* a real dump-time exclusion mechanism** — a `const G1_EXCLUDED_ATTRS` set consulted by `props_to_json`/`node_envelope_json`, keyed off `EnrichLlm` provenance, with a test proving such an attribute cannot reach the sha256 — landing **before** any summary is generated. Per `CLAUDE.md §6 rule 8` that set is a `const` in source, never an allowlist file. (This same gap is a latent cfdb determinism bug independent of 052 — see §6.)

### 3.2 The verb-ceiling problem — honest raise, NOT a fold (solid correction)
`EnrichBackend` is **closed at 7 verbs** (`cfdb-031-audit-cleanup#2`; verified `crates/cfdb-core/src/enrich.rs:91`). The draft preferred *folding* `summary`/`tags` into an existing verb's internal decomposition (citing the `enrich_metrics` 3-module split) to avoid expanding the closed surface. **The council rejects the fold:** the only plausible host is `enrich_metrics`, which is *deterministic, recall-adjacent quality signals* (`enrich.rs:181-191`) — folding a non-deterministic LLM concern into it gives that verb a second reason-to-change, i.e. the exact god-pass SRP violation flagged on cfdb-050-layer-overlay#3.3. The honest move, **if 052 is ever blessed**, is to argue the closed-at-7 `const` up to 8 in a reviewed council PR (the ceiling is methodology, raisable by argument per `cfdb-core.md:215`). A fold that violates SRP is a worse outcome than an honest, council-blessed ceiling raise. Either way this is conditional on the maintainer's charter decision (§1).

### 3.3 Provenance + honesty
A new `Provenance::EnrichLlm` variant marks every LLM-written attribute. `SchemaDescribe` declares these attributes non-deterministic and model-scoped, so no consumer mistakes them for ground-truth facts. The model id used is recorded alongside the keyspace by the *caller* (cfdb does not bless a specific model).

## 4. Invariants

- **`G1` preserved by exclusion.** The non-deterministic attributes never enter the canonical dump (§3.1). If they cannot be cleanly excluded, the RFC is dead.
- **Opt-in.** Default path unchanged; deterministic gates keep guarding it.
- **No recall claim.** LLM attributes are explicitly *not* recall-gated (there is no ground-truth for a summary) — they are declared best-effort in `SchemaDescribe`.
- **Verb ceiling honoured** (§3.2).

## 5. Architect lenses

- **clean-arch — KEEP-PARKED + fence misdescribed.** The port concern is satisfiable: the LLM call lives in a petgraph (or new) adapter pass like `git_history`/`metrics`, and `EnrichBackend`'s signature (`enrich.rs:91`) never sees an SDK type — no model type leaks into `cfdb-core`. But §3.1's "reuse the test_coverage fence" is false: `canonical_dump.rs:45-160` has no exclusion filter; the first slice must *build* one (§3.1 corrected).
- **ddd — KEEP-PARKED.** A `summary` is an *opinion*, not a *fact* — it cannot share the epistemic status of `doc_text` (the author's verbatim bytes). Admissible only behind `Provenance::EnrichLlm` (additive — `Provenance` is `#[non_exhaustive]`, `descriptors.rs:24`) and a G1 exclusion, on maintainer blessing. Caveat for any future review: `:Item.tags` must stay a distinct label/attribute from the deterministic `:Concept` overlay (`overlay.rs:8`, `LABELED_AS`) — conflating them is a split-brain on "semantic label."
- **solid — KEEP-PARKED + ceiling honesty.** If ever blessed, raise the closed-at-7 `const` honestly (§3.2) — do **not** fold a non-deterministic concern into `enrich_metrics` (a god-pass).
- **rust-systems — KEEP-PARKED, decisive finding.** The G6 precedent does not work as claimed: no dump-time filter exists; G6 holds only because `test_coverage` is unpopulated by default. A populated LLM `summary` would enter the dump and break G1. The first slice's red-first test is real (it fails today because the attr *would* appear). `Provenance::EnrichLlm` is the *input* to the exclusion filter, not the filter itself — the filter must be built.

## 6. Non-goals

- **(Spun-off finding, not part of 052)** The G6 spec-vs-code gap surfaced here — `specs/concepts/cfdb-core.md:209` claims `test_coverage` is excluded from the `G1` canonical-dump sha256, but no code enforces it (`canonical_dump.rs` has no filter); it is byte-stable only by non-population. If `enrich_metrics --features llvm-cov` is ever run before a determinism check, `G1` breaks. This is a **latent cfdb determinism bug independent of whether 052 ever proceeds** and warrants its own tracking issue (build the `G1_EXCLUDED_ATTRS` filter + back-fill the test_coverage claim).
- Making LLM enrichment default, or required by any gate.
- Domain→flow→step extraction (a different UA feature; cfdb's `:Context`/`:Concept` already covers most of it — `studies/002 §3 C6` note).
- Blessing a specific model or provider.
- Any change to the deterministic default extraction or its gates.

## 7. Issue decomposition

**Deliberately omitted while parked.** No issue is filed until the maintainer blesses the direction (§1). If blessed, the first slice is the `G1`-exclusion proof (a test that an `EnrichLlm` attribute cannot enter the canonical dump) — the fence must be proven *before* any summary is generated, with the standard `Tests:` block.
