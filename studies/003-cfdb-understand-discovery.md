# Study 003 — Discovery of cfdb via `Understand-Anything`

**Status:** discovery artifact (dogfood of the tool under study in [Study 002](002-borrowed-from-understand-anything.md)).
**Method:** ran `Understand-Anything`'s built pipeline (`scan-project.mjs` → `extract-structure.mjs`, Tree-sitter floor) directly against the cfdb worktree at `develop` 6d2928a. The UA plugin is not wired into Claude Code as a slash command in this session, so its **deterministic engine was driven directly**; the LLM-ceiling agents (per-file summaries/tags) were **not** run across all 344 Rust files (disproportionate token cost) — the architecture/layer reading below is hand-authored in the role of UA's `architecture-analyzer` from the deterministic facts.

---

## 1. What UA's deterministic floor saw

**Inventory (`scan-project.mjs`):** 654 files, complexity classified **`very-large`**, 0 filtered by ignore.

| Dimension | Breakdown |
|---|---|
| **By category** | code 399 · config 99 · docs 136 · infra 2 · script 17 · data 1 |
| **By language** | rust 344 · markdown 113 · toml 72 · **cypher 45** · txt 23 · json 19 · shell 17 · yaml 9 · php 5 · typescript 3 · python 1 · makefile 1 · csv 1 |

Two cfdb-specific things UA's generic scanner surfaced that a Rust-only lens would miss:
- **45 `.cypher` files** — cfdb's query/ban-rule/enrich-rule corpus (the dogfood ruleset in `.cfdb/queries/` plus `examples/` and test fixtures). UA treats these as a first-class language; cfdb's own extractor does not model them as graph inputs.
- **5 PHP + 3 TS fixtures** — the polyglot extractor test corpus (RFC-045), correctly detected as non-Rust code.

**Structure (`extract-structure.mjs`, all 352 code files, <1 s):** **2197 functions, 236 structs/classes.**

| crate | files | fns | structs | role (from dep DAG) |
|---|---:|---:|---:|---|
| cfdb-petgraph | 60 | 578 | 34 | store backend (the behemoth) |
| cfdb-cli | 70 | 391 | 19 | composition root |
| cfdb-extractor | 43 | 315 | 16 | syn extractor (Rust) |
| cfdb-query | 27 | 210 | 20 | Cypher-subset engine |
| cfdb-hir-extractor | 34 | 171 | 18 | HIR-based extractor |
| cfdb-core | 28 | 134 | 41 | schema/vocabulary (Zone of Pain) |
| cfdb-extractor-php | 8(+5) | 96 | 11 | PHP tree-sitter extractor |
| cfdb-extractor-ts | 9(+3) | 90 | 3 | TS tree-sitter extractor |
| cfdb-recall | 8 | 40 | 5 | rustdoc ground-truth gate |
| cfdb-concepts | 3 | 15 | 9 | bounded-context derivation |
| (root: examples/tools/spikes/ci) | 47 | 136 | 57 | — |

## 2. The architecture UA's layer view would assign (cfdb's real crate DAG)

Derived from `[dependencies]` across all `crates/*/Cargo.toml`:

```
Tier 0  cfdb-core ◄──────────────── (everything)        cfdb-extractor-shared
        (foundation, D≈0.95)
Tier 1  cfdb-concepts   cfdb-lang   cfdb-query   cfdb-hir-extractor      (← core only)
Tier 2  cfdb-petgraph (core+concepts)   cfdb-extractor (core+concepts+shared+lang)
        cfdb-extractor-php/ts (core+lang)   cfdb-recall (core+extractor)
        cfdb-hir-petgraph-adapter (core+hir-extractor+petgraph)
Tier 3  cfdb-cli  ──────────────────► (nearly all of the above)   composition root
```

This is a clean, acyclic, hexagonal-leaning layering: `cfdb-core` is the maximally-stable dependency sink (matching its declared Zone-of-Pain status in `specs/concepts/cfdb-core.md`), and `cfdb-cli` is the only composition root. **No crate-level dependency cycles.**

## 3. What the discovery confirms for the RFC candidates

The point of running UA over cfdb was not novelty for its own sake — it was to pressure-test the [Study 002](002-borrowed-from-understand-anything.md) candidates against cfdb's *actual* shape:

- **RFC-050 (layer overlay) is grounded.** cfdb's architecture is genuinely tiered (§2), and "tier/role" is a real, deterministically-derivable concept *distinct from* `:Context` ownership — exactly the split-brain test the candidate must pass. The crate DAG is the natural seed for an intra-workspace `:Layer` overlay.
- **RFC-048 (incremental extraction) has a clear payoff target.** `cfdb-petgraph` (578 fns / 60 files) and `cfdb-cli` (391 / 70) dominate; a structural-fingerprint skip on the other ~11 crates is where re-extract time is saved on a typical single-crate change. The "very-large" classification is the regime where full re-extraction hurts.
- **RFC-049 (framework entry-points) is scoped by reality.** cfdb's entry surface is `clap` (cfdb-cli) + the MCP/CLI `:EntryPoint` kinds it already models — modest, Rust-only. The candidate should target `clap`-derive + the existing kinds first; broad web-framework detectors are speculative for *this* repo (relevant only to downstream targets like agentry).
- **RFC-051 (non-code extraction) finds its first concrete artifact set here, but still no consumer.** The 45 `.cypher` files + 9 yaml + 2 infra are exactly the "non-code facts" UA models. Notably cfdb could model its **own** `.cfdb/queries/*.cypher` ban-rules as graph nodes (self-referential dogfood). Interesting, but still capability-driven, not consumer-pulled — stays parked.
- **RFC-047 (impact/blast-radius) needs nothing new.** The 2197-function CALLS graph + reachability already present is sufficient; this discovery simply confirms the graph is dense enough for blast-radius to be meaningful (avg ~6.2 fns/file).

## 4. Honest limitations of this discovery

- **No LLM enrichment pass.** Per-file summaries/tags/complexity (UA's headline output) were not generated for 344 files. Structural coverage is complete; semantic coverage is not.
- **Import map empty.** UA's file-level `importMap` is populated by a separate agent step (`extract-import-map.mjs`) that was not run; §2 uses the authoritative crate-level Cargo DAG instead, which is the more meaningful architecture signal for a Rust workspace anyway.
- **UA vs cfdb on the same tree:** cfdb's own extractor produces a *finer and ground-truthed* graph (`:CallSite`, `:Argument`, visibility, reachability, recall-vs-rustdoc) than UA's. The value UA adds over cfdb on cfdb is **breadth** (the 45 `.cypher` + yaml + infra it ingested) and **ergonomic framing** (summaries/tours), not structural depth.
