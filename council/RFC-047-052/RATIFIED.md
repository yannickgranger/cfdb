# RFC-047..052 — RATIFIED (council outcome)

Batch: the six Understand-Anything borrow RFCs (`docs/RFC-047..052`). Base `origin/develop` @ `ed27cd0`. Mechanism: agent-team council — four lens teammates (clean-arch, ddd-specialist, solid-architect, rust-systems), mailbox + shared task list, all claims `file:line`-verified. Two rounds: R1 independent verdicts + Phase-B cross-challenge (Q1–Q5); R2 re-review of the lead-applied amendments.

## Council outcome

| RFC / slice | R1 | R2 | Disposition |
|---|---|---|---|
| **047** impact / blast-radius | REQUEST CHANGES (clean-arch) | **RATIFY ×4** | **RATIFIED** — file 47-0, 47-A, 47-B, (47-C) |
| **048-A** profile (corrected phases) | REQUEST CHANGES (rust-systems) | **RATIFY ×4** | **RATIFIED** — the only unconditional 048 slice |
| **048-B** incremental enrichment | REQUEST CHANGES ×3 / REJECT-as-written (rust-systems) | — | **DEFERRED** — re-derive per-pass *after* 048-A's data |
| **048-C** parse-skip | REQUEST CHANGES / DEFER | — | **DEFERRED** — contingent on corrected 048-A |
| **048-D** G1 equivalence gate | RATIFY / conditional | conditional | **RATIFIED-conditional** — gates whichever of 48-B/C ever ships |
| **049** framework entry-points | REQUEST CHANGES ×3 | **RATIFY ×4** | **RATIFIED** — file 49-0, 49-A, 49-B, 49-C, 49-D |
| **050** layer (tier) overlay | REQUEST CHANGES ×4 | **RATIFY ×4** | **RATIFIED** — file 50-A, 50-C (50-B killed) |
| **051** non-code / IaC | KEEP-PARKED ×4 | — | **KEEP-PARKED** — no consumer + no ground-truth |
| **052** LLM enrichment | KEEP-PARKED ×4 (never RATIFY) | — | **KEEP-PARKED** — maintainer charter decision required |

R1 detail: `SYNTHESIS-R1.md`. Per-lens verdicts: `clean-arch.md`, `ddd-specialist.md`, `solid-architect.md`, `rust-systems.md` (each carries the R2 confirmation).

## Redesign delta R1 → R2 (what the council changed)

1. **RFC-047:** added prerequisite slice **47-0** — list-valued seed binding does not exist today (`query.rs:39` `--input` stub, `query.rs:104` rejects arrays); `impact` binds the seed list in-process via `parsed.params.insert` (mirroring `list_callers`), confined to `cfdb-cli`. `--max-depth` documented unbounded-by-default.
2. **RFC-048:** the profiled phase list was factually wrong — `cfdb extract` runs **none** of reachability/dup/git/recall (those are separate verbs + the `cfdb-recall` crate). Corrected to `{cargo-metadata, syn-walk, deferred-resolve, ingest, hir-load (if --hir), save}`; enrich/recall split to a separate profile. 48-B reframed to **per-existing-pass** incrementality (not a cross-cutting engine) and **deferred**; §4 vocab-fence line (fingerprint/change-class is build-cache state, never a `Label`/`EdgeLabel`/`:Item` attr/`SchemaVersion`).
3. **RFC-049:** clap + axum/actix `:EntryPoint` detection **already ships** in `cfdb-hir-extractor` (HIR-side, needs `Semantics` resolution — not `syn` as §3.2 claimed). Added slice **49-0** (refactor existing detectors into the `FrameworkDetector` seam, recall-neutral byte-identical); 49-A/B re-scoped to "register existing"; 49-C/D (PHP/TS) are the only green-field. Registry contract in `cfdb-lang`, impl per language-extractor crate, per-language `detect()` (ISP), no cross-boundary reach.
4. **RFC-050:** the A-vs-B dichotomy dissolved → **extract-time `:Crate.crate_tier`** (`Provenance::Extractor`), no edge, **no enrich verb and no verb extension** (rejecting the `enrich_bounded_context` god-pass and an 8th verb). Renamed `crate_tier` (off the live "Layer 1/Layer 2" homonym); **50-B (`:Item.layer`) killed** (items join via `IN_CRATE`). DAG sourced from each `package.dependencies` (`kind==Normal`, workspace-filtered) under the retained `.no_deps()` — the resolved DAG is **not** in-process (`lib.rs:158`); dev/build deps excluded (verified necessary: `cfdb-hir-extractor` dev-deps `cfdb-cli`). Added a `cfdb-recall` row (`CLAUDE.md §5`). Non-goal: `crate_tier` is depth-only, not instability/`D`.
5. **RFC-052:** the G1 fence it relied on **does not exist as code** (3-of-3 verified — solid retracted its Phase-A "fence is real" claim after reading `canonical_dump.rs`). G6 holds today only because `test_coverage` is unpopulated by default; a populated LLM `summary` would enter the dump and break G1. First slice (if ever blessed) must *build* a `const G1_EXCLUDED_ATTRS` filter; ceiling raised honestly, not folded into `enrich_metrics`.

## Ratification conditions carried into implementation

- **047:** 47-0 lands first (verify/add a `cfdb_core::Param` list variant; binding confined to `cfdb-cli`); 47-A/B compose `query` only, no `cfdb-core`/port change.
- **048-A:** profile the corrected phases only; the enrich/recall profile is a separate slice. 48-B/C are **not filed** until 048-A's numbers justify them.
- **049:** 49-0 must prove cfdb's own `:EntryPoint` set is **byte-identical** before/after the registry refactor (recall-neutral); 49-C/D add the first PHP/TS `:EntryPoint` emission + positive recall fixtures.
- **050:** one additive `:Crate.crate_tier` attribute → **minor `SchemaVersion` bump + lockstep `graph-specs-rust` `.cfdb/cross-fixture.toml` PR** (`CLAUDE.md §3`); 50-A carries the dev/build-dep-exclusion unit row + the recall row; 50-C states the `--features hir` requirement (cross-crate `CALLS` are HIR-resolved).
- **052:** **architects cannot ratify — maintainer charter decision required** before any slice. If blessed, the `G1_EXCLUDED_ATTRS` fence is built and tested *first*.

## Spun-off finding (independent of any RFC — recommend a tracking issue)

The council found a **latent cfdb determinism bug**: `specs/concepts/cfdb-core.md:209` (G6) claims `:Item.test_coverage` is excluded from the `G1` canonical-dump sha256, but `canonical_dump.rs` has **no enforcing filter** — it is byte-stable only because `test_coverage` is never populated by default. Running `enrich_metrics --features llvm-cov` before a determinism check would break `G1`. Fix = build the `G1_EXCLUDED_ATTRS` filter + back-fill the `test_coverage` claim (the same code RFC-052 would need). Worth a `fix:` tracking issue regardless of whether 052 ever proceeds.

## Next step

Ratified RFCs → file the §7 decompositions of **047, 048-A, 049, 050** as issues linked `Refs: docs/RFC-0NN-*.md`, carrying the prescribed `Tests:` blocks verbatim (`CLAUDE.md §2.4`). 048-B/C deferred (file only after 048-A). 051/052 not filed. **Not yet filed** — awaiting maintainer go (and the 052 charter decision).
