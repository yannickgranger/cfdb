# RFC-049 — Framework-aware entry-point detectors

- **Status:** DRAFT — pending architect hardening + council. (Borrowed candidate **C3** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
- **Issue:** none yet (filed only after ratification).
- **Schema impact:** **none in v1.** Reuses the existing `:EntryPoint` label and its `kind`/`handler_qname`/`name`/`params` attributes; improves *recall*, adds no fact type. An optional `:EntryPoint.framework` provenance attribute is considered and **deferred** (§6).
- **Companion:** none in v1 (no schema surface). If `framework` attribute is later added → minor `SchemaVersion` bump + `graph-specs-rust` lockstep.
- **Origin:** `Understand-Anything` `framework-registry.ts` (per-framework `entryPoints` + `layerHints`).

---

## 1. Problem

`:EntryPoint` recall depends entirely on cfdb recognising the *idiom* by which a framework declares an entry. cfdb currently models the kinds it hand-detects (MCP tool, CLI command, HTTP route, cron). The discovery in [`studies/003`](../studies/003-cfdb-understand-discovery.md) showed cfdb's own entry surface is `clap` (in `cfdb-cli`) — and downstream targets (qbot-core, agentry) declare entries via `clap`-derive, Axum/Actix routing (Rust), Symfony/Laravel attributes (PHP), and Nest/Express decorators (TS). Where cfdb doesn't recognise the idiom, the entry point is invisible and every reachability/blast-radius answer rooted at it is incomplete.

`Understand-Anything` solves the analogous problem with a **framework registry**: detect the framework from the manifest, then apply framework-specific entry patterns. The borrow is that registry shape, made deterministic and recall-gated.

## 2. Scope

**Ships:** a registry of **per-framework, deterministic `:EntryPoint` detectors**, each gated on the framework being present (manifest dependency check) to avoid false positives. v1 detectors, ordered by self-dogfood value:
1. **`clap`-derive (Rust)** — cfdb's *own* CLI; immediately self-dogfoodable.
2. **Axum / Actix (Rust)** — route attribute/builder → HTTP `:EntryPoint`.
3. **Symfony / Laravel (PHP)** — route attributes/annotations → HTTP `:EntryPoint`.
4. **Nest / Express (TS)** — `@Controller`/`@Get` decorators, `app.get(...)` → HTTP `:EntryPoint`.

Each detector reuses the existing `:EntryPoint` shape and emits `EXPOSES` (→ handler fn) + `REGISTERS_PARAM` (→ exposed inputs) exactly as the current MCP/CLI detectors do.

**Does not ship:** any new node label or (in v1) attribute; runtime/dynamic route discovery; config-file-driven routes (that overlaps RFC-051).

## 3. Design

### 3.1 Registry shape (borrowed, made deterministic)
A `FrameworkDetector` is `{ id, languages, present(manifest) -> bool, detect(ast, file) -> Vec<EntryPoint> }`. `present` checks the manifest (`Cargo.toml [dependencies]`, `composer.json`, `package.json`) for the framework's crate/package — the same keyword-presence gate `Understand-Anything` uses, but as a hard precondition: **a detector never runs on a workspace that doesn't depend on its framework**, eliminating cross-framework false positives.

### 3.2 Detector = pure recogniser over existing AST
Detectors consume the AST cfdb already parses (`syn` for Rust, tree-sitter for PHP/TS). They add no new parse pass — they recognise framework idioms in the existing tree and project them onto `:EntryPoint`. Example (clap-derive): a `#[derive(Subcommand)]` enum's variants → CLI `:EntryPoint`s, each variant's fields → `REGISTERS_PARAM`.

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

> **DRAFT — to be filled by next-session architect hardening before council.** Pre-seeded focus:
- **clean-arch:** where does the registry live — `cfdb-extractor` (Rust), `cfdb-extractor-php`, `cfdb-extractor-ts`, or a shared `cfdb-extractor-shared` trait? Detectors must not reach across language-extractor boundaries.
- **ddd:** is "framework" a concept cfdb owns, or extraction-time provenance only? (Draft: provenance — §3.4, §6.) Homonym check: framework "route" vs. the existing HTTP `:EntryPoint` kind.
- **solid:** OCP — adding a framework is registering a `FrameworkDetector`, not modifying a match arm. Confirm the registry is the extension point.
- **rust-systems:** `clap`-derive recognition via `syn` — derive-macro attribute parsing is non-trivial; verify the detector reads the derive input, not macro-expanded output (which cfdb doesn't have).

## 6. Non-goals

- `:EntryPoint.framework` provenance attribute — **deferred** until a consumer needs to filter by framework (avoids a schema bump with no puller, per the "tool backlog ≠ client chores" discipline).
- Runtime/dynamic route registration (belongs to RFC-046 trace territory, not static extraction).
- Config-driven routes (YAML/annotations in non-code files) — overlaps RFC-051; out of scope here.
- Framework *layer hints* (UA's `layerHints`) — that feeds RFC-050, not this RFC.

## 7. Issue decomposition

### 49-A — `clap`-derive detector (Rust) + self-dogfood
The highest-value first slice: cfdb's own CLI becomes `:EntryPoint`s.
```
Tests:
  - Unit: a #[derive(Subcommand)] enum fixture yields one :EntryPoint per variant + REGISTERS_PARAM per field.
  - Self dogfood (cfdb on cfdb): assert cfdb-cli's real subcommands appear as CLI :EntryPoints with resolved handler_qname.
  - Cross dogfood (graph-specs-rust at pinned SHA): assert zero new entry points if it doesn't use clap (negative/inert proof).
  - Target dogfood (qbot-core): report count of clap entry points discovered in PR body.
```

### 49-B — Axum/Actix detector (Rust HTTP)
```
Tests:
  - Unit: a route fixture (#[get("/x")] / Router::new().route(...)) yields HTTP :EntryPoints.
  - Self dogfood: inert on cfdb (no Axum/Actix dep) — assert no HTTP entries added.
  - Cross dogfood (graph-specs-rust): inert unless it depends on the framework.
  - Target dogfood (qbot-core): recall against a hand-listed route fixture from the target.
```

### 49-C — Symfony/Laravel detector (PHP)
```
Tests:
  - Unit: a #[Route('/x')] attribute / Route::get fixture yields HTTP :EntryPoints.
  - Self dogfood (cfdb on cfdb): runs over the PHP test fixtures; inert (no Symfony) — negative proof.
  - Cross dogfood (graph-specs-rust): inert.
  - Target dogfood: recall against a PHP framework fixture; report in PR body.
```

### 49-D — Nest/Express detector (TS)
```
Tests:
  - Unit: @Controller/@Get and app.get(...) fixtures yield HTTP :EntryPoints.
  - Self dogfood: inert on cfdb's TS fixtures (no Nest/Express) — negative proof.
  - Cross dogfood (graph-specs-rust): inert.
  - Target dogfood: recall against a TS framework fixture; report in PR body.
```
