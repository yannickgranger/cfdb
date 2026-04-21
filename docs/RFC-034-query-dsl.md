# RFC-034 — Query DSL for machine-checkable predicates

**Status:** DRAFT — awaiting architect council ratification
**Date:** 2026-04-21
**Tracking issue:** #49
**Parent:** #34 (EPIC — cfdb v0.2)
**Author:** Claude (Opus 4.7)
**Council team:** `council-49-query-dsl` (to be spawned after this draft review)

---

## 1. Problem

Issue #49 calls for a **DSL** that `check-prelude-consistency` (qbot-core Study 003 §15c) uses to execute machine-checkable Non-negotiable predicates. The canonical predicate shape from the qbot-core DDD council is:

```
context-query: crate IN context-map[trading] AND re-exported FROM crate IN context-map[portfolio] WITHOUT TranslationAdapter impl
```

Hard constraints (qbot-core DDD council v3, 2026-04-17):

- Context-membership queries MUST consult `.cfdb/concepts/*.toml` as authority, NOT string-match on qname.
- Must support cross-crate impl-trait queries (invariant 4b patterns — depends on `IMPL_TRAIT_FOR_TYPE`, shipped in #42 → 4a3d807).
- Must compose `AND` / `OR` / `NOT`.
- Must have escape hatch to shell-grep for simple file-path checks.
- Must NOT require LLM judgment for execution (deterministic binary).

Issue #49 suggests "New crate `crates/cfdb-query-dsl/` (or extension to `cfdb-query` — decide at discovery). Bias toward thin layer if Cypher covers the predicate forms."

This RFC asks and answers: **does the existing `cfdb-query` Cypher subset already cover the predicate forms?** If yes, a new DSL grammar is scope bloat; the gap reduces to TOML-backed parameter resolution plus a named-predicate library.

## 2. Scope

### 2.1 What ships

1. **Named-predicate library** at `.cfdb/predicates/<name>.cypher` — one Cypher template per predicate, parsed by the existing `cfdb-query` chumsky parser. Templates use ordinary `$param` bindings.
2. **Context-map param resolver** — `--param context:<name>` on the CLI expands `.cfdb/concepts/<name>.toml`'s `crates = [...]` into a `Param::List` of crate names bound to `$context_<name>`.
3. **New CLI verb** `cfdb check-predicate --name <name> [--param k=v ...]` — loads `.cfdb/predicates/<name>.cypher`, resolves `context:` / `regex:` / `literal:` params, dispatches through the existing evaluator, emits the same three-column violation format as `cfdb violations`.
4. **Predicate library seed** — three canonical predicates shipped in-tree to exercise the three forms:
   - `.cfdb/predicates/context-member-reexport-without-adapter.cypher` (context-membership + impl-trait, consumes #42 edges)
   - `.cfdb/predicates/fn-returns-type-in-crate-set.cypher` (type-signature, consumes `cfdb-concepts::PublishedLanguageCrates` vocabulary)
   - `.cfdb/predicates/path-regex.cypher` (file-path fallback — `MATCH (f:File) WHERE f.path =~ $pat RETURN f.path`)
5. **Test suite** — ≥10 integration tests covering the three canonical forms plus AND/OR/NOT composition. Includes a self-dogfood test that runs every `.cfdb/predicates/*.cypher` against cfdb's own keyspace and asserts the fixed seed counts hold.
6. **Documentation** — `docs/query-dsl.md` with canonical examples + param-resolver syntax grammar + "how to add a predicate" runbook.

### 2.2 What does NOT ship

- **No new DSL grammar.** No chumsky-rewrite. No BNF. The existing Cypher subset (`MATCH` / `OPTIONAL MATCH` / `WHERE` / `WITH` / `UNWIND` / `RETURN` / `IN` / `NOT EXISTS` / `AND` / `OR` / `NOT` / regex) already composes every predicate form enumerated in #49.
- **No new crate** `crates/cfdb-query-dsl/`. The shippable surface is three touches:
  - `cfdb-cli`: new `check-predicate` verb handler + param-resolver helper
  - `cfdb-query`: param-resolver public API (read `.cfdb/concepts/*.toml` → `Param::List`); no parser changes
  - `.cfdb/predicates/`: new directory (sibling of `.cfdb/queries/` and `.cfdb/concepts/`) — seed content only
- **No new cfdb-core vocabulary.** No new `:Label`, no new edge kind, no `SchemaVersion` bump. This RFC is a CLI + template + predicate-library addition; it does not touch the wire format.
- **No `.cfdb/queries/` overlap.** `.cfdb/queries/*.cypher` are self-hosted ban rules run by `cfdb violations` in dogfood gates. `.cfdb/predicates/*.cypher` are Non-negotiable predicates run by `cfdb check-predicate` for cross-repo consistency checks. Different consumers, different files, different verbs. Naming is load-bearing; coordinate with §A.14 of `council-cfdb-wiring/RATIFIED.md`.
- **No Cypher-to-DSL translator.** Users (and `check-prelude-consistency` upstream) author predicates directly in the Cypher subset.
- **No template interpolation / composition / macros** inside `.cfdb/predicates/*.cypher`. Files are concrete Cypher with `$param` placeholders; composition is achieved by writing a new file.

## 3. Design

### 3.1 Types

**New in `cfdb-query`:**

```rust
// crates/cfdb-query/src/param_resolver.rs   (NEW FILE)

/// Error surfaced while resolving a `--param` CLI argument to a `cfdb_core::query::Param`.
#[derive(Debug, thiserror::Error)]
pub enum ParamResolveError {
    #[error("unknown param form {form:?} — expected context:<name>, regex:<pat>, literal:<value>, or list:<a,b,c>")]
    UnknownForm { form: String },

    #[error("context `{name}` not declared in .cfdb/concepts/")]
    UnknownContext { name: String },

    #[error("io error reading {path}: {source}")]
    Io { path: std::path::PathBuf, #[source] source: std::io::Error },

    #[error("toml parse error in {path}: {source}")]
    Toml { path: std::path::PathBuf, #[source] source: Box<toml::de::Error> },
}

/// Resolve a single `--param <name>=<form>:<value>` CLI argument into
/// `(param_name, Param)`. Context-valued params read `.cfdb/concepts/<value>.toml`.
pub fn resolve_param(
    workspace_root: &std::path::Path,
    cli_arg: &str,
) -> Result<(String, cfdb_core::query::Param), ParamResolveError>;

/// Resolve all `--param` CLI arguments into a `BTreeMap<String, Param>` suitable
/// for assignment to `Query::params`.
pub fn resolve_params(
    workspace_root: &std::path::Path,
    cli_args: &[String],
) -> Result<std::collections::BTreeMap<String, cfdb_core::query::Param>, ParamResolveError>;
```

**New in `cfdb-cli`:**

```rust
// crates/cfdb-cli/src/check_predicate.rs    (NEW FILE)

/// Execute the named predicate at `.cfdb/predicates/<name>.cypher` against the
/// pinned keyspace. Params from `cli_params` are resolved via cfdb-query's
/// resolver and merged into the parsed Query's `params` map.
///
/// Emits the same three-column format as `cfdb violations`: `qname | line |
/// reason`. Exit non-zero iff ≥1 row matches; CI consumers gate on this.
pub fn check_predicate(
    db: &std::path::Path,
    keyspace: &str,
    workspace_root: &std::path::Path,
    name: &str,
    cli_params: &[String],
) -> Result<PredicateRunReport, CfdbCliError>;

/// Summary of one `check-predicate` invocation: which file was loaded, how many
/// rows matched, with a deterministic list of the matches for CI rendering.
#[derive(Debug, serde::Serialize)]
pub struct PredicateRunReport {
    pub predicate_name: String,
    pub predicate_path: std::path::PathBuf,
    pub row_count: usize,
    pub rows: Vec<PredicateRow>,  // sorted by (qname, line) for determinism
}

#[derive(Debug, serde::Serialize)]
pub struct PredicateRow {
    pub qname: String,
    pub line: Option<i64>,
    pub reason: String,
}
```

### 3.2 Wire-format additions

**None.** No new Node labels, no new Edge labels, no new Item attributes, no Cypher-subset keywords, no `cfdb_core::query` AST variants. Schema vocabulary unchanged; `SchemaVersion` unchanged; no lockstep PR on graph-specs-rust required.

### 3.3 Cypher-subset additions

**None.** Every predicate shape from #49 is expressible today:

| #49 predicate form | Existing Cypher feature |
|---|---|
| `crate IN context-map[trading]` | `WHERE c.name IN $context_trading` (list param bound via resolver) |
| `AND` / `OR` / `NOT` | `Predicate::And` / `Or` / `Not` (eval/predicate.rs:40-54, all three wired) |
| `WITHOUT TranslationAdapter impl` | `WHERE NOT EXISTS { MATCH (c)-[:IMPLEMENTS_FOR]->(t:Trait) WHERE t.name = 'TranslationAdapter' }` |
| `re-exported FROM crate IN context-map[portfolio]` | MATCH path `(i:Item)-[:RE_EXPORTS]->(i2:Item)<-[:DECLARES]-(c:Crate)` + `WHERE c.name IN $context_portfolio` |
| `public fn returns Decimal in crate matching financial-precision-crates.toml` | `MATCH (f:Item) WHERE f.kind = 'fn' AND f.visibility = 'pub' AND f.ret_type = 'Decimal' AND f.crate IN $fin_precision_crates` (resolver reads `.cfdb/financial-precision-crates.toml` → `$fin_precision_crates` list) |
| `path-match: crates/ports[^/]*/src/*.rs` | `MATCH (f:File) WHERE f.path =~ $pat RETURN f.path` |

### 3.4 CLI verb signature

```
cfdb check-predicate --db <path> --keyspace <name> --name <predicate> [--param <name>:<form>:<value> ...] [--format text|json]
```

- `--name` — basename of `.cfdb/predicates/<name>.cypher` (without extension)
- `--param <name>:<form>:<value>` — repeatable. Forms:
  - `context:<concept-name>` → reads `.cfdb/concepts/<concept-name>.toml`, binds `$<name>` to `Param::List` of crate names
  - `regex:<pattern>` → binds `$<name>` to `Param::Scalar(PropValue::Str(pattern))`
  - `literal:<value>` → binds `$<name>` to `Param::Scalar(PropValue::Str(value))`
  - `list:<a,b,c>` → binds `$<name>` to `Param::List` of comma-separated strings
- `--format` — `text` (three-column, default) or `json` (emits `PredicateRunReport` for skill consumers)

### 3.5 `.cfdb/predicates/` directory layout

```
.cfdb/predicates/
├── README.md                                             (runbook — "how to add a predicate")
├── context-member-reexport-without-adapter.cypher        (seed #1)
├── fn-returns-type-in-crate-set.cypher                   (seed #2)
└── path-regex.cypher                                     (seed #3)
```

Each `.cypher` file contains a single Cypher query using `$param` bindings. First-line comment is mandatory and documents the expected `--param` forms. Example:

```cypher
// Params: $context_trading (list of crate names), $context_portfolio (list), $adapter_trait (scalar str)
// Returns: (qname, line, reason) — canonical three-column violation format.
MATCH (i:Item)-[:DECLARED_IN]->(c:Crate)
WHERE c.name IN $context_trading
  AND EXISTS {
    MATCH (i)-[:RE_EXPORTS]->(i2:Item)-[:DECLARED_IN]->(c2:Crate)
    WHERE c2.name IN $context_portfolio
  }
  AND NOT EXISTS {
    MATCH (i)-[:IMPLEMENTS_FOR]->(t:Trait)
    WHERE t.name = $adapter_trait
  }
RETURN i.qname AS qname, i.line AS line, 'cross-context re-export without adapter' AS reason
ORDER BY qname
```

## 4. Invariants

### 4.1 Determinism

**Principle:** `cfdb check-predicate --name X` on the same keyspace produces byte-identical stdout across runs.

**Mechanism:** inherited from the existing Cypher evaluator (RFC-cfdb §12.1 G1). `ORDER BY` in the template + sorted binding iteration in `cfdb-petgraph/src/eval/` guarantee stable row order. Param resolver reads TOML deterministically (sorted crate lists in `.cfdb/concepts/`).

**Proof:** `ci/determinism-check.sh`-style test — run the three seed predicates twice, assert byte-identical stdout.

### 4.2 Recall

**Not applicable.** Predicates read the fact graph; they do not extract new fact types. `cfdb-recall` corpus does not need extension.

### 4.3 No-ratchet (`~/.claude/CLAUDE.md` §6.8)

**Principle:** no baseline/ceiling/allowlist file for predicate violations. Every predicate is zero-tolerance against a hard threshold (the predicate either returns rows or it doesn't).

**Mechanism:** `cfdb check-predicate` returns exit 0 iff row count is 0. No `--baseline`, no `--ceiling`, no `--allowlist`. To raise a threshold, the predicate file itself is edited (reviewed PR).

### 4.4 Keyspace backward-compat

**Principle:** predicates read the v0.2 schema (`cfdb_core::SchemaVersion::V0_2_0`). A schema bump in a future RFC invalidates predicate files that reference removed labels/edges/attrs; each predicate file's first-line comment SHOULD cite the minimum `SchemaVersion` it targets.

**Mechanism:** this RFC does not bump SchemaVersion. Future schema-bumping RFCs MUST audit `.cfdb/predicates/*.cypher` for breaking reference changes.

### 4.5 Param-resolver hermeticity

**Principle:** the param resolver reads ONLY `.cfdb/concepts/*.toml` (and `.cfdb/financial-precision-crates.toml` once relevant). It does NOT shell out, does NOT make network calls, does NOT consult environment variables beyond the workspace root path.

**Mechanism:** `resolve_param(workspace_root, cli_arg)` has a pure-function signature. Unit tests assert `PATH=""` and absent HOME still resolve cleanly against a synthetic workspace.

### 4.6 Canonical-bypass non-regression

**Principle:** introducing `.cfdb/predicates/` does not create a second authority for context-membership that could diverge from `.cfdb/concepts/`.

**Mechanism:** the param resolver MUST read `.cfdb/concepts/*.toml` via `cfdb_concepts::load_concept_overrides` — the canonical loader shipped in #3. A second inline TOML parser in `cfdb-query` is a forbidden move.

## 5. Architect lenses

### 5.1 Clean architecture (`clean-arch`)

**Question:** Does this RFC keep `StoreBackend` trait purity? Is the dependency direction (cfdb-cli → cfdb-query → cfdb-core → cfdb-concepts) respected?

**Proposed placement:**
- Param-resolver public API: **`cfdb-query`** (new module `param_resolver`). Rationale: `cfdb-query` already consumes `toml` + reads `.cfdb/skill-routing.toml`, so reading `.cfdb/concepts/*.toml` is a natural extension. NOT in cfdb-core (cfdb-core is the pure schema vocabulary crate; TOML loaders are composition-layer concerns).
- Verb handler: **`cfdb-cli/src/check_predicate.rs`** (new sibling of `check.rs`). Rationale: mirrors the `check.rs` editorial-drift verb that already composes a Cypher template with a CLI arg.
- Composition root: **`cfdb-cli/src/main_dispatch.rs`** adds one dispatch arm. No new crate.
- `cfdb-concepts`: unchanged API; we use the existing `load_concept_overrides`.
- `cfdb-petgraph` evaluator: **unchanged**. No new predicate primitive, no new pattern kind.

**Boundary contract:** the param resolver in cfdb-query takes `&Path` + `&str`, returns `(String, Param)`; does NOT depend on cfdb-petgraph or cfdb-extractor. Direction is cfdb-cli → cfdb-query → cfdb-concepts + cfdb-core (no cycle).

**Verdict required from clean-arch:** RATIFY / REJECT / REQUEST CHANGES with evidence.

### 5.2 Domain-driven design (`ddd-specialist`)

**Question:** Does the RFC introduce bounded-context concepts coherently? Any homonym risk on `predicate`, `query`, `check`?

**Concepts introduced:**
- **Predicate (file)** — a Cypher template at `.cfdb/predicates/<name>.cypher`. Homonym with `cfdb_core::query::Predicate` (the AST node for `WHERE` expressions). Two bounded contexts: the storage context (file-level predicate template) vs the query-AST context (in-memory predicate node). Homonym-by-design. Documentation MUST surface this in `docs/query-dsl.md` to prevent confusion.
- **Check-predicate verb** — not a homonym with `cfdb check --trigger T1` (editorial-drift) from #101. Different bounded context: `check-predicate` consumes `.cfdb/predicates/`; `check` consumes `.cfdb/concepts/` for T1/T3 drift. Both verbs survive; `check-predicate` is the new one.
- **Context-map param** — "context-map[trading]" is the `.cfdb/concepts/trading.toml` crate set. The authority is `cfdb-concepts::load_concept_overrides`. NOT a new concept.

**Canonical-resolver test:** every context-membership check goes through `cfdb_concepts::load_concept_overrides`. The RFC forbids a sibling inline TOML parser.

**Verdict required from ddd-specialist:** RATIFY / REJECT / REQUEST CHANGES on the (a) predicate/Predicate homonym, (b) check/check-predicate verb split, (c) no-new-context-authority invariant.

### 5.3 SOLID + component principles (`solid-architect`)

**Question:** Crate granularity — is the "no new crate" decision justified by SRP / CCP / CRP? Is the `cfdb-query` crate taking on a new responsibility that violates SRP?

**Current `cfdb-query` responsibilities:**
1. Cypher-subset parser (chumsky)
2. Fluent builder producing the same AST
3. Debt-class inventory types (`DebtClass`, `Finding`, `ScopeInventory`)
4. Shape-lint pre-eval pass
5. `SkillRoutingTable` loader (`.cfdb/skill-routing.toml`)
6. `list_items_matching` composer

Adding the param resolver makes (7) "param resolver reading `.cfdb/concepts/*.toml`". Pattern-wise this is the same shape as (5): a TOML-backed loader that produces strongly-typed bindings. SRP violation risk is LOW; the responsibilities are all "produce `cfdb_core::Query` AST inputs from external inputs (text, fluent API, TOML, CLI)".

**Alternative:** split `cfdb-query` into `cfdb-query-parser` (1) + `cfdb-query-builder` (2) + `cfdb-query-support` (3-7). **REJECTED here**: this RFC is not the right venue for a `cfdb-query` split. If `cfdb-query` reaches a god-file / god-crate threshold (tracked by `quality-architecture`), a dedicated refactor RFC handles the split.

**Verdict required from solid-architect:** RATIFY / REJECT on the decision to extend `cfdb-query` rather than create `cfdb-query-dsl`.

### 5.4 Rust systems (`rust-systems`)

**Question:** chumsky parsing reuse, `cfdb_core::Query` AST shape, feature flags, trait object safety, compile cost.

**Parser reuse:** the existing `cfdb-query::parse(source) -> Result<Query, ParseError>` already handles every predicate form. No new parser. No new chumsky combinator. No new AST variant.

**Compile cost:** zero new proc-macro crates, zero new `syn`/`quote`/`chumsky`-heavy deps. `toml` and `thiserror` already pulled in via `cfdb-query`'s Cargo.toml. New code size ≈ 200 LOC (param resolver) + 150 LOC (verb handler) + 50 LOC tests-common + 300 LOC tests. No impact on `cfdb-core`'s compile profile.

**Feature flags:** none. `check-predicate` is available in default builds. No `#[cfg(feature = "dsl")]` gate — the feature-flag cost model is paid once per crate (evaluated at every `cargo check`); an unconditional inclusion is simpler and the surface is small.

**Trait object safety:** N/A — no new traits.

**Orphan rules:** the param-resolver returns `cfdb_core::query::Param` values constructed from `cfdb_core::fact::PropValue`. Both are upstream types in `cfdb-core`; construction is trivial and does not tangle orphan rules.

**Verdict required from rust-systems:** RATIFY / REJECT on (a) parser reuse, (b) no new feature flag, (c) compile-cost pledge.

## 6. Non-goals (explicit)

- **Not a DSL grammar.** This RFC rejects the "new DSL grammar" framing from #49's deliverables list in favour of "extend Cypher + param resolver + predicate library". This reframe is the RFC's load-bearing decision; architects who disagree should REJECT.
- **Not a new crate.** `cfdb-query-dsl` is NOT created.
- **Not a SchemaVersion bump.** `cfdb-core::SchemaVersion` stays at V0_2_0.
- **Not a new evaluator primitive.** `cfdb-petgraph/src/eval/` is unchanged.
- **Not a UDF framework.** The param resolver is a specific loader, not a generic UDF registration mechanism. A future RFC can add UDFs if a predicate form emerges that Cypher + param resolver cannot express.
- **Not a template composition system.** No `INCLUDE` / `MACRO` / `IMPORT` directives in `.cfdb/predicates/*.cypher` files. One file = one predicate; composition is a future RFC.
- **Not a Shell-grep escape hatch.** #49's "shell-grep escape hatch for simple file-path checks" is satisfied by `MATCH (f:File) WHERE f.path =~ $pat` — the predicate library uses Cypher for path regex, not a shell-out. This is a conscious re-framing of #49's constraint; architects should confirm or REJECT.
- **Not a namespacing scheme.** Predicates live flat under `.cfdb/predicates/`. No sub-directories (e.g. `.cfdb/predicates/trading/`). Naming convention is `<scope>-<noun>-<qualifier>.cypher` (hyphenated slugs).
- **Not a skill-side orchestrator.** `check-prelude-consistency` skill (qbot-core-side) consumes `cfdb check-predicate --name X` as a subprocess; the skill is out of scope for this RFC.
- **Not a qbot-core-side RFC.** This RFC governs only cfdb-side deliverables. qbot-core's `check-prelude-consistency` skill spec lives in qbot-core's RFC-Study-003.
- **Not `.cfdb/queries/` extension.** `.cfdb/queries/*.cypher` (self-hosted ban rules) remain owned by `cfdb violations`; this RFC does NOT merge the directories or re-use them.

## 7. Issue decomposition (vertical slices)

Each slice is a separately-shippable PR. Every slice carries the prescribed `Tests:` block from §2.5 of the project CLAUDE.md verbatim.

### Slice 1 — `cfdb-query::param_resolver` module

**Scope:** add `crates/cfdb-query/src/param_resolver.rs` + `ParamResolveError` + `resolve_param` + `resolve_params` public fns. Wire `toml` + `cfdb-concepts` deps (both already present). Delegate `.cfdb/concepts/*.toml` reading to `cfdb_concepts::load_concept_overrides` — no inline TOML parser.

**Tests:**
```
Tests:
  - Unit: resolve_param covers all 4 forms (context / regex / literal / list); error variants tested for UnknownForm / UnknownContext / Io / Toml; hermeticity test with PATH="" asserts no env leak; sorted output determinism.
  - Self dogfood (cfdb on cfdb): integration test `resolve_params(workspace_root=cfdb_root, ["--param", "ctx:context:cfdb"])` returns Param::List with the crates declared in .cfdb/concepts/cfdb.toml — asserts exact sorted crate list.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): unchanged — no schema/evaluator touch.
  - Target dogfood (on qbot-core at pinned SHA): none — cfdb-internal addition; qbot-core consumes through CLI verb only (slice 3).
```

### Slice 2 — `.cfdb/predicates/` directory + seed files

**Scope:** add directory with README.md (runbook) + three seed `.cypher` files (context-member-reexport-without-adapter, fn-returns-type-in-crate-set, path-regex). Each file carries the param-docs first-line comment mandated by §3.5. Zero code.

**Tests:**
```
Tests:
  - Unit: none — files-only slice.
  - Self dogfood (cfdb on cfdb): a pure-parse test iterates `.cfdb/predicates/*.cypher` and asserts every file parses with zero ParseError (evaluator not run here — that's slice 4).
  - Cross dogfood: unchanged.
  - Target dogfood: none.
```

### Slice 3 — `cfdb check-predicate` verb handler

**Scope:** add `crates/cfdb-cli/src/check_predicate.rs` + `PredicateRunReport` + `PredicateRow` + the verb wiring in `main_command.rs` (new `Command::CheckPredicate { ... }` variant) + `main_dispatch.rs` (new dispatch arm into `dispatch_typed`). Re-export `check_predicate` via `cfdb-cli` lib per the existing `check`/`violations`/`list_callers` pattern.

**Tests:**
```
Tests:
  - Unit: pure-function assertions on `PredicateRunReport` row ordering (sorted by qname asc, line asc) and JSON serialization shape.
  - Self dogfood (cfdb on cfdb): integration test invokes `cfdb check-predicate --name path-regex --param pat:literal:'cfdb-query/.*\\.rs'` against cfdb's own keyspace, asserts ≥10 File rows returned (loose lower bound — every cfdb-query source file). Loose bound keeps test stable across source growth.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): runs `cfdb check-predicate --name path-regex --param pat:literal:'.*\\.rs'` against companion, asserts exit 0 + row count matches pinned fixture — zero false positives.
  - Target dogfood (on qbot-core at pinned SHA): runs the three seed predicates against qbot-core, records row counts in the PR body for reviewer sanity. No assertion — qbot-core-side baselines are the next slice's concern.
```

### Slice 4 — predicate-library dogfood + determinism check

**Scope:** add a crate-level integration test `tests/predicate_library_dogfood.rs` in `cfdb-cli` that runs EVERY `.cfdb/predicates/*.cypher` against cfdb's own keyspace with a fixed param set, asserts fixed seed counts. Add `ci/predicate-determinism.sh` that runs each predicate twice and asserts byte-identical stdout. Wire both into `.gitea/workflows/ci.yml` as new steps (siblings of the existing `cfdb self-audit` step).

**Tests:**
```
Tests:
  - Unit: none — integration-shaped slice.
  - Self dogfood (cfdb on cfdb): predicate_library_dogfood integration test; byte-identical stdout across two runs per predicate.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): the path-regex predicate runs against the companion in the existing ci/cross-dogfood.sh, asserts zero rows (no unexpected matches).
  - Target dogfood (on qbot-core at pinned SHA): the context-member-reexport-without-adapter predicate runs against qbot-core, reports row count in PR body. No assertion; the bar is "the run succeeds, producing deterministic output" — this is the load-bearing signal for qbot-core's check-prelude-consistency skill to consume.
```

### Slice 5 — `docs/query-dsl.md` user guide

**Scope:** author `docs/query-dsl.md` with: canonical-examples gallery, param-resolver syntax table, "add a new predicate" runbook, homonym-note on `Predicate` vs predicate-file (per §5.2 DDD concern). Update `docs/RFC-cfdb.md` §11 CLI verb inventory to include `check-predicate`. Update `.cfdb/predicates/README.md` (from slice 2) to cross-reference `docs/query-dsl.md`.

**Tests:**
```
Tests:
  - Unit: none — docs.
  - Self dogfood: graph-specs anti-drift gate (RFC-030) unchanged — no pub types added to cfdb-query/cfdb-cli beyond slice 1 and 3 which handle their own spec entries.
  - Cross dogfood: unchanged.
  - Target dogfood: none.
```

### Sequencing

```
slice-1 (resolver) ───┐
                      ├─→ slice-3 (verb) ─→ slice-4 (dogfood+det)
slice-2 (predicates)──┘                              │
                                                     ↓
                                              slice-5 (docs, ships last)
```

Slices 1 and 2 can ship in parallel (no file overlap). Slice 3 blocks on both. Slice 4 blocks on 3. Slice 5 ships after 4.

## 8. Compatibility with existing skills

- **`/freshness` Step 2g** — no change. Tier-1 triggers continue to consume `check-prelude-triggers` binary.
- **`/discover`** — gains a new capability: call `cfdb check-predicate --name X --format json` to augment concept inventory with "is concept Y context-homonym-free" checks.
- **`/prescribe`** — gains the same capability for verification.
- **`check-prelude-consistency`** (qbot-core skill, out of cfdb's repo) — consumes `cfdb check-predicate --name X --format json` as a subprocess.
- **`/ship`** — no change.
- **`/gate-contract`** — gains ability to run `cfdb check-predicate --name contract-adapters-only-in-adapter-crates` if such a predicate is filed.

## 9. Open questions for the council

1. **Q-CA-1 (clean-arch):** Is the dependency direction `cfdb-cli → cfdb-query → cfdb-concepts` acceptable for introducing a TOML loader into `cfdb-query`? (cfdb-query already depends on cfdb-concepts transitively through cfdb-core-shared types.)
2. **Q-DDD-1:** Is the `Predicate` (AST node) vs `predicate` (file) homonym acceptable given both are in the `cfdb-query` bounded context? Would naming the files `.cfdb/rules/` or `.cfdb/checks/` avoid the homonym?
3. **Q-SOLID-1:** Does extending `cfdb-query` with a 7th responsibility cross the SRP threshold? Quantitative threshold missing — council to opine.
4. **Q-RS-1:** Is it safe to assume every future predicate will express as Cypher, or should the RFC leave a door open for non-Cypher predicates (shell-out, regex-on-source-text, etc.)? The "escape hatch to shell-grep" from #49 is answered here by Cypher's path regex — is that answer sufficient?
5. **Q-DDD-2 / Q-CA-2:** The `cfdb check-predicate` verb overlaps conceptually with `cfdb violations --rule <path>`. Both run a Cypher query and return a three-column violation list. Should they be merged, or kept as two verbs with different auth/binding semantics? (This RFC defaults to kept-separate; ratifying architects should confirm.)

## 10. References

- Issue #49 (this RFC's tracker).
- qbot-core RFC Study 003 v2.1 §19 Q8 (promoted blocker) — motivating consumer.
- `council/RATIFIED.md` §A.14 — `.cfdb/queries/` ownership / scope-verb ratification.
- `docs/RFC-cfdb.md` §6 (CLI inventory), §11 (wire form), §12.1 (determinism), §14 (error messages).
- `docs/RFC-cfdb-v0.2-addendum-draft.md` §A1.6 — Study 003 S2 unblock condition.
- `docs/RFC-030-anti-drift-gate.md` — neighbouring RFC pattern (gate plus predicate; this RFC generalises the template mechanism).
- `docs/RFC-033-cross-dogfood.md` §3.5 — `Tests:` block convention.
- `crates/cfdb-query/src/parser/mod.rs` — parser re-use point.
- `crates/cfdb-petgraph/src/eval/predicate.rs` — evaluator re-use point.
- `crates/cfdb-cli/src/check.rs` — prototype `check` verb for naming convention.
- `.cfdb/concepts/cfdb.toml` — seeded context-map.
- `.cfdb/skill-routing.toml` — TOML-loader precedent.
