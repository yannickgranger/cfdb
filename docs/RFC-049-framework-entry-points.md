# RFC-049 — Framework-aware entry-point detectors

- **Status:** **RATIFIED.** Decomposition (49-0, 49-A, 49-B, 49-C, 49-D) ready to file; not yet filed.
- **Issue:** none yet (filed only after ratification).
- **Schema impact:** **none in v1.** Reuses the existing `:EntryPoint` label and its `kind`/`handler_qname`/`name`/`params` attributes; improves *recall*, adds no fact type. An optional `:EntryPoint.framework` provenance attribute is considered and **deferred** (§6).
- **Companion:** none in v1 (no schema surface). If `framework` attribute is later added → minor `SchemaVersion` bump + `graph-specs-rust` lockstep.
- **Origin:** `Understand-Anything` `framework-registry.ts` (per-framework `entryPoints` + `layerHints`).

---

## 1. Problem

`:EntryPoint` recall depends entirely on cfdb recognising the *idiom* by which a framework declares an entry. cfdb currently models the kinds it hand-detects (MCP tool, CLI command, HTTP route, cron). The discovery in [`studies/003`](../studies/003-cfdb-understand-discovery.md) showed cfdb's own entry surface is `clap` (in `cfdb-cli`) — and downstream targets (qbot-core, agentry) declare entries via `clap`-derive, Axum/Actix routing (Rust), Symfony/Laravel attributes (PHP), and Nest/Express decorators (TS). Where cfdb doesn't recognise the idiom, the entry point is invisible and every reachability/blast-radius answer rooted at it is incomplete.

`Understand-Anything` solves the analogous problem with a **framework registry**: detect the framework from the manifest, then apply framework-specific entry patterns. The borrow is that registry shape, made deterministic and recall-gated.

**Council correction (this changes the slice shape, not the goal).** cfdb already detects more Rust framework idioms than the draft assumed. `crates/cfdb-hir-extractor/src/entry_point_emitter.rs` already emits `:EntryPoint` for **clap** (`cli_command`, `#[derive(Parser/Subcommand)]`), **MCP** (`mcp_tool`), **axum + actix** (`http_route`, via `HTTP_ROUTE_METHOD_NAMES`, issues #124/#125), **cron** (`cron_job`), and **websocket**. So the Rust slices the draft framed as green-field (49-A clap, 49-B axum/actix) would **re-implement shipped code** — a DRY/recreation violation. Two consequences: (1) the detection is **HIR-side** (it needs `Semantics`/`sema.to_def` handler-qname resolution — `entry_point_emitter.rs:291-299`), not `syn`-side as §3.2 claimed; (2) the genuinely-new work is the **`FrameworkDetector` registry seam itself** (so adding a framework is a registration, not a match-arm edit) plus the **PHP/TS detectors** (49-C/D), which need new `:EntryPoint` emission in crates that emit none today. This RFC is therefore re-scoped: **49-0 builds the seam** by refactoring the existing detectors into it (recall-neutral, byte-identical emissions); **49-A/B register the existing detectors** into the seam; **49-C/D are the only new detectors.**

## 2. Scope

**Ships:** a registry of **per-framework, deterministic `:EntryPoint` detectors**, each gated on the framework being present (manifest dependency check) to avoid false positives. v1 detectors, ordered by self-dogfood value:
1. **`clap`-derive (Rust)** — cfdb's *own* CLI; immediately self-dogfoodable.
2. **Axum / Actix (Rust)** — route attribute/builder → HTTP `:EntryPoint`.
3. **Symfony / Laravel (PHP)** — route attributes/annotations → HTTP `:EntryPoint`.
4. **Nest / Express (TS)** — `@Controller`/`@Get` decorators, `app.get(...)` → HTTP `:EntryPoint`.

Each detector reuses the existing `:EntryPoint` shape and emits `EXPOSES` (→ handler fn) + `REGISTERS_PARAM` (→ exposed inputs) exactly as the current MCP/CLI detectors do.

**Does not ship:** any new node label or (in v1) attribute; runtime/dynamic route discovery; config-file-driven routes (that overlaps cfdb-051-non-code-extraction).

## 3. Design

### 3.1 Registry shape (borrowed, made deterministic, language-scoped)
A `FrameworkDetector` is `{ id, languages, present(manifest) -> bool, detect(ast, file) -> Vec<EntryPoint> }`. `present` checks the manifest (`Cargo.toml [dependencies]`, `composer.json`, `package.json`) for the framework's crate/package — the same keyword-presence gate `Understand-Anything` uses, but as a hard precondition: **a detector never runs on a workspace that doesn't depend on its framework**, eliminating cross-framework false positives.

**Registry placement.** The `FrameworkDetector` **contract** (the trait + the `:EntryPoint` projection) may live in the shared `cfdb-lang` seam, but each **impl lives in its own language-extractor crate** — the Rust detectors in `cfdb-hir-extractor`, PHP in `cfdb-extractor-php`, TS in `cfdb-extractor-ts`. `detect()` is parameterised by the **language-specific AST** the detector's `languages` declares (ISP — a Rust detector never sees a PHP AST; a single unioned super-AST would force cross-extractor coupling). The orphan rule enforces this placement anyway. **No detector reaches across language-extractor boundaries.** Registering a detector is the OCP extension point — adding a framework is a registration, not a `match framework { ... }` arm edit.

### 3.2 Detector = pure recogniser over existing AST (Rust = HIR-side)
Detectors consume the AST cfdb already parses (rust-analyzer HIR for Rust, tree-sitter for PHP/TS). They add no new parse pass — they recognise framework idioms in the existing tree and project them onto `:EntryPoint`. **Correction: Rust framework detection is HIR-side, not `syn`-side** — a route/command handler must be resolved to a real `:Item` qname via `Semantics`/`sema.to_def` (`entry_point_emitter.rs:291-299`); a `syn`-only detector would emit dangling handler `src` that `cfdb-petgraph`'s ingest drops (`entry_point_emitter.rs:230-232`). Only a *purely-textual* recogniser could be `syn`-side. clap-derive recognition reads the **`#[derive(...)]` attribute token text** on the AST node (`registers_param.rs:18-20`), never macro-expanded output (cfdb has no expanded `clap::Parser` impl to read) — the only tractable approach, and what the shipped detector already does.

### 3.3 "Stubs are not arrows" discipline
A detector emits an `:EntryPoint` **only when its handler fn resolves to a real `:Item`**. An unresolved route (handler in an unparsed/dynamic location) is **not** emitted as a synthesized-flag stub — it is dropped with a `Warning`, honouring the repo rule that a `_synthesized`/`_stub` endpoint discriminator means two concepts were conflated. Recall is measured against the resolvable set.

### 3.4 No schema change in v1
All emitted facts use existing `:EntryPoint` attributes. The framework identity is *not* recorded on the node in v1 (see §6 deferral) — the detector that produced it is an extraction-time concern, not a queryable fact, until a consumer needs to filter "entry points of framework X."

## 4. Invariants

- **Recall-gated.** Each detector ships with a hand-curated fixture of that framework's canonical entry declarations; the recall test asserts `extractor ≡ fixture ground-truth` (no missing entries, no spurious ones). This is the framework analog of the rustdoc recall gate — `cfdb-recall` is extended per detector.
- **Determinism (`G1`).** Detectors are pure functions of the AST + manifest; byte-stable re-extract preserved.
- **No false positives off-framework.** The `present(manifest)` precondition (§3.1) guarantees a detector is inert on a workspace not using its framework — asserted by a negative fixture.
- **No schema surface (v1).** No `SchemaVersion` bump, no `graph-specs-rust` lockstep.

## 5. Architect lenses

- **clean-arch.** Entry-point emission lives entirely in `cfdb-hir-extractor`; `cfdb-extractor` (syn) and the PHP/TS crates emit zero `:EntryPoint`. clap (49-A) + axum/actix (49-B) already ship there, so the §5 "where does the registry live" question was built on a stale premise. Flip condition: acknowledge the existing detectors; reframe 49-A/B as registry-extraction (not new detectors); resolve registry placement as language-scoped (contract in `cfdb-lang`, impl per extractor crate, no cross-boundary reach); scope 49-C/D as the real new emission capability in the PHP/TS crates.
- **ddd.** "framework" is extraction-time provenance, not a concept cfdb owns — the deferred `:EntryPoint.framework` attribute (§6) is the right call. No homonyms: a framework "route" folds onto the existing `:EntryPoint.kind="http_route"` enum value (`call_graph.rs:34`), and clap onto `cli_command` — reuse, not new vocabulary. "Stubs are not arrows" (§3.3) matches house style (`:Literal` drops `kind` to avoid a homonym; the `:CallSite` discriminator contract). One positive-recall test row added to 49-C (below).
- **solid.** The `FrameworkDetector` registry IS the OCP seam (registration, not match-arm edit) and `present(manifest)` is the right ISP-flavoured inert-off-framework guard. But since the Rust detectors already exist, the OCP claim describes a *target* state — 49-A/B as written would re-implement shipped code. Flip condition: add **49-0** (refactor existing const-scan + clap/tool predicates into the seam, recall-neutral / byte-identical), then 49-A/B *register* the existing detector (recall fixtures become the registration regression proof). ISP: per-language `detect()`.
- **rust-systems.** Confirmed clap-derive reads the derive *input* token text (no expansion to read) — the §5 macro-expansion worry is moot, and it is the only tractable approach. Confirmed both 49-A (clap) and 49-B (axum/actix) ship in `cfdb-hir-extractor` (HIR-side, needs handler-qname resolution). Flip condition: reframe both as "generalize the existing detector into the registry, byte-identical emissions"; correct §3.2 ("syn for Rust" → HIR for resolution-needing detectors). 49-C/D (PHP/TS) genuinely new — keep.

## 6. Non-goals

- `:EntryPoint.framework` provenance attribute — **deferred** until a consumer needs to filter by framework (avoids a schema bump with no puller, per the "tool backlog ≠ client chores" discipline).
- Runtime/dynamic route registration (belongs to cfdb-046-runtime-trace-ingest trace territory, not static extraction).
- Config-driven routes (YAML/annotations in non-code files) — overlaps cfdb-051-non-code-extraction; out of scope here.
- Framework *layer hints* (UA's `layerHints`) — that feeds cfdb-050-layer-overlay, not this RFC.

## 7. Issue decomposition

### 49-0 — Build the `FrameworkDetector` seam (council-added) — DO FIRST
Refactor the existing HIR entry-point detection (`entry_point_emitter.rs` const-scan + `has_clap_derive`/`has_tool_attr`/`HTTP_ROUTE_METHOD_NAMES`) behind the `FrameworkDetector` registry trait. **Recall-neutral**: cfdb's own `:EntryPoint` set must be byte-identical before/after. The OCP extension point is only real after this slice. Mechanically a refactor (per `CLAUDE.md §1` this is the registry-creation that the new PHP/TS slices then extend).
```
Tests:
  - Unit: the registry dispatches an AST to exactly the detectors whose present(manifest) is true; inert detectors are not invoked.
  - Self dogfood (cfdb on cfdb): the registry-routed path emits the byte-identical :EntryPoint + EXPOSES + REGISTERS_PARAM set the pre-registry HIR path emitted — no recall regression, no new/dropped entry.
  - Cross dogfood (graph-specs-rust at pinned SHA): :EntryPoint set unchanged → exit 0.
  - Target dogfood (qbot-core): none — rationale: behaviour-preserving refactor; the end-to-end signal is the byte-identical self-dogfood set.
```

### 49-A — Register the existing clap-derive detector into the seam
The clap (`cli_command`) detector already ships (`entry_point_emitter.rs:137-170`); this slice *registers* it, it does not re-implement it.
```
Tests:
  - Unit: a #[derive(Subcommand)] enum fixture yields one :EntryPoint per variant + REGISTERS_PARAM per field (existing behaviour, now via the registry).
  - Self dogfood (cfdb on cfdb): cfdb-cli's real subcommands still appear as cli_command :EntryPoints with resolved handler_qname — byte-identical to pre-49-0 (regression proof, not new capability).
  - Cross dogfood (graph-specs-rust at pinned SHA): inert if it doesn't use clap (negative proof) → exit 0.
  - Target dogfood (qbot-core): report count of clap entry points discovered in PR body.
```

### 49-B — Register the existing Axum/Actix detector into the seam
Also already shipped (`entry_point_emitter.rs:8-15` + `HTTP_ROUTE_METHOD_NAMES`); register, don't re-implement.
```
Tests:
  - Unit: a route fixture (Router::new().route(...) / actix .route(...)) yields http_route :EntryPoints (existing behaviour, via the registry).
  - Self dogfood: inert on cfdb (no Axum/Actix dep) — assert no HTTP entries added; byte-identical to pre-49-0.
  - Cross dogfood (graph-specs-rust): inert unless it depends on the framework → exit 0.
  - Target dogfood (qbot-core): recall against a hand-listed route fixture from the target.
```

### 49-C — Symfony/Laravel detector (PHP) — GENUINELY NEW (PHP emits zero `:EntryPoint` today)
This is real new capability: `cfdb-extractor-php` has no entry-point emission today, so this slice adds the first PHP `:EntryPoint` path (a larger lift than registering an existing detector).
```
Tests:
  - Unit: a #[Route('/x')] attribute / Route::get fixture yields http_route :EntryPoints.
  - Self dogfood (cfdb on cfdb): inert (cfdb has no Symfony) — negative manifest-gate proof.
  - Recall corpus (positive ground-truth): a hand-curated Symfony/Laravel route fixture in cfdb-recall — extractor ≡ fixture (no missing/spurious entries). This is the actual deliverable signal, not the inert self-dogfood.
  - Target dogfood: recall against a real PHP framework fixture; report count in PR body.
```

### 49-D — Nest/Express detector (TS) — GENUINELY NEW (TS emits zero `:EntryPoint` today)
Same as 49-C: `cfdb-extractor-ts` emits no `:EntryPoint` today; this adds the first TS path.
```
Tests:
  - Unit: @Controller/@Get and app.get(...) fixtures yield http_route :EntryPoints.
  - Self dogfood: inert on cfdb's TS fixtures (no Nest/Express) — negative manifest-gate proof.
  - Recall corpus (positive ground-truth): a hand-curated Nest/Express fixture in cfdb-recall — extractor ≡ fixture.
  - Target dogfood: recall against a real TS framework fixture; report count in PR body.
```
