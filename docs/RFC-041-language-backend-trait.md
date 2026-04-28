# RFC-041 — Pluggable `LanguageProducer` trait

Status: **Ratified (R2, 2026-04-28) — 4/4 RATIFY: clean-arch, ddd-specialist, solid-architect, rust-systems.**
Parent META: #266 (cfdb multi-language roadmap).
Implementing issue: #263 (Phase 1 of META #266).

R1 council outcome: 3× REQUEST CHANGES (clean-arch / ddd-specialist / rust-systems) + 1× RATIFY-with-clarification (solid-architect). 11 distinct change requests + 2 rust-systems factual corrections; all applied in R2.

R2 council outcome: **4× RATIFY**. clean-arch + ddd-specialist + solid-architect + rust-systems verdicts captured inline in §5; the rust-systems R2 factual correction (resolver-v2 `dep:` prefix in `cfdb-cli/Cargo.toml`'s feature gate) folded into §3.4. The ddd-specialist + clean-arch R2 editorial nit (stale `RustBackend::extract` references in §4 "Determinism" / "Recall") corrected.

---

## Glossary

- **producer** — a language-specific walker that emits cfdb structural facts (`:Item`, `:CallSite`, `:Module`, edges). The R2 ubiquitous-language word for what R1 called "backend"; renamed per ddd-specialist R1 to escape the `StoreBackend` (consumer-side persistence) / `EnrichBackend` (consumer-side enrichment) homonym (`crates/cfdb-core/src/store.rs:62`, `crates/cfdb-core/src/enrich.rs:91`). Three roles share neither suffix nor verb in R2: `StoreBackend` stores, `EnrichBackend` enriches, `LanguageProducer` produces.
- **`LanguageProducer::produce`** — the trait method that walks a workspace and returns the fact tuple. R2 rename of R1's `extract()`, per ddd-specialist R1 — escapes the four-way homonym `cfdb extract` (CLI verb), `cfdb-extractor` (crate), `cfdb_extractor::extract_workspace` (legacy fn), and the trait method itself.
- **`Detection`** — the boolean answer "does this producer own this workspace?" Returned as bare `bool` in v0.1; ddd-specialist R1 ratified the YAGNI ruling against promoting it to a value object.

---

## 1. Problem

cfdb's `cfdb-extractor` crate is monolithically Rust-specific: `pub fn extract_workspace(workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), ExtractError>` (`crates/cfdb-extractor/src/lib.rs:91`) walks `Cargo.toml` via `cargo_metadata`, parses every `.rs` via `syn`, and emits the v0.1 fact set. There is no abstraction seam where a non-Rust language could plug in.

The cost surfaces in three places:

1. **agentry** (the orchestrator) wants project-agnostic anti-drift. Today the `cfdb extract → cfdb query → graph-specs check` pipeline is mechanical for Rust workspaces only; PHP RIS legacy + TypeScript SPAs get only semantic anti-drift via the reviewer-claude prompt — strictly weaker than mechanical (per META #266 §"Why this matters").

2. **qbot-core** rescue informed cfdb's vocabulary, but the vocabulary itself (`:Item`, `:CallSite`, `:Module`, `:Crate`, `IN_*` / `CALLS` / `IMPLEMENTS` / `RETURNS` edges) is not Rust-specific — these are general AST-level structural concepts every typed language exposes. The schema is reusable; the producer pipeline is not.

3. **The `cfdb v1.0` split (EPIC #279)** treats cfdb-extractor as one of several core crates. Without a plug-in seam at the language level, every additional language requires touching every consumer (`cfdb-cli`'s extract command, the `--workspace` flag's language detection logic, schema describer entries). With a seam, languages plug in compile-time-optionally without touching the core.

This RFC commissions Phase 1 of META #266: refactor the existing Rust extractor behind a `LanguageProducer` trait so PHP (#264) and TypeScript (#265) can plug in without re-architecting the core.

---

## 2. Scope

### Deliverables

1. **New crate `cfdb-lang`** — defines the `LanguageProducer` trait + the small types that flow through it (`LanguageError`). Bounded context: **language production** (per ddd-specialist R1, captured in `specs/concepts/cfdb-lang.md`). Sits next to `cfdb-core` in the inner ring; depends on `cfdb-core` for `Node` / `Edge` only.

2. **The `LanguageProducer` trait** — `name() / detect() / produce()` shape (§3.1). Object-safe under the actual safety conditions; supertrait bound is `Send` only (no `Sync` — see §5.4 rust-systems R1).

3. **Move existing `cfdb-extractor` behind the trait** — the existing `extract_workspace` becomes a backward-compat shim delegating to `RustProducer::produce`. The `cfdb-extractor` crate gains a `pub struct RustProducer` + `impl LanguageProducer for RustProducer { ... }` block, no behavioural change.

4. **`cfdb extract` CLI dispatches via the trait** — `cfdb-cli/src/lang.rs` (NEW composition-root module) builds a registry of producers compiled in (gated by Cargo features), runs each producer's `detect()` against `--workspace`, dispatches to the matching producer's `produce()`. Polyglot repo (multiple producers detect): v0.1 picks first-detected and warns; `--lang` override is deferred to a follow-up.

5. **Cargo feature flags + `cfdb-extractor` becomes optional in `cfdb-cli`'s manifest.** `cfdb-cli` gains `default = ["lang-rust"]` and `lang-rust = ["cfdb-extractor"]`; `cfdb-extractor` becomes `optional = true`. Future languages add `lang-php`, `lang-typescript` on the same pattern. The `lang-rust` default-on preserves backward-compat for every existing consumer; the slim build (`cargo check -p cfdb-cli --no-default-features`) compiles without the Rust producer's `syn` / `cargo_metadata` / `ra-ap-*` transitives — verified in CI per Slice 41-C.

6. **README repositioning** — the §"What cfdb extracts" paragraph gains a "Languages: Rust (v0.1 reference)" sub-row pointing at the trait + the multi-language follow-ups (#264 / #265).

### Non-deliverables (out of scope, not deferred — see §6 for deferred decisions)

- **No PHP or TypeScript producer.** Those are #264 / #265 — separate slices that DEPEND on this RFC's deliverables.
- **No `SchemaVersion` bump.** The trait surfaces existing `cfdb-core::Node` / `Edge` types unchanged. **`:Item.kind` is closed-set** (per §4 + ddd-specialist R1) — new languages introducing new `kind` values must ship a separate schema RFC, not extend the open-set ad-hoc.
- **No new node label, edge label, or attribute** — the trait is purely a producer-side seam; the schema vocabulary stays inner-ring.
- **No polyglot-repo merge semantics.** v0.1 = one producer per invocation.
- **No removal of the existing `cfdb_extractor::extract_workspace` public function.** Stays as a backward-compat shim per EPIC #279. (The shim's deprecation timeline is a deferred decision — see §6.)

---

## 3. Design

### 3.1 The `LanguageProducer` trait

```rust
// crates/cfdb-lang/src/lib.rs

use std::path::Path;
use cfdb_core::fact::{Node, Edge};

/// A language-specific producer of cfdb structural facts. Each
/// implementation walks a workspace whose root path matches its
/// detection criterion and emits the v0.1 `:Item` / `:CallSite` /
/// `:Module` / `:Crate` / `IN_*` / `INVOKES_AT` fact set defined in
/// `cfdb-core::schema`. The set of allowed `:Item.kind` values is
/// schema-governed (see §4 "Schema vocabulary"); a producer that
/// needs a new `kind` must ship a separate RFC + `cfdb-core::schema`
/// patch, not extend the open-set ad-hoc.
pub trait LanguageProducer: Send {
    /// Stable kebab-case identifier used in CLI flags + keyspace
    /// suffixes (`"rust"`, `"php"`, `"typescript"`). Must match the
    /// Cargo feature gate (`lang-<name>`).
    fn name(&self) -> &'static str;

    /// `true` when this producer is willing to walk
    /// `workspace_root`. Cheap — typically reads one or two marker
    /// files (`Cargo.toml` for Rust, `composer.json` for PHP,
    /// `package.json` + `tsconfig.json` for TS).
    ///
    /// MUST NOT walk the entire workspace — `detect()` runs once per
    /// producer per CLI invocation, gating the expensive `produce()`
    /// call.
    fn detect(&self, workspace_root: &Path) -> bool;

    /// Walk the workspace and emit the fact set. Pure: produces the
    /// node + edge vectors and returns; does not touch any store.
    /// Errors carry a `LanguageError` enumerating the failure modes
    /// the producer recognises.
    fn produce(
        &self,
        workspace_root: &Path,
    ) -> Result<(Vec<Node>, Vec<Edge>), LanguageError>;
}

/// Errors a producer may surface.
#[derive(Debug, thiserror::Error)]
pub enum LanguageError {
    #[error("workspace root not detected by producer `{producer}`: {reason}")]
    NotDetected { producer: &'static str, reason: String },

    #[error("workspace root I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("producer-specific parse failure in `{producer}`: {message}")]
    Parse { producer: &'static str, message: String },
}
```

The trait is **object-safe** under the actual object-safety conditions (per the rust-systems R1 factual correction): no generic methods, no `where Self: Sized` clauses, all method receivers are `&self`, no associated types. `Send` (alone) is the supertrait bound — `Sync` is not required because the v0.1 dispatcher (`cfdb-cli`'s `available_producers()`, §3.4) is single-threaded sequential. `Sync` is added in a follow-up RFC if a polyglot-parallel-dispatch design ever needs it. `Box<dyn LanguageProducer>` is the dispatch shape (§3.4).

**Why these three methods:**

- **`name()`** — needed by the CLI to disambiguate when multiple producers accept the same workspace root (e.g. a Rust crate with TS frontend) and by the keyspace-naming convention (`<workspace>-<lang>`).
- **`detect()`** — separating detection from production is the standard plug-in shape (cargo subcommands, language-server adapters, file-type detectors). Saves the producer cost on negative invocations.
- **`produce()`** — emits the existing fact-set tuple. The signature mirrors the current `cfdb_extractor::extract_workspace` so the migration is mechanical (§3.3). The method name was `extract()` in R1 — renamed in R2 per ddd-specialist to escape the four-way `extract` homonym (CLI verb, crate, legacy fn, trait method).

**Why not also `name() -> impl Iterator<Item = &str>` (alias support):** YAGNI for v0.1; if a producer needs to advertise multiple flag aliases (`"ts"` AND `"typescript"`) the CLI dispatcher wraps the canonical name in an alias map.

**Why not `produce(&self, ws) -> Box<dyn Iterator<Item = Fact>>`** (streaming): the existing `extract_workspace` returns the eager `(Vec<Node>, Vec<Edge>)` tuple; streaming would break object safety (associated `Item` type on the iterator can't be named through `dyn`). Idiomatic alternative when memory-bound becomes real: a callback `fn produce(&self, ws: &Path, sink: &mut dyn FnMut(Fact)) -> Result<(), LanguageError>`. Deferred to a follow-up RFC; cfdb-self at ~19k nodes / ~22k edges is well under any process memory cap (rust-systems R1 ratified the deferral).

### 3.2 The `cfdb-lang` crate

A new crate at `crates/cfdb-lang/`:

```
[package]
name = "cfdb-lang"

[dependencies]
cfdb-core = { path = "../cfdb-core" }
thiserror.workspace = true
```

Crate purpose: hold the `LanguageProducer` trait + `LanguageError` type. Bounded context: **language production** (per ddd-specialist R1), distinct from `cfdb-core`'s "schema vocabulary" context and `cfdb-extractor`'s "Rust producer" context. The bounded-context name is captured in `specs/concepts/cfdb-lang.md` (Slice 41-A) so the cfdb extractor's `BELONGS_TO -> :Context` resolves to meaningful vocabulary rather than the heuristic `"lang"`.

Phase 1 dependency-graph metrics (per solid-architect R1):

| Crate | Ca | Ce | I = Ce/(Ca+Ce) | A | D = \|A+I−1\| | Zone |
|---|---|---|---|---|---|---|
| `cfdb-lang` (proposed) | 2 (`cfdb-extractor` + `cfdb-cli`) | 1 (`cfdb-core`) | 0.33 | 0.50 | 0.17 | Usefulness |
| `cfdb-core` (alternative) | 8 | 2 | 0.20 | 0.022 | 0.78 | Pain |

`cfdb-lang` lands in the Zone of Usefulness; `cfdb-core` placement would push the schema-vocabulary ring into the Zone of Pain. As more producers are added (PHP #264, TS #265), `cfdb-lang`'s instability `I` decreases (Ca grows, Ce constant), strengthening its stable-abstraction claim. **The R1 wording "`Ca = 0` from concrete language backends" was wrong** — concrete backends DO depend on `cfdb-lang` (they `impl` the trait); the metric in the table is the corrected Phase-1 figure.

**Why a new crate, not a module in `cfdb-core`:** ADP (Acyclic Dependency Principle) + SAP (Stable Abstractions Principle). `cfdb-core` already has 92 public items with only 2 traits (`StoreBackend` at `crates/cfdb-core/src/store.rs:62`, `EnrichBackend` at `crates/cfdb-core/src/enrich.rs:91`); A ≈ 0.022. Adding `LanguageProducer` there would push `cfdb-core` further into the Zone of Pain. The clean-arch R1 lens reinforces this: `cfdb-core`'s two existing traits are *consumer-side ports* (storing, enriching). `LanguageProducer` is a *producer-side seam* with a different stability profile — it changes whenever a new language is added. Mixing the two responsibilities in one crate violates SRP at the crate level.

**Why not a module in `cfdb-extractor`:** `cfdb-extractor` becomes a *concrete* producer, depending on `cfdb-lang`'s trait. Defining the trait inside `cfdb-extractor` would couple every future producer's compile graph to the Rust pipeline's `syn` dep tree.

### 3.3 Migrating `cfdb-extractor` to the trait

The existing `cfdb_extractor::extract_workspace` becomes a thin wrapper over the new `RustProducer::produce`:

```rust
// crates/cfdb-extractor/src/lib.rs

use cfdb_lang::{LanguageProducer, LanguageError};

pub struct RustProducer;

impl LanguageProducer for RustProducer {
    fn name(&self) -> &'static str { "rust" }

    fn detect(&self, workspace_root: &Path) -> bool {
        workspace_root.join("Cargo.toml").is_file()
    }

    fn produce(
        &self,
        workspace_root: &Path,
    ) -> Result<(Vec<Node>, Vec<Edge>), LanguageError> {
        // Delegate to the existing private extract_inner that takes
        // workspace_root and returns the (Vec<Node>, Vec<Edge>).
        // Map ExtractError → LanguageError::Parse.
        extract_inner(workspace_root)
            .map_err(|e| LanguageError::Parse {
                producer: "rust",
                message: e.to_string(),
            })
    }
}

/// Backward-compat — qbot-core CI calls this function directly via
/// `cargo install --git`. Preserved as a delegating shim. The
/// deprecation timeline for this shim is intentionally deferred (see
/// §6 Non-goals: ExtractError deprecation timeline).
pub fn extract_workspace(
    workspace_root: &Path,
) -> Result<(Vec<Node>, Vec<Edge>), ExtractError> {
    extract_inner(workspace_root)
}
```

**`ExtractError` stays public** — it's the existing public type qbot-core's CI catches. The new `LanguageError` is a strictly-broader sum (carries the producer name + the original message); `ExtractError` is preserved for the legacy entry point. The dual public error type is a known asymmetry — see §6 Non-goals for why its deprecation timeline is deferred.

**No file moves.** The submodule layout (`attrs/`, `item_visitor/`, `call_visitor.rs`, etc.) stays. Only `lib.rs` gains the `pub struct RustProducer` + `impl LanguageProducer for RustProducer` block.

### 3.4 CLI dispatch

The composition root for the producer registry lives in a dedicated `crates/cfdb-cli/src/lang.rs` module (per clean-arch R1; mirrors the existing `compose::empty_store()` separation at `crates/cfdb-cli/src/commands/extract.rs:52`). The command handler in `crates/cfdb-cli/src/commands/extract.rs` evolves from:

```rust
// today (single call site, no producer abstraction)
let (nodes, edges) = cfdb_extractor::extract_workspace(workspace)?;
```

to:

```rust
// post-RFC, in commands/extract.rs
use crate::lang::{available_producers, NoProducerDetected};

let producers = available_producers();
let matched: Vec<&dyn LanguageProducer> = producers
    .iter()
    .filter(|p| p.detect(workspace))
    .map(|boxed| boxed.as_ref())
    .collect();

match matched.as_slice() {
    [] => return Err(NoProducerDetected {
        workspace,
        compiled_in: producers.iter().map(|p| p.name()).collect(),
    }.into()),
    [single] => {
        let (nodes, edges) = single.produce(workspace)?;
        ingest(nodes, edges)
    }
    [first, ..] => {
        // Polyglot — v0.1 picks the first-detected language and warns.
        // A future flag (--lang) lets the user override; deferred to a
        // follow-up if PHP+TS monorepos surface real pain.
        emit_warning(format!(
            "polyglot workspace; v0.1 trait dispatches `{}`. \
             Override with --lang once that flag ships.",
            first.name()
        ));
        let (nodes, edges) = first.produce(workspace)?;
        ingest(nodes, edges)
    }
}
```

`available_producers()` is the only place that names concrete producer types; it lives in the `crates/cfdb-cli/src/lang.rs` module and builds the registry from compiled-in features:

```rust
// crates/cfdb-cli/src/lang.rs
use cfdb_lang::LanguageProducer;

pub(crate) fn available_producers() -> Vec<Box<dyn LanguageProducer>> {
    let mut v: Vec<Box<dyn LanguageProducer>> = Vec::new();
    #[cfg(feature = "lang-rust")]
    v.push(Box::new(cfdb_extractor::RustProducer));
    #[cfg(feature = "lang-php")]
    v.push(Box::new(cfdb_extractor_php::PhpProducer));
    #[cfg(feature = "lang-typescript")]
    v.push(Box::new(cfdb_extractor_ts::TypeScriptProducer));
    v
}
```

**`cfdb-extractor` becomes optional in `cfdb-cli`'s manifest.** Today `cfdb-extractor` is an unconditional dep at `crates/cfdb-cli/Cargo.toml:21`. Post-RFC the line becomes:

```toml
# crates/cfdb-cli/Cargo.toml
[dependencies]
cfdb-extractor = { path = "../cfdb-extractor", optional = true }
cfdb-lang      = { path = "../cfdb-lang" }

[features]
default     = ["lang-rust"]
lang-rust   = ["dep:cfdb-extractor"]
# Future: lang-php = ["dep:cfdb-extractor-php"], lang-typescript = ["dep:cfdb-extractor-ts"]
```

The `dep:` prefix is **load-bearing under resolver v2** (rust-systems R2 factual correction). The workspace uses `resolver = "2"` (`Cargo.toml:2`); the bare name `"cfdb-extractor"` in a feature list enables a *feature named `cfdb-extractor`* on any dep that exposes one, NOT the optional dep itself. Every existing optional-dep feature in `cfdb-cli` correctly uses `dep:` — see `crates/cfdb-cli/Cargo.toml:52`: `hir = ["dep:cfdb-hir-extractor", "dep:cfdb-hir-petgraph-adapter"]`. Slice 41-C must mirror this idiom.

Without this `optional = true` change, `cargo build --no-default-features` would still pull in `cfdb-extractor` and its `syn` / `cargo_metadata` dep tree, defeating the slim-build purpose (rust-systems R1). The slim-build invariant is verified in CI (Slice 41-C `Tests:`) via `cargo check -p cfdb-cli --no-default-features` — the pattern already used for `cfdb-recall` at `.gitea/workflows/ci.yml:143`.

**Default features.** `cfdb-cli`'s `[features] default = ["lang-rust"]` preserves backward-compat — the binary built with `cargo install` (no `--features`) behaves exactly as today.

### 3.5 Why `Box<dyn LanguageProducer>` and not generics

The CLI must hold a heterogeneous registry of producers compiled in by feature flags. Generics would require monomorphising over a tuple of feature-gated types, which is structurally awkward (cannot conditionally include a generic type parameter). Trait objects are the standard idiom for plugin registries; the runtime cost of one virtual dispatch per producer per CLI invocation is irrelevant to produce-time latency (dominated by the syn parse).

---

## 4. Invariants

### Determinism (RFC-cfdb §6.8)

- `RustProducer::produce` produces byte-identical canonical-dump SHAs to the existing `cfdb_extractor::extract_workspace` on the same workspace. Verified by `ci/determinism-check.sh` (which runs the legacy entry point) AND a new test that compares the two paths' outputs on the cfdb-self workspace.

### Recall (cfdb-recall vs rustdoc-json)

- `cfdb-recall` continues to call `cfdb_extractor::extract_workspace` (the legacy shim), unchanged. The trait's `RustProducer::produce` produces the same fact set, so recall ratios are unchanged. No new corpus extension required (the schema vocabulary is unchanged).

### No-ratchet (CLAUDE.md §6 row 5 / project §3)

- No baseline file, no allowlist file. Producers emit deterministically; thresholds for downstream gates (recall, dogfood) live as `const` in tool source per the existing rule.

### Keyspace backward-compat

- `cfdb extract --workspace <rust-workspace>` produces the **same JSON keyspace bytes** before and after the trait migration. Verified by the determinism + cross-version test above.

### Schema vocabulary (closed-set per ddd-specialist R1)

- `cfdb-core::SchemaVersion` is **unchanged** by Phase 1. The v0.1 trait does NOT widen the `:Item.kind` enum unilaterally.
- **`:Item.kind` is a schema-governed closed set.** Allowed values for v0.1 are exactly the values today's `cfdb-extractor` emits (`"struct"`, `"enum"`, `"trait"`, `"fn"`, `"impl_block"`, `"const"`, `"static"`, `"type"`, `"mod"`, etc. — the closed set rooted in `crates/cfdb-extractor/src/item_visitor/`). Concrete producers MUST NOT emit `:Item.kind` values outside this set.
- **Adding a new `kind` value is RFC-gated.** Future languages (`"interface"` for TS, `"trait_php"` for PHP traits) require: (a) a separate RFC bumping `cfdb-core::SchemaVersion` patch + adding a `LABEL_*` const in `crates/cfdb-core/src/schema/labels.rs` mirroring the existing `CONST_TABLE` / `RFC_DOC` pattern (`labels.rs:54-61`), and (b) a lockstep PR on `graph-specs-rust` per RFC-033 §4 I2. The producer-side trait does not get to extend the schema vocabulary by side effect.
- **Published Language invariant** (per ddd-specialist R1): `cfdb-core` owns the published language all consumer contexts speak. `cfdb-lang` (producer-side seam) does not own schema vocabulary — that distinction is what makes the two-crate split clean-arch coherent (§3.2).

### Backward-compat for qbot-core / graph-specs-rust

- `cfdb_extractor::extract_workspace` (the legacy fn) stays public + binary-compatible. `cargo install --git` consumers see no change.

---

## 5. Architect lenses

### 5.1 Clean architecture — **R1 verdict: REQUEST CHANGES** (3 change requests, all applied in R2)

> R1 clean-arch verdict (`team-lead@rfc-041-council` mailbox, 2026-04-28): **REQUEST CHANGES**. (1) Move `available_backends()` (now `available_producers()`) into a dedicated `crates/cfdb-cli/src/lang.rs` module — composition-root concern separated from command handler. Applied in §3.4. (2) Add Non-goal: ExtractError deprecation timeline is deferred — applied in §6. (3) Reframe HIR carve-out as "deferred architectural decision" not "non-goal" — applied in §6.

- **Trait placement** — `cfdb-lang` is correct (clean-arch R1 ratified the SAP argument; metrics in §3.2 table). `cfdb-core`'s two existing port traits are *consumer-side* (storing, enriching); `LanguageProducer` is a *producer-side seam* with a different stability profile. Putting them in one crate would mix change-reasons (consumer-side schema vs producer-side extension), violating SRP at the crate level.
- **Composition root** — `cfdb-cli` is correct, but `available_producers()` MUST live in `cfdb-cli/src/lang.rs` (NOT in `commands/extract.rs`) so the registry concern is separated from the command handler. R2 §3.4 applies this.
- **Dependency-rule check** — every arrow points inward: `cfdb-cli → cfdb-lang → cfdb-core` and `cfdb-cli → cfdb-extractor → cfdb-lang → cfdb-core`. The composition root (`cfdb-cli/src/lang.rs`) is the only place that names concrete producer types — DIP satisfied.
- **Backward-compat shim purity** — `cfdb_extractor::extract_workspace` is a concrete adapter on a concrete crate; keeping it public does not violate the dependency rule. The dual `ExtractError` / `LanguageError` public-error-type asymmetry is a known debt — its deprecation timeline is now an explicit Non-goal in §6 rather than implicit by silence.
- **HIR carve-out** — reframed in §6 as a *deferred architectural decision* (revisit when PHP/TS HIR resolution stories are understood) rather than a permanent non-goal.

### 5.2 Domain-driven design — **R1 verdict: REQUEST CHANGES** (4 change requests, all applied in R2)

> R1 ddd-specialist verdict (`team-lead@rfc-041-council` mailbox, 2026-04-28): **REQUEST CHANGES**. (1) Rename `LanguageBackend` → `LanguageProducer` to escape `StoreBackend` / `EnrichBackend` homonym — applied in §3.1 + propagated. (2) Rename trait method `extract()` → `produce()` to escape four-way `extract` homonym — applied. (3) Declare bounded-context name (`language production`) in the concepts spec (Slice 41-A) — applied. (4) Constrain `:Item.kind` to a schema-governed closed set (Published Language invariant) — applied in §4.

- **Naming.** `Backend` was an overloaded suffix in this repo (consumer-side persistence + consumer-side enrichment). R2 ubiquitous-language word for "language-specific producer" is **producer**. Trait name = `LanguageProducer`. Concrete implementor for Rust = `RustProducer`. Future = `PhpProducer`, `TypeScriptProducer`. Consistent across the trait, error enum, dispatcher fn (`available_producers`), and CLI error variant (`NoProducerDetected`).
- **Method name.** R1's `extract()` collided with three sibling sites (CLI verb, crate name, legacy fn). R2 method = `produce()`. The CLI verb `cfdb extract` and the crate name `cfdb-extractor` are NOT renamed (backward-compat); the trait method IS renamed because it is the cheapest escape point.
- **Bounded context.** `cfdb-lang` owns the **language production** context — distinct from `cfdb-core`'s schema-vocabulary context and `cfdb-extractor`'s Rust-producer context. Slice 41-A captures this in `specs/concepts/cfdb-lang.md` so the cfdb extractor's `BELONGS_TO -> :Context` resolves to meaningful vocabulary.
- **`:Item.kind` extensibility.** Closed-set per §4 — Published Language invariant. New kind values (`"interface"` for TS, `"trait_php"` for PHP) require a separate schema RFC bumping `cfdb-core::SchemaVersion` patch + adding a `LABEL_*` const in `crates/cfdb-core/src/schema/labels.rs` mirroring the existing `CONST_TABLE` / `RFC_DOC` pattern. Producers do not get to extend the schema by side effect.
- **`Detection` as bare `bool`.** YAGNI ratified — the boolean has no identity, no invariant, no behavior; promoting it to a value object adds machinery without payoff.

### 5.3 SOLID + component principles — **R1 verdict: RATIFY** (with one CCP correction, applied below)

> R1 solid-architect verdict (`team-lead@rfc-041-council` mailbox, 2026-04-28): **RATIFY** — full SAP table verified, ISP satisfied (3-method utilization = 100%), SRP satisfied, OCP open/closed verified. One clarification request: the R1 wording "Adding `lang-php` does not modify `cfdb-lang`" overclaimed CCP — concrete-producer registration also touches `cfdb-cli/Cargo.toml` (feature flag) and `cfdb-cli/src/lang.rs` (registry entry). Corrected below.

**ISP — 3-method shape is minimum-viable, not fat.** `name()`, `detect()`, and `produce()` are logically inseparable for any producer that participates in the `available_producers()` registry. `cfdb-cli` uses all three in §3.4's dispatch loop; future concrete producers implement all three. Utilization = 100%. A "detect-only" split trait would force the dispatcher to hold two `Box<dyn _>` per producer with no cohesion gain. (No ISP violation.)

**SRP — one reason to change.** The trait changes when the contract between the dispatcher and a producer changes. Adding language X changes only the *concrete crate*; the trait signature is stable under extension. `name()` belongs on the same trait as `produce()` because both are part of the dispatcher contract.

**SAP — `cfdb-lang` placement wins decisively over `cfdb-core` (metrics in §3.2 table).** `cfdb-lang` D = 0.17 (Zone of Usefulness); `cfdb-core` placement would push the schema-vocabulary ring to D = 0.78 (Zone of Pain). As more producers are added, `cfdb-lang`'s instability `I` decreases — strengthening the abstraction.

**CCP — partial violation in the registry pattern, accepted explicitly (R2 correction).** The R1 wording was wrong: adding `lang-php` modifies THREE files — (1) the new `cfdb-extractor-php` crate, (2) `cfdb-cli/Cargo.toml` for the feature flag, AND (3) `cfdb-cli/src/lang.rs`'s `available_producers()` function (§3.4). The dispatcher's `#[cfg(feature = "lang-php")] v.push(Box::new(cfdb_extractor_php::PhpProducer))` is a CCP violation: two files in `cfdb-cli` change for the same reason (adding a language). **This is the accepted cost of compile-time feature-flag dispatch**: a procedural-macro or `inventory`-style registry would eliminate it but adds unsafe complexity that exceeds the benefit at three languages. The blast radius is bounded to `cfdb-cli` (the composition root); the trait crate `cfdb-lang` and existing concrete producers remain untouched.

**OCP — open for extension, closed for modification.** Existing `RustProducer` is untouched when PHP/TS land. The `cfdb-lang` trait is closed; new producers open it via `impl LanguageProducer`. The `extract_workspace` shim (§3.3) is a DIP-conformant adapter that delegates inward; the dependency flows from the concrete shim toward the trait, not the reverse.

**Producer-name string placement.** String literals like `"rust"` live on the concrete `RustProducer` in `cfdb-extractor` (returned by `name()`), not in `cfdb-lang`. `cfdb-lang` does not encode any concrete producer name — the CLI dispatcher reads `p.name()` at runtime. No analogous threshold-const issue to RFC-039 §5.3 arises here.

### 5.4 Rust systems — **R1 verdict: REQUEST CHANGES** (3 change requests + 2 factual corrections, all applied in R2)

> R1 rust-systems verdict (`team-lead@rfc-041-council` mailbox, 2026-04-28): **REQUEST CHANGES**. All three change requests applied — (1) drop `Sync` from supertrait → §3.1; (2) make `cfdb-extractor` `optional = true` in `cfdb-cli/Cargo.toml` explicit → §3.4 + §7 Slice 41-C; (3) add `cargo check -p cfdb-cli --no-default-features` to Slice 41-C `Tests:` → §7. Both factual corrections folded inline below.

- **Object safety** — satisfied under the actual conditions (per the rust-systems R1 factual correction): no generic methods, no `where Self: Sized` clauses, all method receivers are `&self`, no associated types. The R1 phrasing "`Send + Sync + ?Sized` is satisfied" was misleading — `?Sized` is the implicit bound on `Self` for trait objects, not a supertrait that affects safety. R2 supertrait is `Send` only (R1 over-specified `Sync`; cfdb-cli's dispatch is single-threaded sequential — `Sync` is added only when a polyglot-parallel-dispatch design surfaces).
- **Feature flags** — `cfdb-cli` gains `lang-rust = ["cfdb-extractor"]` default-on; `cfdb-extractor` becomes `optional = true` (§3.4). Slim build (`cargo check -p cfdb-cli --no-default-features`) compiles without `cfdb-extractor` and its `syn` / `cargo_metadata` / `ra-ap-*` transitives — verified in CI per Slice 41-C `Tests:`. `available_producers()` returns an empty `Vec` in that build; `cfdb extract --workspace ...` returns `NoProducerDetected` cleanly (no panic).
- **Orphan rules** — `impl LanguageProducer for RustProducer` in `cfdb-extractor` is valid: the trait is foreign (defined in `cfdb-lang`), the struct is local. Future producer crates (`cfdb-extractor-php` owning `PhpProducer`) follow the same pattern — each producer crate owns its concrete struct, no orphan rule risk.
- **Compile cost** — new `cfdb-lang` crate is tiny (1 trait, 1 error enum, 2 deps: `cfdb-core` + `thiserror`). Warm incremental: sub-second. **Cold build** of `thiserror` (workspace `thiserror = "2"` proc-macro at `Cargo.toml:28`) is slightly larger than "sub-second" on a fully-cold target — informational, not a blocker. Adds one cargo unit; transitively the `cfdb-cli` dep graph gains one edge.
- **Streaming vs eager `produce()`** — eager `(Vec<Node>, Vec<Edge>)` mirrors today's `extract_workspace`. cfdb-self at ~19k nodes / ~22k edges = well under memory caps. A polyglot TS monorepo with 500k items would produce ~50–200 MB of `Vec<Node>` — large but not pathological for a single-shot CLI. Streaming via `Box<dyn Iterator<Item = Fact>>` would break object safety (the iterator's associated `Item` type can't be named through `dyn`); idiomatic alternative is a `&mut dyn FnMut(Fact)` callback. Deferred to a follow-up RFC; `Vec` is acceptable for v0.1.
- **Backward-compat** — `cfdb_extractor::extract_workspace` at `crates/cfdb-extractor/src/lib.rs:91` retains the exact wire signature `(&Path) -> Result<(Vec<Node>, Vec<Edge>), ExtractError>`. ABI stable; qbot-core's `cargo install --git` consumers see zero API change.
- **Dep-graph blast radius** — new edges: `cfdb-lang -> {cfdb-core, thiserror}`; `cfdb-extractor -> cfdb-lang`; `cfdb-cli -> cfdb-lang` (transitively via `cfdb-extractor` when `lang-rust` is on; directly when `lang-rust` is off). No cycle risk. No interaction with existing `hir` / `git-enrich` / `quality-metrics` features (those gate `cfdb-cli -> cfdb-hir-extractor` etc., orthogonal to the language axis).

---

## 6. Non-goals + deferred architectural decisions

### Non-goals (out of scope, not deferred)

- **No PHP or TypeScript producer.** Phase 2 / 3 (#264 / #265).
- **No polyglot-repo merge semantics.** v0.1 = one producer per invocation.
- **No streaming `produce()`** — eager `(Vec<Node>, Vec<Edge>)` is sufficient at v0.1 scale (~19k nodes / ~22k edges on cfdb-self per rust-systems R1). Streaming via callback is the documented follow-up shape if profiling identifies a memory bound.
- **No removal of `extract_workspace` public fn.** It stays as a backward-compat shim for qbot-core's `cargo install --git` consumer.
- **No `Detection` struct.** YAGNI per ddd-specialist R1; the `bool` is sufficient.
- **No multi-producer output merging.** Per-language keyspaces is the deferred design.
- **No `Sync` supertrait bound on `LanguageProducer`.** R1 had it; R2 dropped it per rust-systems — cfdb-cli's dispatch is single-threaded sequential. Re-add only when a polyglot-parallel-dispatch design surfaces.

### Deferred architectural decisions (revisit when prerequisites land)

- **HIR-side trait abstraction.** `cfdb-hir-extractor` stays Rust-specific in Phase 1. Revisit when PHP (#264) and TypeScript (#265) have shipped and their HIR-resolution stories (TS module resolution + ts-morph; PHP autoload chains + nikic/php-parser) are concrete. The deferral is per clean-arch R1 — premature trait abstraction over an unknown HIR shape would produce a leaky abstraction.
- **`ExtractError` deprecation timeline.** The R2 design publishes `ExtractError` (legacy) AND `LanguageError` (new) as parallel public error types. The legacy fn `extract_workspace` returns `ExtractError` for binary compat; the trait method `produce` returns `LanguageError`. Long-term consolidation is needed but not at Phase 1 — qbot-core's `cargo install --git` CI consumer would break on a sudden removal. Revisit when (a) qbot-core migrates to `cargo install` of a cfdb release artifact rather than git-pinned source, OR (b) the v1.0 split (EPIC #279) lands and provides a clean migration shim. Per clean-arch R1.
- **Bounded-context-aware producer registry.** Today `available_producers()` is a static feature-flag-gated `Vec`. A future polyglot story might want runtime-discovered producers (plug-in DLLs, scriptable detectors). Out of scope for Phase 1.

---

## 7. Issue decomposition

Vertical slices, one issue each, each carries an explicit `Tests:` line per project §2.5.

### Slice 41-A — `cfdb-lang` crate + trait + error enum (NEW)

`crates/cfdb-lang/{Cargo.toml, src/lib.rs}`. Adds the `LanguageProducer` trait + `LanguageError` enum with thiserror derive. No producer impls. No CLI changes. Also adds `specs/concepts/cfdb-lang.md` declaring the **language production** bounded context + the `LanguageProducer` / `LanguageError` pub types (per ddd-specialist R1 — bounded context is named, not just heuristic).

```
Tests:
  - Unit: object-safety pin (`fn _assert_obj_safe(_: &dyn LanguageProducer) {}`)
  - Unit: `Send` bound pin (`fn _assert_send(_: Box<dyn LanguageProducer>) where Box<dyn LanguageProducer>: Send {}`) — catches accidental `Sync` reintroduction or accidental loss of `Send`
  - Self dogfood (cfdb on cfdb): N/A — no schema or behavioural change
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): `make graph-specs-check` confirms the new pub trait + enum carry concepts spec entries (`specs/concepts/cfdb-lang.md`); cross-dogfood returns zero rule-row delta
  - Target dogfood: N/A — no public verb change
```

### Slice 41-B — `RustProducer` impl in `cfdb-extractor`

`crates/cfdb-extractor/src/lib.rs` + `Cargo.toml`. Adds `pub struct RustProducer` + `impl LanguageProducer for RustProducer`. The existing `pub fn extract_workspace` becomes a thin delegating shim (preserved for qbot-core CI). Adds `cfdb-lang` to `[dependencies]`.

```
Tests:
  - Unit: `LanguageProducer::detect(&RustProducer, tmp_dir)` returns true iff `Cargo.toml` exists at the root + false otherwise (3 fixtures: empty dir, dir with random.txt, dir with Cargo.toml)
  - Unit: `RustProducer.produce(cfdb-self)` produces byte-identical canonical-dump SHA to `extract_workspace(cfdb-self)` (the trait method is a no-op wrapper over the existing path; this pin catches accidental divergence and `LanguageError` translation drift)
  - Self dogfood (cfdb on cfdb): the existing `ci/determinism-check.sh` continues to run unchanged via the shim — invariant preserved
  - Cross dogfood: `make graph-specs-check` confirms `RustProducer` carries a concepts spec entry; cross-dogfood returns zero rule-row delta
  - Target dogfood: N/A — no public CLI change
```

### Slice 41-C — `cfdb-cli` dispatcher + feature flags + CLI extract refactor

Three coordinated changes:

1. **`crates/cfdb-cli/Cargo.toml`** — `cfdb-extractor` becomes `optional = true`; new `cfdb-lang` dep is unconditional; `[features]` adds `default = ["lang-rust"]` and `lang-rust = ["cfdb-extractor"]` (per rust-systems R1).
2. **NEW `crates/cfdb-cli/src/lang.rs`** — composition-root module hosting `pub(crate) fn available_producers()` + the `NoProducerDetected` error type (per clean-arch R1; mirrors the existing `compose::empty_store()` separation pattern). The function is the only place that names concrete producer types.
3. **`crates/cfdb-cli/src/commands/extract.rs`** — switches from direct `cfdb_extractor::extract_workspace` to `crate::lang::available_producers()` + dispatch loop per §3.4.

```
Tests:
  - Unit: `available_producers()` returns exactly one entry under `default-features` (the `RustProducer`); `name()` == "rust"
  - Slim-build CI step: `cargo check -p cfdb-cli --no-default-features` compiles clean with NO `cfdb-extractor` in the dep tree (mirrors the cfdb-recall pattern at `.gitea/workflows/ci.yml:143`); test surface is the CI workflow YAML — applied via PR-time `Check` job extension. Pin: under `--no-default-features`, `cfdb extract --workspace <any>` returns `NoProducerDetected` cleanly (no panic).
  - Integration: `cfdb extract --workspace <rust-workspace>` produces byte-identical output before/after the dispatcher refactor (same fact set; same keyspace JSON SHA)
  - Self dogfood: full PR-time CI pipeline (`ci.yml` already runs `cfdb extract` on cfdb-self for the self-audit step) — must stay green
  - Cross dogfood: `ci/cross-dogfood.sh` (RFC-033) continues to pass on graph-specs-rust at pinned SHA — zero rule-row delta
  - Target dogfood: report `cfdb extract` wall-time delta before/after in the PR body (regression budget: <5% on cfdb-self extract). Also report the slim-build `cargo check -p cfdb-cli --no-default-features` wall-time as a baseline for future feature-flag drift.
```

### Slice 41-D — README repositioning + concepts spec entries

`README.md` "What cfdb extracts" gains a "Languages: Rust (v0.1 reference)" sub-row pointing at the trait + the multi-language follow-ups. `specs/concepts/cfdb-lang.md` (created in Slice 41-A) is extended with `RustProducer` entry referencing `crates/cfdb-extractor/src/lib.rs:<line>`. The bounded-context name (**language production**) is captured in the spec.

```
Tests: none — rationale: docs-only README change + concepts spec entries; verified by `make graph-specs-check` returning 0 violations on both `specs/concepts/` and `specs/tools/` passes
```

---

## 8. Refs

- META #266 — cfdb multi-language roadmap
- Issue #263 — Phase 1 (this RFC's implementing slice)
- Issues #264 / #265 — PHP / TS MVPs (depend on this RFC)
- EPIC #279 — cfdb v1.0 split (constrains backward-compat)
- RFC-033 — cross-dogfood + lockstep policy (constrains schema bumps)
- `crates/cfdb-extractor/src/lib.rs:91` — current `extract_workspace` entry point
- `crates/cfdb-cli/src/commands/extract.rs:49` — current call site
