# RFC-034 — Query DSL for machine-checkable predicates

**Status:** RATIFIED (2026-04-21) — `council/49/RATIFIED.md` seals all 4 lens verdicts
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
   - `.cfdb/predicates/context-homonym-crate-in-multiple-contexts.cypher` (context-membership set intersection, using `IN $list` list-param binding)
   - `.cfdb/predicates/fn-returns-type-in-crate-set.cypher` (type-signature, uses `IN $list` against `.cfdb/published-language-crates.toml`-derived set)
   - `.cfdb/predicates/path-regex.cypher` (file-path fallback — `MATCH (f:File) WHERE f.path =~ $pat RETURN f.path`)
5. **Test suite** — ≥10 integration tests covering the three canonical forms plus AND/OR/NOT composition. Includes a self-dogfood test that runs every `.cfdb/predicates/*.cypher` against cfdb's own keyspace and asserts the fixed seed counts hold. Includes a static schema-label check (Slice 2 per R1 synthesis C6): every `:Label` and `[:EdgeLabel]` literal in every seed `.cypher` file resolves to a known variant in `cfdb_core::schema::{Label,EdgeLabel}`.
6. **Documentation** — `docs/query-dsl.md` with canonical examples + param-resolver syntax grammar + "how to add a predicate" runbook.

### 2.2 What does NOT ship

- **No new DSL grammar.** No chumsky-rewrite. No BNF. The existing Cypher subset (`MATCH` / `OPTIONAL MATCH` / `WHERE` / `WITH` / `UNWIND` / `RETURN` / `IN` / `NOT EXISTS` / `AND` / `OR` / `NOT` / regex) already composes every predicate form enumerated in this RFC's reduced-scope seed set. Issue #49's original "re-exported FROM crate IN context-map[portfolio]" example requires a `RE_EXPORTS` edge not yet emitted (re-export resolution is Phase B / HIR per `crates/cfdb-extractor/src/type_render.rs:4`); that predicate is explicitly deferred to a future RFC (see §6).
- **No new crate** `crates/cfdb-query-dsl/`. The shippable surface is two touches (R1 C1 relocation):
  - `cfdb-cli`: new `check-predicate` verb handler + **param-resolver module** (moved here from cfdb-query per solid-architect's CRP verdict)
  - `cfdb-query`: **no changes** — the existing parser, builder, and AST are reused as-is
  - `.cfdb/predicates/`: new directory (sibling of `.cfdb/queries/` and `.cfdb/concepts/`) — seed content only
- **No new cfdb-core vocabulary.** No new `:Label`, no new edge kind, no `SchemaVersion` bump. This RFC is a CLI + template + predicate-library addition; it does not touch the wire format.
- **No `.cfdb/queries/` overlap.** `.cfdb/queries/*.cypher` are self-hosted ban rules run by `cfdb violations` in dogfood gates. `.cfdb/predicates/*.cypher` are Non-negotiable predicates run by `cfdb check-predicate` for cross-repo consistency checks. Different consumers, different files, different verbs. Naming is load-bearing; coordinate with §A.14 of `council-cfdb-wiring/RATIFIED.md`.
- **No Cypher-to-DSL translator.** Users (and `check-prelude-consistency` upstream) author predicates directly in the Cypher subset.
- **No template interpolation / composition / macros** inside `.cfdb/predicates/*.cypher`. Files are concrete Cypher with `$param` placeholders; composition is achieved by writing a new file.

## 3. Design

### 3.1 Types

**New in `cfdb-cli`** (R1 C1 relocation — previously proposed for `cfdb-query`; moved to `cfdb-cli` per solid-architect's CRP verdict: `cfdb-query` has zero runtime consumers of a filesystem-reading module, `cfdb-cli` is the sole consumer):

```rust
// crates/cfdb-cli/src/param_resolver.rs   (NEW FILE)

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
/// `(param_name, Param)`. Context-valued params read `.cfdb/concepts/<value>.toml`
/// via `cfdb_concepts::load_concept_overrides` — NEVER an inline TOML parser
/// (invariant §4.6).
pub(crate) fn resolve_param(
    workspace_root: &std::path::Path,
    cli_arg: &str,
) -> Result<(String, cfdb_core::query::Param), ParamResolveError>;

/// Resolve all `--param` CLI arguments into a `BTreeMap<String, Param>` suitable
/// for assignment to `Query::params`.
pub(crate) fn resolve_params(
    workspace_root: &std::path::Path,
    cli_args: &[String],
) -> Result<std::collections::BTreeMap<String, cfdb_core::query::Param>, ParamResolveError>;
```

**Also new in `cfdb-cli`:**

```rust
// crates/cfdb-cli/src/check_predicate.rs    (NEW FILE)

/// Execute the named predicate at `.cfdb/predicates/<name>.cypher` against the
/// pinned keyspace. Params from `cli_params` are resolved via the sibling
/// `crate::param_resolver` module and merged into the parsed Query's `params` map.
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

**None.** Every predicate shape in the reduced seed set is expressible today. Note that vocabulary MUST come from `cfdb_core::schema::{Label,EdgeLabel}` — the R1 synthesis verified that `RE_EXPORTS` and `DECLARED_IN` are NOT in the current schema (re-export tracking is Phase B / HIR).

Supported predicate shapes in the reduced seed set (R1 C2):

| Predicate form | Existing Cypher feature | Edge/Label vocabulary used |
|---|---|---|
| `crate IN context-A` | `WHERE c.name IN $context_a` (list param bound via resolver) | `:Crate` |
| `AND` / `OR` / `NOT` composition | `Predicate::And` / `Or` / `Not` (eval/predicate.rs:40-54) | — |
| `crate IN context-A AND crate IN context-B` (homonym detector) | `MATCH (c:Crate) WHERE c.name IN $context_a AND c.name IN $context_b` | `:Crate` |
| `WITHOUT <Trait> impl` | `WHERE NOT EXISTS { MATCH (i)-[:IMPLEMENTS_FOR]->(t) WHERE t.name = '<Trait>' }` (inner WHERE is Compare-only, supported) | `[:IMPLEMENTS_FOR]` |
| `fn signature contains type T in crate set` | `MATCH (f:Item) WHERE f.kind = 'fn' AND f.signature =~ $type_pattern AND f.crate IN $fin_precision_crates` | `:Item` with `kind`/`signature`/`crate` props |
| `path-match: <regex>` | `MATCH (f:File) WHERE f.path =~ $pat RETURN f.path` | `:File` with `path` prop |

**Deferred to future RFC (re-export vocabulary — not expressible today):**

| Predicate form | Reason deferred | Future requirement |
|---|---|---|
| `re-exported FROM crate IN context-B` | No `RE_EXPORTS` edge in schema today; re-export resolution is RFC §8.2 Phase B (HIR) per `crates/cfdb-extractor/src/type_render.rs:4` | Separate RFC adding `RE_EXPORTS` edge emission (HIR-backed) |

**Schema-reference static check (R1 C6):** Slice 2 ships a unit test that walks every seed `.cypher` AST and asserts every `:Label` and `[:EdgeLabel]` literal resolves to a known variant in `cfdb_core::schema::{Label,EdgeLabel}`. This prevents future predicate files from shipping with typo'd or out-of-schema vocabulary.

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

Each `.cypher` file contains a single Cypher query using `$param` bindings. First-line comment is mandatory and documents the expected `--param` forms. Example (seed #1 — `context-homonym-crate-in-multiple-contexts.cypher`), revised per R1 C2 to use real schema vocabulary and supported parser constructs:

```cypher
// Params: $context_a (list of crate names), $context_b (list of crate names)
// Returns: (qname, line, reason) — canonical three-column violation format.
// Purpose: detect a Crate whose name appears in the crate-set of BOTH contexts —
//          a candidate context-homonym flagged for manual DDD review.
MATCH (c:Crate)
WHERE c.name IN $context_a
  AND c.name IN $context_b
RETURN c.name AS qname, 0 AS line, 'crate is a member of both contexts — candidate homonym' AS reason
ORDER BY qname
```

Second seed example (`path-regex.cypher` — file-path fallback, demonstrates `=~` regex against a scalar param):

```cypher
// Params: $pat (scalar regex string)
// Returns: (qname, line, reason) — path matches emitted as `qname` for uniform output shape.
MATCH (f:File)
WHERE f.path =~ $pat
RETURN f.path AS qname, 0 AS line, 'file path matched regex' AS reason
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

**Question:** Does this RFC keep `StoreBackend` trait purity? Is the dependency direction respected?

**Proposed placement (R1 C1 + C3 revision):**
- **Param-resolver module: `cfdb-cli/src/param_resolver.rs`** (moved from `cfdb-query` per solid-architect CRP verdict — R1 C1). Rationale: `cfdb-query` has zero runtime consumers that use a filesystem-reading module; `cfdb-cli` is the sole consumer. Placing the resolver in `cfdb-query` forces every future `cfdb-query` consumer to accept a `cfdb-concepts` dep they do not need. CRP wins the tie-break; SDP direction is acyclic either way.
- Verb handler: `cfdb-cli/src/check_predicate.rs` (new sibling of `check.rs`). Mirrors the `check.rs` editorial-drift verb that already composes a Cypher template with a CLI arg.
- Composition root: `cfdb-cli/src/main_dispatch.rs` adds one dispatch arm. No new crate.
- **New direct dep:** `crates/cfdb-cli/Cargo.toml` gains `cfdb-concepts = { path = "../cfdb-concepts" }`. Confirmed via `grep cfdb-concepts crates/cfdb-cli/Cargo.toml` (currently absent). The prior RFC draft's claim that `cfdb-concepts` was "already present" as a dep of `cfdb-query` was factually incorrect (R1 C3 retraction).
- `cfdb-query`: **unchanged**. No new module, no new dep, no new responsibility. The parser / builder / inventory / shape_lint / skill_routing / list_items surface is preserved at 6 responsibilities.
- `cfdb-concepts`: unchanged API; we use the existing `load_concept_overrides`.
- `cfdb-petgraph` evaluator: unchanged. No new predicate primitive, no new pattern kind.

**Boundary contract:** the param resolver in cfdb-cli takes `&Path` + `&str`, returns `(String, Param)`; depends on `cfdb-concepts` + `cfdb-core::query` + `toml`. Direction is `cfdb-cli → {cfdb-concepts, cfdb-core, cfdb-query, cfdb-petgraph, ...}` (no cycle — cfdb-cli already sits above all library crates).

**Clean-arch verdict (R1):** RATIFY with editorial corrections; solid-architect's CRP tie-break adopted. See `council/49/verdicts/clean-arch.md`.

### 5.2 Domain-driven design (`ddd-specialist`)

**Question:** Does the RFC introduce bounded-context concepts coherently? Any homonym risk on `predicate`, `query`, `check`?

**Concepts introduced:**
- **Predicate (file)** — a Cypher template at `.cfdb/predicates/<name>.cypher`. Homonym with `cfdb_core::query::Predicate` (the AST node for `WHERE` expressions). Two bounded contexts: the storage context (file-level predicate template) vs the query-AST context (in-memory predicate node). Homonym-by-design. Documentation MUST surface this in `docs/query-dsl.md` to prevent confusion.
- **Check-predicate verb** — not a homonym with `cfdb check --trigger T1` (editorial-drift) from #101. Different bounded context: `check-predicate` consumes `.cfdb/predicates/`; `check` consumes `.cfdb/concepts/` for T1/T3 drift. Both verbs survive; `check-predicate` is the new one.
- **Context-map param** — "context-map[trading]" is the `.cfdb/concepts/trading.toml` crate set. The authority is `cfdb-concepts::load_concept_overrides`. NOT a new concept.

**Canonical-resolver test:** every context-membership check goes through `cfdb_concepts::load_concept_overrides`. The RFC forbids a sibling inline TOML parser.

**Verdict required from ddd-specialist:** RATIFY / REJECT / REQUEST CHANGES on the (a) predicate/Predicate homonym, (b) check/check-predicate verb split, (c) no-new-context-authority invariant.

### 5.3 SOLID + component principles (`solid-architect`)

**Question (R1 re-framing):** Since R1 C1 moves the param resolver OUT of `cfdb-query` and into `cfdb-cli`, the original SRP-threshold concern about `cfdb-query` growing a 7th responsibility is RETRACTED. The question becomes: does `cfdb-cli` gaining a param-resolver module violate SRP or CRP?

**Current `cfdb-cli` responsibilities:**
- Binary entry + dispatch (`main.rs`, `main_dispatch.rs`, `main_command.rs`, `main_parse.rs` post-#128)
- Per-verb modules (`check.rs`, `commands.rs`, `compose.rs`, `enrich.rs`, `error.rs`, `hir.rs`, `scope.rs`, `stubs.rs`)
- The verbs themselves share one common axis of change: "dispatch CLI args → invoke cfdb library → format output"

Adding `param_resolver.rs` is on the same axis — it is one step in the "dispatch CLI args" phase, specifically for resolving `--param` CLI flags into `cfdb_core::query::Param` values. No SRP violation.

**`cfdb-query` responsibilities stay at 6:** parser, builder, inventory, shape_lint, SkillRoutingTable loader, list_items_matching. No R1 change here.

**CRP justification (solid-architect R1):** components reused together stay together. `param_resolver` is reused only by `check-predicate` (this RFC) — which lives in `cfdb-cli`. No other crate today uses it; no other crate projects to use it. Placing it in `cfdb-cli` keeps the reuse group tight.

**Rejected alternatives:**
- New sub-crate `cfdb-param-resolver/` — overkill for ~200 LOC with one consumer. If future consumers emerge, a follow-up RFC carves out a micro-crate.
- Extension of `cfdb-query` — rejected by solid-architect CRP analysis (R1 C1); would rise `cfdb-query`'s instability metric from 0.33 to ~0.50 and add a `cfdb-concepts` dep onto every future `cfdb-query` consumer.

**Solid-architect verdict (R1):** RATIFY pending confirmation of the relocation. See `council/49/SYNTHESIS-R1.md` C1.

### 5.4 Rust systems (`rust-systems`)

**Question:** chumsky parsing reuse, `cfdb_core::Query` AST shape, feature flags, trait object safety, compile cost.

**Parser reuse:** the existing `cfdb-query::parse(source) -> Result<Query, ParseError>` already handles every predicate form. No new parser. No new chumsky combinator. No new AST variant.

**Compile cost:** zero new proc-macro crates, zero new `syn`/`quote`/`chumsky`-heavy deps. `toml` and `thiserror` already pulled in via `cfdb-query`'s Cargo.toml. New code size ≈ 200 LOC (param resolver) + 150 LOC (verb handler) + 50 LOC tests-common + 300 LOC tests. No impact on `cfdb-core`'s compile profile.

**Feature flags:** none. `check-predicate` is available in default builds. No `#[cfg(feature = "dsl")]` gate — the feature-flag cost model is paid once per crate (evaluated at every `cargo check`); an unconditional inclusion is simpler and the surface is small.

**Trait object safety:** N/A — no new traits.

**Orphan rules:** the param-resolver returns `cfdb_core::query::Param` values constructed from `cfdb_core::fact::PropValue`. Both are upstream types in `cfdb-core`; construction is trivial and does not tangle orphan rules.

**Verdict required from rust-systems:** RATIFY / REJECT on (a) parser reuse, (b) no new feature flag, (c) compile-cost pledge.

## 6. Non-goals (explicit)

- **Not a DSL grammar.** This RFC rejects the "new DSL grammar" framing from #49's deliverables list in favour of "extend Cypher + param resolver + predicate library". This reframe is the RFC's load-bearing decision; ratified by all four architect lenses (R1).
- **Not a new crate.** `cfdb-query-dsl` is NOT created.
- **Not a SchemaVersion bump.** `cfdb-core::SchemaVersion` stays at V0_2_0.
- **Not a new evaluator primitive.** `cfdb-petgraph/src/eval/` is unchanged.
- **Not an extension to schema vocabulary (R1 C5).** The seed predicates use only labels/edges already in `cfdb_core::schema::{Label,EdgeLabel}`. The original issue #49 example shape ("re-exported FROM crate IN context-map[portfolio]") requires a `RE_EXPORTS` edge that does not exist and cannot be emitted by the current syn-based extractor (re-export resolution is RFC §8.2 Phase B / HIR per `crates/cfdb-extractor/src/type_render.rs:4`). That predicate is deferred to a future RFC that lands `RE_EXPORTS` edge emission.
- **Not an extension to inner-subquery WHERE grammar (R1 C5).** Cypher subqueries keep the current Compare-only inner-predicate grammar (`crates/cfdb-query/src/parser/predicate.rs:131-139`); widening to `IN` / `AND` / `OR` / `NOT` in subquery WHERE is a separate RFC. Seed predicates work within this constraint by hoisting multi-operator filters to the top-level WHERE.
- **Not positive `EXISTS { }` in the parser (R1 C5).** Only `NOT EXISTS { }` is supported (`parser/predicate.rs:43-48`, `ast.rs:147`, `eval/predicate.rs:45-47`). Seed predicates use top-level path MATCH for positive set membership instead of `EXISTS`.
- **Not a UDF framework.** The param resolver is a specific loader, not a generic UDF registration mechanism. The `eval_call` dispatch table at `crates/cfdb-petgraph/src/eval/predicate.rs:111-121` is the documented extension point when a predicate form emerges that Cypher + param resolver cannot express. Adding UDFs is a future RFC.
- **Not a template composition system.** No `INCLUDE` / `MACRO` / `IMPORT` directives in `.cfdb/predicates/*.cypher` files. One file = one predicate; composition is a future RFC.
- **Not a Shell-grep escape hatch.** #49's "shell-grep escape hatch for simple file-path checks" is satisfied by `MATCH (f:File) WHERE f.path =~ $pat` — the predicate library uses Cypher for path regex, not a shell-out. Ratified by rust-systems (R1).
- **Not a namespacing scheme.** Predicates live flat under `.cfdb/predicates/`. No sub-directories (e.g. `.cfdb/predicates/trading/`). Naming convention is `<scope>-<noun>-<qualifier>.cypher` (hyphenated slugs).
- **Not a skill-side orchestrator.** `check-prelude-consistency` skill (qbot-core-side) consumes `cfdb check-predicate --name X` as a subprocess; the skill is out of scope for this RFC.
- **Not a qbot-core-side RFC.** This RFC governs only cfdb-side deliverables. qbot-core's `check-prelude-consistency` skill spec lives in qbot-core's RFC-Study-003.
- **Not `.cfdb/queries/` extension.** `.cfdb/queries/*.cypher` (self-hosted ban rules) remain owned by `cfdb violations`; this RFC does NOT merge the directories or re-use them.
- **Not the re-export predicate from #49's issue body.** The "re-exported FROM crate IN context-map[portfolio]" predicate shape is EXPLICITLY deferred; it is unshippable until schema-level `RE_EXPORTS` emission lands. The seed predicate #1 is re-framed to "context-homonym-crate-in-multiple-contexts" which exercises the same param-resolver + composition pathway without depending on deferred edges.

## 7. Issue decomposition (vertical slices)

Each slice is a separately-shippable PR. Every slice carries the prescribed `Tests:` block from §2.5 of the project CLAUDE.md verbatim.

### Slice 1 — `cfdb-cli::param_resolver` module (R1 C1 + C4 relocation)

**Scope (R1-revised):** add `crates/cfdb-cli/src/param_resolver.rs` + `ParamResolveError` + `resolve_param` + `resolve_params` `pub(crate)` fns. Add `cfdb-concepts = { path = "../cfdb-concepts" }` to `crates/cfdb-cli/Cargo.toml` (new direct dep — currently absent per `grep cfdb-concepts crates/cfdb-cli/Cargo.toml`). Delegate `.cfdb/concepts/*.toml` reading to `cfdb_concepts::load_concept_overrides` — no inline TOML parser (invariant §4.6). `cfdb-query` is NOT touched by this slice.

**Tests:**
```
Tests:
  - Unit: resolve_param covers all 4 forms (context / regex / literal / list); error variants tested for UnknownForm / UnknownContext / Io / Toml; hermeticity test asserts no env leak; sorted output determinism.
  - Self dogfood (cfdb on cfdb): integration test in crates/cfdb-cli/tests/ — `resolve_params(workspace_root=cfdb_root, ["--param", "ctx:context:cfdb"])` returns Param::List with the crates declared in .cfdb/concepts/cfdb.toml — asserts exact sorted crate list.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): unchanged — no schema/evaluator touch.
  - Target dogfood (on qbot-core at pinned SHA): none — cfdb-internal addition; qbot-core consumes through CLI verb only (slice 3).
```

### Slice 2 — `.cfdb/predicates/` directory + seed files + schema-reference static check (R1 C6)

**Scope (R1-revised):** add directory with README.md (runbook) + three seed `.cypher` files using ONLY real schema vocabulary per R1 C2:
- `context-homonym-crate-in-multiple-contexts.cypher` (revised from original "context-member-reexport-without-adapter" — uses `IN $list` + top-level AND, no RE_EXPORTS edge)
- `fn-returns-type-in-crate-set.cypher` (uses `:Item` with `signature`/`crate` props + `IN $list`)
- `path-regex.cypher` (uses `:File` with `path` prop + `=~` regex)

Each file carries the param-docs first-line comment mandated by §3.5. Plus a new unit test at `crates/cfdb-query/tests/predicate_schema_refs.rs` (located here because it exercises the parser + schema label vocabulary — NOT a param-resolver test): iterate every `.cfdb/predicates/*.cypher`, parse each, walk the AST, assert every `:Label` and `[:EdgeLabel]` literal resolves to a known variant in `cfdb_core::schema::{Label,EdgeLabel}`. Prevents typo'd or out-of-schema vocabulary.

**Tests:**
```
Tests:
  - Unit: predicate_schema_refs — asserts every :Label / [:EdgeLabel] in every seed .cypher resolves in cfdb_core::schema (ddd-specialist R1 non-blocking request, C6).
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

## 9. Open questions — all ANSWERED by R1 council verdicts

1. **Q-CA-1 (clean-arch):** ANSWERED. Clean-arch verdict ratified the general direction; solid-architect CRP tie-break (R1 C1) RELOCATES the param resolver to `cfdb-cli`, so the question "is a TOML loader in cfdb-query acceptable?" becomes moot — the TOML loader is in cfdb-cli. `cfdb-cli → cfdb-concepts` is a new direct dep, acyclic.
2. **Q-DDD-1 (homonym):** ANSWERED. ddd-specialist verdict: acceptable homonym (same bounded context, different layers — AST node vs on-disk storage artefact). Resolution via `docs/query-dsl.md` homonym note in Slice 5.
3. **Q-SOLID-1 (SRP on cfdb-query):** ANSWERED. MOOT after R1 C1 — param resolver is no longer in cfdb-query. cfdb-cli absorbs the module with no SRP violation (same axis of change as existing verb handlers).
4. **Q-RS-1 (UDF deferral):** ANSWERED. rust-systems verdict: safe to defer. `eval_call` dispatch table at `crates/cfdb-petgraph/src/eval/predicate.rs:111-121` is the documented extension point. §6 non-goals now explicitly cites this.
5. **Q-DDD-2 / Q-CA-2 (verb split):** ANSWERED. Both ddd-specialist and clean-arch verdicts endorse keeping `cfdb check-predicate` and `cfdb violations --rule` as separate verbs: different contracts, different consumers, different change vectors. `cfdb check --trigger T1` (editorial-drift) and `cfdb check-predicate` (predicate library) likewise stay separate.

**R1 open items (for R2):**
- rust-systems R2 confirmation that R1 C2 (rewritten §3.5 example + §3.3 table) and R1 C5 (expanded non-goals) resolve Finding 1-3.
- solid-architect R2 confirmation that R1 C1 (relocation to cfdb-cli) resolves the CRP concern.

## 10. References

- Issue #49 (this RFC's tracker).
- qbot-core RFC Study 003 v2.1 §19 Q8 (promoted blocker) — motivating consumer.
- `council/RATIFIED.md` §A.14 — `.cfdb/queries/` ownership / scope-verb ratification.
- `docs/RFC-cfdb.md` §6 (CLI inventory), §11 (wire form), §12.1 (determinism), §14 (error messages).
- `docs/RFC-cfdb.md` §A1.6 — Study 003 S2 unblock condition.
- `docs/RFC-030-anti-drift-gate.md` — neighbouring RFC pattern (gate plus predicate; this RFC generalises the template mechanism).
- `docs/RFC-033-cross-dogfood.md` §3.5 — `Tests:` block convention.
- `crates/cfdb-query/src/parser/mod.rs` — parser re-use point.
- `crates/cfdb-petgraph/src/eval/predicate.rs` — evaluator re-use point.
- `crates/cfdb-cli/src/check.rs` — prototype `check` verb for naming convention.
- `.cfdb/concepts/cfdb.toml` — seeded context-map.
- `.cfdb/skill-routing.toml` — TOML-loader precedent.
