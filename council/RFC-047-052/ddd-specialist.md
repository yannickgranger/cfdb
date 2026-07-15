# ddd-specialist verdict — RFC-047..052

**Lens:** Node/Edge vocabulary integrity · bounded-context ownership · homonym detection · aggregate boundaries · the split-brain test (two resolvers for one concept = one future bug).
**Base verified against:** `develop` @ `ed27cd0`, worktree `docs/harden-ua-rfcs`.
**Q5 (concept-ownership across 049+050): I lead.** Position recorded in §Contested below.

## Verdict table

| RFC | Verdict | One-line reason |
|---|---|---|
| 047 | **RATIFY** | "Impact/blast-radius" is a query *view*, not a concept — zero new vocabulary, no `:Item` attribute, no label. Confirmed against `labels.rs`. |
| 048-A (profile) | **RATIFY** | Profiling adds no schema; pure measurement. |
| 048-B (incremental enrich) | **REQUEST CHANGES** | Fingerprint/staleness is a *build mechanism*, not a schema concept — must be fenced out of the vocabulary explicitly (Q3). |
| 048-C (incremental extract) | **REQUEST CHANGES** | Same fence; plus the G1 byte-identity obligation is a build invariant, not a fact type. |
| 049 | **RATIFY** | "Framework" is extraction-time provenance, not a concept cfdb owns; framework "route" folds onto the *existing* `:EntryPoint.kind="http_route"` value — no homonym, no new label. `framework` attr correctly deferred. |
| 050 | **R1 REQUEST CHANGES → R2 RATIFY** | Split-brain passes (`crate_tier` ⟂ `:Context`); all three ddd amendments applied in the amended RFC AND the §2/§7 decomposition synced to the body (R2-verified `docs/RFC-050-layer-overlay.md:19,69-82`). **RATIFY.** |
| 051 | **KEEP-PARKED** | `:Service`/`:Resource`/`:Table` are a *consumer's* domain, not cfdb's bounded context. Correct to park on no-consumer (§1.1). |
| 052 | **KEEP-PARKED** (never RATIFY) | A `summary` is an *opinion*, not a *fact* — admissible only behind the `EnrichLlm` provenance fence, and only on maintainer blessing. Fence mechanism is sound IF blessed. |

---

## Per-RFC analysis (DDD lens)

### RFC-047 — impact / blast-radius → RATIFY

The draft's own §5 ddd position ("a *view*, not a concept") is correct and I verified the basis: `impact` introduces no `Label` constant and no `:Item` attribute (the full attribute list is `cfdb-core.md:101`; `EdgeLabel` set is `labels.rs:118-157`). It composes the existing `CALLS` edge (`labels.rs:135`) and the existing `reachable_from_production_entry` attribute (`structural.rs:162`). "Blast radius" is a *naming of a traversal*, not a noun in the domain model — exactly the right call. No bounded-context boundary is crossed; no aggregate is mutated (read-only, §4). **Nothing for my lens to gate.** RATIFY.

### RFC-048 — incremental extraction → RATIFY 048-A, REQUEST CHANGES 048-B/C

DDD scope here is narrow but load-bearing (I own the Q3 sub-point): **the fingerprint and the staleness/change-class are build mechanisms, not vocabulary.** They must never become a node label, edge label, or a `:Item` attribute. The precedent for "this is a build invariant, not a schema concept" is already in the spec: G6 / `test_coverage` is described as a *dump-stability* concern, not a fact (`specs/concepts/cfdb-core.md:209`), and G1 byte-identity is a property of the canonical dump, not a thing in the graph.

- 048-A (profile) is pure measurement — RATIFY.
- 048-B/C: **REQUEST CHANGES** with a concrete, checkable amendment: add an invariant line to RFC-048 §4 stating *"the structural fingerprint and the SKIP/PARTIAL/FULL change-class are persisted build-cache state under `.cfdb/`, NOT schema vocabulary: no `Label`, no `EdgeLabel`, no `:Item` attribute, no `SchemaVersion` bump. The incremental extract MUST produce a byte-identical canonical dump (G1) to a full re-extract of the same workspace SHA — the recall corpus asserts this."* With that line present, my lens flips to RATIFY (the engineering feasibility of 048-B is rust-systems' and clean-arch's call, not mine).

### RFC-049 — framework entry-points → RATIFY

Two homonym checks, both clear:

1. **Framework "route" vs. the existing HTTP `:EntryPoint` kind.** Verified `call_graph.rs:34`: `:EntryPoint.kind` is *already* an enum whose values include `http_route` (and `cli_command`, `mcp_tool`, `cron_job`, `websocket`, `test`, `bench`). An Axum/Symfony/Express route does **not** introduce a new concept — it is a *new recogniser for an existing kind value*. This is the right shape: RFC-049 reuses the label AND an existing enum value, so there is no homonym and no vocabulary growth. The clap-derive detector likewise emits `cli_command`, already a value.

2. **Is "framework" a concept cfdb owns, or provenance?** Provenance. There is no `framework` identifier anywhere in `crates/cfdb-core/src/` today (grep clean). The framework is *how the extractor recognised the entry*, not a queryable fact about the system's domain — so it belongs to the detector (extraction-time), not the node. The §6 deferral of the `:EntryPoint.framework` attribute is the correct DDD call: do not mint a vocabulary term for "which recogniser fired" until a consumer needs to filter on it ("tool backlog ≠ client chores"). If a puller later appears, `framework` would be an *additive provenance-style attribute* under `Provenance::Extractor`, a minor bump + graph-specs lockstep — fine, but not now.

3. **"Stubs are not arrows" (§3.3).** Verified the discipline is real house style: `:Literal` deliberately drops a `kind` attr to avoid a three-way homonym (`overlay.rs:125`), and the published `:CallSite` discriminator contract (`labels.rs:34-48`) exists precisely so two extractors don't conflate one label. RFC-049 §3.3's "emit `:EntryPoint` only when the handler resolves to a real `:Item`, else drop with a Warning — never a `_synthesized` stub" matches that style exactly. Good.

RATIFY. One test-surface correction in §Tests below (49-C self-dogfood row).

### RFC-050 — layer/tier overlay → REQUEST CHANGES (split-brain PASSES; the homonym is the problem)

**The split-brain test passes** — this is the heart of Q5, full reasoning in §Contested. `:Context` (ownership) and `tier` (DAG depth) are genuinely orthogonal resolvers; CREATE is justified. But two naming defects must be fixed before my lens ratifies:

**Amendment 1 (blocking) — name the attribute `crate_tier`, not `tier`/`layer`.** The word "layer" is already live in cfdb's prose with a *different* meaning: the schema is organised as "Layer 1 — structural extract" vs "Layer 2 — enrichment" (`descriptors.rs:11-16`, `overlay.rs:13`, repeated across specs). RFC-050 §3.4's own ddd note flags this. Minting a `:Crate.layer` or `:Item.layer` attribute would collide head-on with that established sense and is the textbook "two resolvers for one word" trap. The fix is mechanical: call the attribute **`crate_tier`** (an `:Crate` attribute) — unambiguous, no existing identifier collides (grep for `tier`/`framework` in the schema crate is empty). The 50-A title and §3 must use `crate_tier` throughout.

**Amendment 2 (blocking) — drop or rename 50-B (`:Item.layer`).** Inheriting the crate tier down onto every item as `:Item.layer` is (a) a homonym with the Layer-1/Layer-2 sense, and (b) redundant vocabulary — the item already reaches its crate via `IN_CRATE` (`labels.rs:118`), so any query can join to `:Crate.crate_tier` without a denormalised per-item copy. Per DDD "one concept, one home": the tier is a property of the *crate aggregate*, not of each item. **Recommended: kill 50-B entirely** (the layering-violation query in 50-C joins through `IN_CRATE`, it does not need `:Item.layer`). If a measured query-ergonomics reason to denormalise emerges, name it `:Item.crate_tier` (same term, explicitly a copy), never `layer`.

With amendments 1+2 my lens flips to RATIFY. On the Q1 axis my DDD position is **strictly an argument against resolution (B)**, not a vote for A's *mechanism*: materialising a `crate -[DEPENDS_ON]-> crate` edge mints a new edge label that no current consumer queries — vocabulary growth ahead of pull, the anti-pattern §6 correctly defers. If the council picks B it should be its own RFC with its own consumer justification, not folded in here. **A and C are DDD-indistinguishable** — both yield exactly one `crate_tier` attribute and no edge (the footprint I want); they differ only in *where* the Cargo read happens (A = enrich-time inside `enrich_bounded_context`; C = extract-time). That placement is a SOLID/clean-arch call, not a DDD one — see Amendment 3, where I adopt **C** because routing the Cargo read through `enrich_bounded_context` is a god-pass (solid-architect's ruling, which I concur with on aggregate-cohesion grounds).

**Amendment 3 (adopted from rust-systems Phase-B; supersedes RFC-050 §3.3) — emit `:Crate.crate_tier` at EXTRACT time (`Provenance::Extractor`), NOT via `enrich_bounded_context`.** rust-systems challenged the draft's routing and is right on the concept: `crate_tier` is a deterministic pure function of the Cargo `[dependencies]` DAG — a *structural fact*, identical in kind to `:Crate` itself — so it belongs with the structural extract, not an enrichment pass. It is also the cohesive home: `:Crate`, `:Context`, and `BELONGS_TO` are all already born together in `emit_crate_and_walk_targets` (`crates/cfdb-extractor/src/lib.rs:267-289`), where `bounded_context` is computed inline (`lib.rs:276`); `crate_tier` joins them there. Routing through `enrich_bounded_context` (RFC-050 §3.3) is wrong twice over: that pass reads `.cfdb/concepts/*.toml`, never `Cargo.toml`, AND §3.3 was the *sole* origin of the Q2 verb-ceiling question. **Extract-time emission dissolves Q2 entirely — no 8th verb, no extension of an existing enrich verb, `Provenance::Extractor`.** I adopt this; it strengthens the "fact not opinion" basis of my RATIFY.

**Correction to rust-systems' supporting claim (verified `file:line`) — the resolved DAG is NOT in-process today.** rust-systems asserted the extractor "already has the resolved DAG in-process via `cargo_metadata::exec()`." It does not: the single `MetadataCommand` call (`lib.rs:156-160` — the only one in the crate) uses **`.no_deps()`** (`lib.rs:158`), which deliberately suppresses dependency-graph resolution — only workspace package manifests come back, not the resolved DAG `crate_tier` needs. This does not change the verdict (extract-time emission is still correct), but it adds a concrete must-flag implementation cost: RFC-050 §3 must specify how the DAG is obtained — either (a) read each workspace package's `[dependencies]` table from its on-disk `Cargo.toml`, keeping intra-workspace edges only (sufficient for tier, and preserves `.no_deps()`), or (b) drop `.no_deps()` / issue a second deps-resolving `exec()` (heavier; full transitive resolve). Option (a) is the minimal surface (intra-workspace edges are all `crate_tier` needs). The RFC must NOT cite "the DAG is already in-process" as justification — it is not. (Note: this is the DAG-*sourcing* mechanism under resolution C; orthogonal to the A-vs-C *placement* question above.)

### RFC-051 — non-code / IaC / DDL → KEEP-PARKED

The DDD framing of §1.1's "no consumer" blocker: **`:Service`, `:Resource`, `:Table`, `:Pipeline` describe a *deployment/infrastructure* bounded context, not cfdb's *code-graph* bounded context.** cfdb owns "the structural facts of a source tree, ground-truthed against rustdoc." A Terraform `:Resource` or a K8s `:Service` is a fact about a *system's runtime topology* — that is a consumer's domain (agentry's deploy concerns, per the standing boundary "cfdb should not know about its clients"). Admitting these labels would import another bounded context's ubiquitous language into cfdb's core vocabulary with no anti-corruption layer and no ground-truth gate (the §1.2 recall blocker is real: there is no rustdoc analog, and cfdb's correctness contract *is* the recall gate, `cfdb-core.md` schema discipline). The one on-charter sliver — modelling cfdb's *own* `.cfdb/queries/*.cypher` as nodes — is genuinely cfdb's domain (its dogfood ruleset), but it is a different, much smaller RFC and should be split out if pursued, not used to justify the 12-format infra surface. **KEEP-PARKED**; unpark only when a named consumer pulls a specific format AND a ground-truth is designed for it.

### RFC-052 — opt-in LLM enrichment → KEEP-PARKED (never RATIFY)

The DDD question the draft poses ("is a `summary` a fact about the code or an opinion about it?") has a clear answer: **an opinion.** cfdb's entire vocabulary is *facts* — every attribute traces to a deterministic source (`Provenance::Extractor` from the AST, or an `Enrich*` pass over deterministic inputs). A `summary`/`tags` pair is a model's *interpretation*, which is why it cannot share the epistemic status of `doc_text` (`doc_text` is the author's verbatim bytes — a fact about the source; a summary is a generated gloss).

The fence is **sound IF blessed**, and I verified the mechanism it rests on is real, not aspirational:
- `Provenance` is `#[non_exhaustive]` (`descriptors.rs:24`), so adding `Provenance::EnrichLlm` is additive-compatible by construction.
- The G6 exclusion is a documented, generalisable invariant: `specs/concepts/cfdb-core.md:209` excludes `test_coverage` from the G1 dump sha256 and states *"Any future attribute with similar [non-deterministic] provenance must be declared under G6 at introduction and excluded from G1."* A `summary`/`tags` under `EnrichLlm` excluded from G1 is exactly the pattern the spec already anticipates.

So *if* the maintainer blesses the direction, the vocabulary mechanism is consistent with the existing model: a new provenance variant + a G6-excluded attribute clearly marked non-deterministic in `SchemaDescribe`. But this is a charter decision, not an architect's — **KEEP-PARKED**, never RATIFY. One DDD caveat to record for the eventual review: `:Item.tags` must NOT reuse or alias the `:Concept` overlay (`overlay.rs:8`, `LABELED_AS`) — `:Concept` is a deterministic, rule-assigned canonical-name overlay, whereas LLM `tags` are non-deterministic free-text; conflating them would be a split-brain on "semantic label." Keep them distinct labels/attributes with distinct provenance.

---

## Contested-question positions

### Q5 (I LEAD) — concept-ownership across RFC-049 + RFC-050

**My split-brain verdict, sent to all three peers:**

> **`tier`/`crate_tier` PASSES the split-brain test — CREATE is justified — but must be renamed off "layer" to avoid the live Layer-1/Layer-2 homonym. "Framework" is provenance, not a concept cfdb owns — RFC-049's deferral is correct.**

**Evidence the two resolvers are genuinely orthogonal (not one implying the other):**

1. `:Context` is *ownership*, derived from **crate-name prefix stripping**, with zero knowledge of the dependency DAG. Verified: `context_node_descriptor` (`overlay.rs:32-49`) carries only `canonical_crate`, `name`, `owning_rfc`, `source` — **no depth/tier attribute** — and `:Context` is computed by `cfdb_concepts::compute_bounded_context` via "crate-prefix heuristic" (`cfdb-concepts.md:3`, `overlay.rs:44`). Nothing in the `:Context` resolver consults `Cargo.toml [dependencies]`.

2. `tier` is *architectural role in the dependency stack*, the topological longest-path depth of the Cargo DAG (`studies/003 §2`). Its source is the manifest dependency graph — a completely different input from the crate-name string that produces `:Context`.

3. They are **independently variable**: `studies/003 §2` shows `cfdb-core` (tier 0) and `cfdb-cli` (tier 3) are both in the *same* `cfdb` context — one context spans multiple tiers. Conversely one tier can hold crates from multiple contexts. There is no functional dependency in either direction. That is the definition of orthogonal concepts → two distinct resolvers are warranted, not a split-brain.

4. **No existing canonical resolver for "tier."** `enrich_bounded_context` resolves ownership only; grep confirms zero `tier`/`framework` identifiers in `crates/cfdb-core/src/schema/`. So CREATE does not duplicate an existing resolver — it fills a genuine gap.

**The catch (why 050 is REQUEST CHANGES, not RATIFY):** the *concept* is independent, but the *name* "layer" is taken. cfdb already uses "Layer 1 / Layer 2" for structural-vs-enrichment provenance (`descriptors.rs:11-16`). Minting `:Crate.layer`/`:Item.layer` re-uses one word for two concepts — the exact failure mode this lens exists to catch. Hence Amendment 1 (`crate_tier`) and Amendment 2 (kill `:Item.layer`).

**On "framework" (RFC-049):** not a concept cfdb owns. It is extraction-time provenance ("which recogniser fired"), correctly *not* recorded on the node in v1 (§3.4, §6). The framework "route" homonym dissolves because routes map to the pre-existing `:EntryPoint.kind="http_route"` value (`call_graph.rs:34`) — reuse, not collision.

**Peer engagement & outcome:** I issued the Q5 statement to `clean-arch`, `solid-architect`, `rust-systems` and invited challenge.

**rust-systems engaged (Phase B) → CONVERGED, with my verdict strengthened and one of rust-systems' supporting facts corrected.** rust-systems endorsed the orthogonality finding and added the sharper framing that `crate_tier` is a *structural extractor fact*, not an enrichment output — therefore emit it at extract time under `Provenance::Extractor`, not via `enrich_bounded_context`. **I adopt this (Amendment 3 above).** It is the more cohesive home (`:Crate`/`:Context`/`BELONGS_TO` are already co-emitted in `lib.rs:267-289`) and it *dissolves the Q2 verb-ceiling question*, which only existed because RFC-050 §3.3 routed tier through an enrich verb. I corrected one supporting claim: the resolved Cargo DAG is **not** in-process today — the lone `MetadataCommand` call uses `.no_deps()` (`lib.rs:158`), so the RFC must specify how the DAG is sourced (per-package `Cargo.toml` read preferred). The concept verdict is unchanged; the implementation note is now accurate.

**solid-architect engaged (Phase B) → CONVERGED, with one clarification I accept.** solid independently reached the kill-50-B verdict (their CRP/no-independent-signal route vs. my aggregate-boundary route — same conclusion, two lenses) and adopted the `crate_tier` naming. solid sharpened the Q1 axis: RFC-050's resolution **(A)** specifically means *enrich-time* Cargo read; my DDD "no edge, no vocab-ahead-of-pull" argument is really an argument against **(B)** and is **satisfied identically by solid's resolution (C)** (extract-time, also one attribute, no edge). On DDD grounds A and C are indistinguishable; the placement is a SOLID call. solid rules the enrich-time read (A) a **god-pass** (two input domains in one pass: `.cfdb/concepts/*.toml` + `Cargo.toml`) and routes to extract-time (C). This is the same destination as my Amendment 3 (which I'd framed via rust-systems). **I have no objection: the council records C-not-A as the resolution** — extract-time emission, `Provenance::Extractor`, one `crate_tier` attribute, no edge. I corrected my own file's earlier mislabeling of C as "resolution (A)".

**clean-arch engaged (Phase B) → CONVERGED — all four lenses now aligned.** clean-arch independently confirmed the `layer` homonym from the screaming-architecture remit (citing `descriptors.rs:12-13` "Layer 1, syn AST + cargo_metadata" vs "Layer 2 enrichment" and `overlay.rs:13`) and adopted `crate_tier`. clean-arch **revised** an earlier RATIFY of 50-B to a drop, accepting my aggregate-boundary argument (a per-item copy has no query that needs it; `IN_CRATE` `labels.rs:118` gives 50-C its one-hop join). On Q1/placement, clean-arch confirms resolution **C** from the composition-root angle: the Cargo read already lives in the `cfdb-extractor` adapter (`lib.rs:50,156,267-300`), so extract-time emission adds no new read and `cfdb-core` declares no DAG type — satisfying my no-vocab-ahead-of-pull concern AND the Dependency Rule simultaneously. The A-vs-C axis is enrich-vs-extract (orthogonal to my A-vs-B edge axis); C wins on both lenses.

**Status: Q5 CONVERGED across ALL FOUR lenses (ddd + rust-systems + solid + clean-arch).** Tier is orthogonal to `:Context`; CREATE-as-`:Crate.crate_tier` justified; emitted at **extract time** in `cfdb-extractor` under `Provenance::Extractor` (resolution **C**, not A); `cfdb-core` declares no DAG type and no `DEPENDS_ON` edge; 50-B (`:Item.layer`) killed (two independent justifications: ddd aggregate-boundary + solid CRP, + clean-arch concurrence); DAG sourced from per-package on-disk `Cargo.toml` `[dependencies]` (the resolved DAG is not in-process — `.no_deps()` at `lib.rs:158`). Q2 verb-ceiling is moot for RFC-050 under C. **Q5 fully resolved — no open items.**

### Q3 (I respond) — fingerprint is a build mechanism, NOT a schema concept

**Confirmed.** The fingerprint and the SKIP/PARTIAL/FULL change-class are persisted build-cache state, not vocabulary. The precedent that "a dump/stability concern is not a fact" is G6 (`cfdb-core.md:209`) and the G1 canonical-dump invariant itself — both are properties of the *dump process*, not nodes in the graph. My REQUEST CHANGES on 048-B/C is exactly the amendment that keeps this out of the schema (§RFC-048 above). I defer to **rust-systems (lead)** on whether 48-B is an achievable engineering target vs. "cache the rustdoc JSON" being the true RFC; my lens only insists the cache key never becomes vocabulary.

---

## Test-surface prescription notes

- **RFC-050 50-A**: the unit row must assert the attribute is named **`crate_tier`** (per Amendment 1) and that it lives on `:Crate`, not `:Item`. Self-dogfood row is correct (`cfdb-core.crate_tier == 0`, `cfdb-cli.crate_tier == max`). Add a row to the recall/ground-truth assertion: `computed crate_tier == topological depth of the live Cargo manifest DAG` (the RFC already calls this the recall substitute, §4 — make it an explicit `Tests:` line).
- **RFC-050 50-B**: if killed per Amendment 2, delete the slice. If retained as `:Item.crate_tier` (denormalised copy), the unit row "an item's layer equals its crate's tier" must be reworded to "an item's `crate_tier` equals its crate's `crate_tier`" and justify the denormalisation with a named query that needs it — otherwise `Tests: none` is not available because the slice itself should not exist.
- **RFC-049 49-C (Symfony/Laravel)**: the self-dogfood row says "runs over the PHP test fixtures; inert (no Symfony)." That is a *negative* proof and fine, but it under-tests the detector. Either (a) keep the inert self-dogfood AND add an integration row exercising a real `#[Route]` fixture in the recall corpus (the RFC's §4 "hand-curated fixture" — make it a named `Tests:` row, not just prose), or (b) note the recall fixture *is* the positive signal. As written the 4-row block has no positive recall assertion for the PHP detector — add one.
- **RFC-049 49-A/B/D**: blocks are sound; each correctly pairs a positive unit fixture with an inert-off-framework self/cross-dogfood proof, which is the right shape for the `present(manifest)` precondition (§3.1).
- **RFC-052**: if ever blessed, the first `Tests:` block must prove the *negative* fence (an `EnrichLlm` attribute cannot reach the G1 sha256 path) BEFORE any positive summary test — the draft §7 already says this; endorse it. Add a DDD-specific row: assert `:Item.tags` and `:Concept` remain distinct labels (no alias), to prevent the semantic-label split-brain flagged above.
