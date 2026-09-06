# RFC-042 — test/bench :EntryPoint kinds + scope --production-only flag

Status: **RATIFIED** 2026-05-17.
Author: captain (a0 session 2026-05-17)
Supersedes/relates: cfdb-029-code-facts-database (v0.2 :EntryPoint vocabulary), cfdb-032-v02-extractor (v0.2 extractor),
cfdb-037-schema-producer-alignment (schema-producer alignment); originating issue `yg/cfdb#378`.

## 1. Problem

Issue #378 (`https://agency.lab:3000/yg/cfdb/issues/378`) documents an
empirically-grounded under-reporting in the `Unwired` classifier output that
breaks the operational consumers `/sweep-epic` and `/operate-module`. The
shape of the bug, in the issue's own framing:

> Empirical: `cfdb scope --context trading --keyspace qbot-core` reports
> **2057 unwired fn/method items**. Spot-audit of the first hundred
> reveals ≥38% are reachable from **test** code — `#[test]` fns, BDD
> step-definitions (`#[given]`/`#[when]`/`#[then]`), and `criterion`
> `#[bench]` fns — and only "unreachable" because cfdb-hir-extractor
> emits `:EntryPoint` nodes for `cli_command` / `mcp_tool` / `http_route`
> / `cron_job` / `websocket` and nothing else.

The concrete canonical example from the issue: `JupiterCryptoBroker::new`
in `qbot-core/crates/qbot-trading/src/brokers/jupiter.rs`. It has no
production call site (the production wiring uses a different broker), so
the reachability BFS in `enrich/reachability.rs` never visits it from any
`{cli_command, mcp_tool, http_route, cron_job, websocket}` seed. But it
has **eight** integration tests in `crates/qbot-trading/tests/jupiter_*.rs`
exercising its full surface, and a `#[bench]` in `benches/broker_perf.rs`.
The classifier reports it as "Unwired"; the operator who triages it
deletes a tested-and-benchmarked broker; the next release regression-
tests fail.

Operational impact:

- `/sweep-epic` runs `cfdb scope --context <ctx>` and drives an
  inventory-driven cleanup. With 2057 false-positive "unwired" items in a
  single context, the LLM dispatcher either drowns or — worse — proceeds
  to mark the first batch for deletion, hitting the JupiterCryptoBroker
  failure mode above.
- `/operate-module` consults the same scope output to choose its next
  vertical-slice target; "unwired" items are deprioritised. Items that
  ARE unwired by production code but heavily test-exercised get
  systematically starved of investment.

cfdb-029-code-facts-database v0.2 §A2 predicts (line "unwired 4%") that the unwired class
should sit around 4% of items in a healthy production codebase. qbot-core
reports it at 24%. The under-counting prediction from cfdb-029-code-facts-database — that the
v0.2 `:EntryPoint` vocabulary is **incomplete** with respect to the test
graph and would inflate the apparent unwired set — is the on-ramp this
RFC fixes.

The fix is doctrinally simple: **test binaries (integration + BDD +
benches) are entry points into the codebase too.** Treating them as
first-class `:EntryPoint` nodes (with new `kind` values) means the
existing BFS in `enrich/reachability.rs` finds them automatically; the
classifier's "unwired" definition (Unwired := no `:EntryPoint` reaches
via `CALLS*`) then accurately reflects "nobody — not even a test —
exercises this code." Operators who explicitly want the production-only
view get a `--production-only` flag.

Per cfdb CLAUDE.md §1, both deliverables — a new `:EntryPoint.kind` enum
extension AND a new `cfdb scope --flag` — are RFC-first. This RFC is
that gate; issue #378 is therefore premature as a code issue and waits
for ratification before slice issues are filed (§7).

## 2. Scope

**Ships:**

- `:EntryPoint.kind` enum gains `"test"` and `"bench"` variants. The
  schema-doc text at `crates/cfdb-core/src/schema/describe/nodes.rs:296`
  is extended; the enum is documented as open per its existing semantics
  (see "Does not ship" — no SchemaVersion bump).
- `cfdb-hir-extractor` (`entry_point_emitter.rs::scan_file` FN branch)
  detects attribute-marked test functions:
  - `#[test]` (libtest)
  - `#[tokio::test]` and equivalents — any attribute whose last path
    segment is `test` other than `cfg(test)` (cfg is not in the attr
    path)
  - `#[given]`, `#[when]`, `#[then]` (cucumber-rs BDD step definitions)
- `cfdb-hir-extractor` detects `#[bench]` (criterion / libtest bench
  attribute) on `ast::Fn` nodes.
- File-location–based recognition: any `FN` declared in a file whose
  workspace-relative path starts with `tests/` (Cargo integration-test
  convention) is emitted as `kind=test`; any `FN` under `benches/` is
  emitted as `kind=bench`. This catches helper fns inside test/bench
  targets that don't carry an attribute themselves but ARE part of the
  test binary's reachability surface.
- `cfdb scope` gains a `--production-only` boolean flag (default
  `false`).
- Default `cfdb scope` semantics change: `unwired` is redefined as
  "**no `:EntryPoint` of any kind reaches via `CALLS*`**" (currently,
  the classifier query already reads `reachable_from_entry`; the BFS in
  `enrich/reachability.rs` already seeds from every `:EntryPoint`
  regardless of kind — see §3.3 design choice — so on a v0.2-only
  keyspace the default is mathematically unchanged. With this RFC's
  extractor changes the *meaning* shifts as more entry points appear).
- New synthetic-workspace fixture under `crates/cfdb-hir-extractor/tests/
  fixtures/entry_points/` covering test/bench attribute + file-location
  recognition with an `EXPECTED.md` ground truth (§3.4).
- **Feature-flag scope.** All
  new emission (`kind=test`, `kind=bench` via either attribute or
  file-location detection) requires `--features hir` on extraction,
  exactly as `kind=mcp_tool` does today. `cfdb-hir-extractor` is the
  sole producer; there is no syn-only partial path. Operators on the
  default-feature build see no new `:EntryPoint` kinds; the §2 Does NOT
  ship "backfill of pre-cfdb-042-test-bench-entry-points keyspaces" guidance applies.

**Does NOT ship (see §6):**

- `criterion_group!` / `criterion_main!` macro-call detection. Bench
  fns are detected via `#[bench]` attribute OR `benches/` file location;
  criterion's group-registration macro expands to a runner that the
  HIR-pass cannot see as a literal fn declaration without macro
  expansion. Deferred.
- HTTP/cron parity for test attributes. A `#[tokio::test]` fn that
  internally constructs an axum `Router::route("/health", health)` does
  NOT also emit `:EntryPoint{kind=http_route}` for the `health` handler;
  the route-call detector is unchanged and continues to fire only in
  production source. (The test fn itself emits `kind=test` and reaches
  `health` via `CALLS*` from there.)
- Behavior changes to `enrich_reachability` when the keyspace has zero
  `:EntryPoint` nodes. The existing degraded path (returns `ran: false`
  with the "run `cfdb extract --features hir`" warning,
  `enrich/reachability.rs:80-91`) is preserved verbatim.
- SchemaVersion bump. The `kind` field on `:EntryPoint` is documented
  in `nodes.rs:296` as an enum but the schema vocabulary treats it as
  an open-set string attribute (no exhaustive match in any consumer of
  the schema-describe output — evidence in `cfdb-petgraph/src/graph.rs`,
  no `match ep.kind { … }` exhaustive arm). Adding `"test"` and
  `"bench"` to the documented enum is therefore additive-doc, not a
  wire-contract break.
- Backfill of pre-cfdb-042-test-bench-entry-points keyspaces. Operators re-extract; old keyspaces
  see the new `--production-only` flag as a no-op (no `kind ∈ {test,
  bench}` nodes exist, so the production-only filter excludes nothing).

## 3. Design

### 3.1 Extractor changes

In `crates/cfdb-hir-extractor/src/entry_point_emitter.rs::scan_file`,
the `SyntaxKind::FN` dispatch branch (currently `entry_point_emitter.rs:
173-188`) is extended with two new attribute probes and a file-location
fallback. The branch's current shape:

```rust
SyntaxKind::FN => {
    if let Some(fn_ast) = ast::Fn::cast(descendant) {
        if has_tool_attr(&fn_ast) {
            // emit kind="mcp_tool" + REGISTERS_PARAM edges
        }
    }
}
```

The new shape (pseudo-code; implementer follows the existing precedence
discipline):

```rust
SyntaxKind::FN => {
    if let Some(fn_ast) = ast::Fn::cast(descendant) {
        if has_tool_attr(&fn_ast) {
            // mcp_tool branch unchanged; precedence preserved
        } else if has_test_attr(&fn_ast) {
            // kind="test", attribute-based — no REGISTERS_PARAM
        } else if has_bench_attr(&fn_ast) {
            // kind="bench", attribute-based — no REGISTERS_PARAM
        } else if is_under_tests_dir(file_path) {
            // kind="test", file-location-based
        } else if is_under_benches_dir(file_path) {
            // kind="bench", file-location-based
        }
    }
}
```

**Probe semantics.** Both probes live in a NEW sibling file
`crates/cfdb-hir-extractor/src/entry_point_emitter/test_bench.rs`
(NOT in `registers_param.rs`). The probes have NO REGISTERS_PARAM counterpart (test/bench entry
points do not emit `REGISTERS_PARAM` edges per §3.1 item 2-3 below),
so placing them in `registers_param.rs` would be a CCP violation — they
change for a different reason (vocabulary evolution) than the existing
param-emission probes (param-edge wiring evolution).

The new file's module doc:

```rust
//! Test and bench attribute classification probes used by
//! `scan_file`'s `SyntaxKind::FN` dispatch (RFC-042). These probes
//! have no REGISTERS_PARAM counterpart — they are pure classification
//! (kept separate from `registers_param.rs` because they change for a
//! different reason: vocabulary evolution vs param-edge wiring
//! evolution).
```

The probes mirror the discipline of `has_tool_attr` at
`registers_param.rs:56-71`: walk `fn_ast.attrs()`, extract the last
`::`-separated segment of each attribute's meta path via
`attr.meta().and_then(|m| m.path()).syntax().to_string()` and
`rsplit("::")`, compare against a fixed token set.

- `has_test_attr(&fn_ast) -> bool` returns `true` when any attribute's
  last path segment is `test`, `given`, `when`, or `then`. This covers:
  - `#[test]` → segment `test`
  - `#[tokio::test]` / `#[async_std::test]` / `#[actix_rt::test]` →
    segment `test`
  - `#[given]` / `#[when]` / `#[then]` (cucumber-rs step-definition
    attributes) → segments `given`/`when`/`then`
- `has_bench_attr(&fn_ast) -> bool` returns `true` when any attribute's
  last path segment is `bench`. This covers `#[bench]` (libtest /
  criterion-as-attribute).

The probes match the textual-attribute discipline cfdb-037-schema-producer-alignment#3.1 and
cfdb-029-code-facts-database §A1.1 establish: no trait resolution, no macro expansion, no
crate-presence check. A fn under `#[cfg(any())] #[test]` is detected;
this is consistent with the existing `has_tool_attr` policy (the
detection is structural — what the source says — not "what code would
ultimately run").

**`#[cfg(test)]` safely does not fire.** The probe reads `attr.meta().path()`, which for
`#[cfg(test)]` yields path segment `cfg` (not `test`). The `test`
inside `cfg(...)` is a token-tree argument to `cfg`, not a path
segment, so the textual probe correctly does not fire on `#[cfg(test)]`
fns. A fn carrying BOTH `#[cfg(test)] #[test]` triggers the probe on
the `test` attribute and is classified `kind=test` — the intended
behavior, since the test runner can invoke it.

**File-location detection.** When neither attribute fires, the
extractor checks `file_path` (the `&Path` already threaded into
`scan_file`):

- If the workspace-relative path's first non-`/` component is `tests`
  AND the path's parent crate root is identifiable (i.e. there exists
  a `Cargo.toml` ancestor whose `tests/` subdirectory equals the file's
  ancestor), emit `kind=test` for every `FN` in the file.
- Same for `benches/` → `kind=bench`.

The Cargo convention is the contract: under any crate's `Cargo.toml`,
`tests/*.rs` are integration-test binaries and `benches/*.rs` are bench
binaries, by Cargo target auto-discovery. The HIR-pass already has
crate-relative file paths via the existing `file_path: &Path` parameter
threaded into `scan_file`; the implementer derives the
`tests/`-or-`benches/` predicate from that path via a string-prefix
check on the workspace-relative form (same normalization
`:Item.file` uses, see cfdb-041-literal-extraction#3.1 for the precedent).

**Mutual exclusion / precedence (load-bearing for the no-duplicate
invariant, §4):**

1. The existing precedence — `STRUCT/ENUM` with `#[derive(Parser|
   Subcommand)]` emits `cli_command`; `FN` with `#[tool]` emits
   `mcp_tool` — is preserved.
2. Attribute-based test/bench detection wins over file-location-based.
   A `#[test]` fn inside `tests/integration.rs` emits exactly **one**
   `:EntryPoint{kind=test}` (attribute-driven), not two.
3. `#[tool]` precedence over `has_test_attr` is preserved: an MCP
   `#[tool]` fn placed inside `tests/` for whatever reason stays
   classified as `mcp_tool` (matches the spirit of the existing
   `#[tool]` branch — the fn is an MCP entry point first, a test
   second). This is the exact same precedence discipline the existing
   `scan_file` body uses: each `if let` branch is mutually exclusive at
   the dispatch level.
4. `#[bench]` and `has_test_attr` are mutually exclusive in source —
   the rustc lints reject a fn with both — so the order of probing
   does not matter for valid code; the implementer probes `test`
   before `bench` for stable ordering.

**Recall non-goal.** Per CLAUDE.md §5 ("an RFC that adds a new fact type
MUST extend the recall corpus") — the `:EntryPoint{kind=test|bench}`
addition is NOT a new fact type, it extends an existing one. The
recall gate (rustdoc-json ground truth) does not enumerate
`:EntryPoint` membership today (entry points are heuristic, not a
rustdoc fact); this RFC does not change that. The §3.4 synthetic-
workspace fixture is the correctness gate, exactly mirroring cfdb-041-literal-extraction
§3.3 / cfdb-040-const-table-overlap `:ConstTable` precedent.

### 3.2 Schema documentation

Three descriptor edits land with this RFC, all in
`crates/cfdb-core/src/schema/describe/nodes.rs`.

**Edit 1 — `:EntryPoint.kind` descriptor (line 296).** Current text:
"Entry-point kind: `mcp_tool`, `cli_command`, `http_route`, or
`cron_job`. v0.2.0 MVP detects ...". Extended to:

```
"Entry-point kind: `mcp_tool`, `cli_command`, `http_route`, `cron_job`,
 `websocket`, `test`, `bench`. v0.2.0 MVP detects `cli_command` (clap
 `#[derive(Parser/Subcommand)]`) and `mcp_tool` (`#[tool]`); HTTP / cron
 / websocket kinds added later via call-site detection. `test` / `bench`
 (RFC-042) detect `#[test]`, `#[tokio::test]`, `#[given]`/`#[when]`/
 `#[then]` (cucumber BDD), `#[bench]` attributes plus FNs in `tests/` /
 `benches/` directories. BDD step attributes classify as `test`.

 Note: `kind=\"test\"` on `:EntryPoint` is ORTHOGONAL to `:Item.is_test`.
 The former classifies the entry surface (this fn is an invocation root
 for the test runner). The latter classifies the item's compile scope
 (this item lives under `#[cfg(test)]`). A query that needs items
 reachable only from test entry points should match on
 `:EntryPoint{kind:\"test\"}`-reachability via the
 `:Item.reachable_from_production_entry` attribute, NOT on
 `:Item.is_test=true`."
```

**Edit 2 — `:Item.reachable_from_production_entry` attribute (new).**
Add to the `:Item` node descriptor (alongside the existing
`reachable_from_entry` row):

```
attr("reachable_from_production_entry",
     "true iff item is reachable via `CALLS*` from at least one
      `:EntryPoint` whose kind ∉ {test, bench}. Written by the
      `EnrichReachability` pass's production-only BFS. Used by
      `classifier-unwired-production.cypher` (consumed via
      `cfdb scope --production-only`).",
     Provenance::EnrichReachability,
     AttrKind::Bool)
```

**Edit 3 — `:Item.reachable_production_entry_count` attribute (new).**
Add to the `:Item` node descriptor (alongside the existing
`reachable_entry_count` row):

```
attr("reachable_production_entry_count",
     "Count of distinct production `:EntryPoint` nodes (kind ∉ {test, bench})
      that reach the item via `CALLS*`. Sibling to `reachable_entry_count`.
      Written by the `EnrichReachability` pass's production-only BFS.",
     Provenance::EnrichReachability,
     AttrKind::I64)
```

All three edits are documentation only — no schema-version constant
changes (see §2 Does not ship: open-enum). The two new `:Item`
attributes are additive (consumers that do not read them continue to
work unchanged); the homonym-disambiguation sentence is descriptive.

### 3.3 Scope CLI changes

`crates/cfdb-cli/src/scope.rs::scope` accepts a new flag:

```rust
pub fn scope(
    db: &Path,
    context: &str,
    workspace: Option<&Path>,
    format: &str,
    output: Option<&Path>,
    keyspace: Option<&str>,
    explain: bool,
    production_only: bool,  // NEW — default false
) -> Result<(), crate::CfdbCliError>;
```

The flag plumbs through `build_scope_inventory` → `populate_findings_
by_class` → `run_classifier_rule` to the embedded `classifier-unwired.
cypher`. Semantics:

| `--production-only` | "Unwired" definition                                                        |
|---------------------|-----------------------------------------------------------------------------|
| `false` (default)   | Item is NOT reached via `CALLS*` from any `:EntryPoint{kind ∈ ANY}`.        |
| `true`              | Item is NOT reached via `CALLS*` from any `:EntryPoint{kind ∈ {cli_command, mcp_tool, http_route, cron_job, websocket}}`. |

The production kind set is the v0.2 set MINUS `{test, bench}` — i.e.
the kinds that represent runtime-exposed surfaces. The default (false)
matches the operational intuition the issue body argues for: "if a
human OR a test exercises this code, it is not unwired."

**Implementation choice — option (A) over option (B).**

The RFC ratifies **option (A): `enrich_reachability` runs the BFS
twice and emits two parallel attributes** — `reachable_from_entry`
(unchanged, all-kinds seed set) and `reachable_from_production_entry`
(new, kind-filtered seed set). The `--production-only` flag selects
which attribute the classifier reads; the choice is a compile-time
template substitution in the embedded `classifier-unwired.cypher`, not
a runtime branch.

Justification (one paragraph as the brief requires): option (B) — post-
filter at the classifier via a separate `MATCH (e:EntryPoint)-[:EXPOSES]
->(:Item) WHERE e.kind IN [...]` query — moves the kind-filter into
the query layer but pays the cost on every `cfdb scope` invocation,
re-traversing the entry-points×items product set per call. Option (A)
pays the cost once at enrich time (one extra BFS, same algorithm, same
seeds-minus-test-bench) and writes a derived attribute; subsequent
`cfdb scope --production-only` reads are O(1)-per-item. More importantly,
the data shape is symmetric — both the default and the `--production-
only` view consult an `:Item` attribute via the existing `reachable_
from_entry` precedent — which keeps the classifier rule grammar uniform
(one attribute name template, two binding values) instead of branching
the query AST at the CLI layer. The cost of option (A) is one
additional `i64` and one `bool` per `:Item` in every keyspace, ~16
bytes × N items; at the qbot-core scale (~85k items) this is ~1.4 MB
per keyspace, an acceptable trade for the symmetry.

A third option — a single multi-source BFS with per-visit kind-mask
that accumulates both attribute sets in one traversal — is also viable
and would halve the per-item visit cost. Option (A) is ratified for
symmetry with the existing single-filter `enrich_reachability::run`
signature (one filter, one pass, one report); the third option can be
introduced as a perf optimization later without changing the public
attribute schema.

**Trait surface impact.**

cfdb-042-test-bench-entry-points does **NOT** change the `EnrichBackend` trait in
`cfdb-core/src/enrich.rs:177`. The trait method signature
`enrich_reachability(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>`
is preserved verbatim. No downstream `impl EnrichBackend` is affected.
No `cfdb-core` API change is required.

The dual-BFS orchestration lives **inside** the `PetgraphStore`
implementor at `crates/cfdb-petgraph/src/enrich_backend.rs:151-163`.
That impl method internally calls the module-private helper
`crate::enrich::reachability::run` twice with a `cfdb-petgraph`-private
filter enum:

```rust
// crates/cfdb-petgraph/src/enrich/reachability.rs (module-private)
pub(crate) enum ReachabilityFilter {
    All,
    ProductionOnly,
}

pub(crate) fn run(
    state: &mut KeyspaceState,
    filter: ReachabilityFilter,
) -> /* ... */;
```

The `ReachabilityFilter::ProductionOnly → exclude {test, bench}`
mapping is an implementation detail of `reachability.rs` and **never
crosses the crate boundary**. No `BTreeSet<&str>`, no string-typed
filter, and no CLI vocabulary leaks into the port. The CLI continues
to make a single `store.enrich_reachability(&ks)` call; the dual-BFS
happens behind that one call.

The returned `EnrichReport.attrs_written` reports the **sum** of both
passes' writes (2 passes × 2 attrs × N items = 4N writes vs. 2N
previously). Implementers MUST sum the per-pass counters; returning
only one pass's count is a bug. The classifier rule
`classifier-unwired.cypher` is duplicated to
`classifier-unwired-production.cypher`, parameterised over the same
`$context`, reading the production attribute; the orchestrator picks
between them based on the `--production-only` flag.

**Implementer note — sibling .cypher hygiene.** Both
`classifier-unwired.cypher` and `classifier-unwired-production.cypher`
MUST carry a one-line header comment naming the sibling and the single
point of divergence (the `reachable_from_*` attribute name). Future
edits to the WHERE clause MUST be applied to both files.

### 3.4 Test fixtures

A synthetic-workspace fixture under `crates/cfdb-hir-extractor/tests/
fixtures/entry_points/test_bench/` (sibling to existing
`entry_points/` fixtures) covers every detection path:

| File                                | FN                            | Expected kind     | Detection path      |
|-------------------------------------|-------------------------------|-------------------|---------------------|
| `tests/integration.rs`              | `fn test_plain() {}`          | `test`            | `#[test]` attr      |
| `tests/integration.rs`              | `fn test_tokio() {}`          | `test`            | `#[tokio::test]`    |
| `tests/integration.rs`              | `fn test_helper() {}`         | `test`            | under `tests/`      |
| `tests/bdd.rs`                      | `fn step_given() {}`          | `test`            | `#[given]`          |
| `tests/bdd.rs`                      | `fn step_when() {}`           | `test`            | `#[when]`           |
| `tests/bdd.rs`                      | `fn step_then() {}`           | `test`            | `#[then]`           |
| `benches/bench.rs`                  | `fn bench_one(b: &mut Bencher) {}` | `bench`      | `#[bench]` attr     |
| `benches/bench.rs`                  | `fn bench_helper() {}`        | `bench`           | under `benches/`    |
| `src/lib.rs` (in `#[cfg(test)] mod tests`) | `fn unit_one() {}` (attr `#[test]`) | `test`     | `#[test]` attr      |
| `src/lib.rs` (production fn)        | `pub fn library_fn() {}`      | none (no entry-point emitted) | n/a       |

`EXPECTED.md` asserts: each row's FN emits exactly one `:EntryPoint`
with the expected `kind` and an `EXPOSES` edge to the item-qname of the
fn. The production fn emits no entry-point. The `#[cfg(test)]` inner
module case is the load-bearing assertion that attribute-based
detection works for `src/lib.rs::tests::*` (which is NOT under
`tests/`); without this, the unit-test reachability surface is missed
for any crate using the conventional inline-test idiom.

The fixture also includes a `tests/integration.rs::fn helper_with_tool(
)` — annotated `#[tool]` for whatever absurd reason — asserting it
emits `kind=mcp_tool` (precedence rule §3.1 item 3).

## 4. Invariants

- **Determinism (G1).** `cfdb extract` on the §3.4 fixture is sha256-
  byte-stable across two sequential runs. The new emission paths inherit
  the existing `entry_point_emitter.rs` sort discipline (`nodes.sort_
  by(|a, b| a.id.cmp(&b.id))` at `entry_point_emitter.rs:105` plus the
  edge sort at line 106-112). No HashMap iteration order is introduced;
  attribute walks are source-ordered.

- **Recall (no-corpus-extension exemption).** Per CLAUDE.md §5, RFCs
  that add a new fact type extend the recall corpus. This RFC does
  NOT add a new fact type — it extends an existing one (`:EntryPoint`).
  The recall corpus (rustdoc-json ground truth) has no notion of
  "this fn is an entry point" — entry-point classification is
  heuristic, not rustdoc-extractable. The §3.4 synthetic-workspace
  fixture with `EXPECTED.md` exact-match assertions IS the correctness
  gate, mirroring the cfdb-040-const-table-overlap `:ConstTable` and cfdb-041-literal-extraction `:Literal`
  precedents (both rustdoc-invisible, both gated on synthetic fixture).

- **No duplicate emission.** The §3.1 precedence rules guarantee
  exactly one `:EntryPoint` per fn even when both an attribute AND a
  file-location apply (e.g. `#[test]` inside `tests/integration.rs`).
  The fixture covers this case; the assertion is "exactly one EXPOSES
  edge with `kind=test` from the test fn's `:EntryPoint`."

- **No-ratchet (CLAUDE.md §3 / §6.8).** The downstream consumers
  (`/sweep-epic`, `/operate-module`) MUST NOT carry a baseline file
  recording "expected count of unwired-test-false-positives". Per cfdb
  policy, no allowlist or threshold for new false positives is
  introduced — the fix is structural (entry-point coverage), not
  metric-shifted.

- **Keyspace backward-compat.** Pre-cfdb-042-test-bench-entry-points keyspaces have no `:Entry
  Point{kind ∈ {test, bench}}` nodes; `cfdb scope --production-only`
  on such a keyspace is a no-op (the production-only kind filter
  excludes no nodes that weren't already absent), and the default
  `cfdb scope` produces the same `unwired` count as before the RFC
  (the BFS already seeds from all-kinds; the kinds present in the old
  keyspace are exactly the production kinds). The flag is therefore
  forward-compatible: operators benefit from re-extracting, but old
  data continues to work.

- **SchemaVersion stability.** No bump. The schema-doc enum text
  changes (§3.2) are descriptor-only; no `cfdb_core::SchemaVersion`
  constant changes; the lockstep `yg/graph-specs-rust` `.cfdb/cross-
  fixture.toml` bump (cfdb-033-cross-dogfood#4) is NOT required for this RFC.

## 5. Architect lenses

### 5.1 Clean architecture (`clean-arch`)

**clean-arch verdict: RATIFY.** `mod enrich` at
`crates/cfdb-petgraph/src/lib.rs:17` has
no `pub` modifier; the entire enrich module is crate-private;
`ReachabilityFilter` cannot be named by `cfdb-cli` or `cfdb-core`. Port
boundary is structurally enforced, not by convention. Composition root
unambiguously `PetgraphStore::enrich_reachability` (at
`crates/cfdb-petgraph/src/enrich_backend.rs:151-163`).

### 5.2 Domain-driven design (`ddd-specialist`)

**ddd-specialist verdict: RATIFY.** The §3.2 Edit 1
disambiguation sentence correctly names both concepts and their axes
("entry surface" vs "compile scope") and redirects future query
authors to `:Item.reachable_from_production_entry` rather than
`is_test=true`. The two new attribute descriptors
(`reachable_from_production_entry: bool`,
`reachable_production_entry_count: i64`) use `Provenance::EnrichReachability`
correctly (the existing variant at `descriptors.rs:60` already covers
`enrich_reachability()`-produced attributes; the two new attrs are
produced by the same pass's second invocation with
`ReachabilityFilter::ProductionOnly`). Vocabulary unification of
runtime-exposed and build-time-invoked entry-point kinds on the `kind`
axis accepted. Tests tightenings carried verbatim in §7. The synthesis
D4 zero-violation override accepted with one residual implementation
note (companion PR must not silently ship as zero-violation if initial
extract is non-zero).

### 5.3 SOLID + component principles (`solid-architect`)

**solid-architect verdict: RATIFY.** §3.1 names
`entry_point_emitter/test_bench.rs` as the probe destination with
module-doc CCP rationale (vocabulary evolution vs param-edge wiring
evolution); no REGISTERS_PARAM counterpart. §3.3 "Trait surface impact"
subsection preserves `EnrichBackend::enrich_reachability` signature
verbatim; `ReachabilityFilter` is `pub(crate)`. Sibling .cypher
header-comment requirement is binding ("MUST" language). EnrichReport
sum requirement is binding. Two non-blocking
implementation notes for 042-B: verify `ReachabilityFilter` is not in
any pub re-export; co-locate unit tests in
`test_bench.rs #[cfg(test)] mod tests`.

### 5.4 Rust systems (`rust-systems`)

**rust-systems verdict: RATIFY.** §3.3
"Trait surface impact" subsection — signature preserved verbatim,
`ReachabilityFilter { All, ProductionOnly }` as `pub(crate)` enum
inside `cfdb-petgraph/src/enrich/reachability.rs`, `PetgraphStore::
enrich_reachability` calls `run` twice internally, `EnrichReport.
attrs_written` sums both passes. §3.1 explains
`#[cfg(test)]` yields path segment `cfg` not `test`, plus dual-attribute
case. §2 Ships explicit feature-flag scope bullet. §3.3
acknowledges third option with deferral rationale.
One implementation-time note for
042-B implementer (PropValue::Str extraction shape).

## 6. Non-goals

- **`criterion_group!` / `criterion_main!` macro-call detection.** A
  criterion bench typically declares `criterion_group!(benches,
  bench_one, bench_two);` — a macro invocation that expands to a
  runner registering the named fns. The HIR-pass does not expand
  macros and sees only the `criterion_group!` token tree, not the
  individual fn names registered with it. Detection-by-`#[bench]`-
  attribute and file-location-under-`benches/` covers the common
  cases (criterion fns are typically in `benches/*.rs` files even
  when they don't carry `#[bench]` directly); macro-tracking is a
  separate future RFC.

- **HTTP-route detection for test handlers.** A `#[tokio::test] fn
  test_health()` that internally does `Router::new().route("/health",
  health_handler)` does NOT trigger the `http_route` extractor to
  emit a new `:EntryPoint{kind=http_route, name="/health"}`. The
  route-call detector continues to fire in all source uniformly (test
  or production), but in practice route registrations inside a test
  fn are typically setup-only and the test fn itself already covers
  the reachability via `kind=test`. Operators wanting per-route test
  coverage assert it via a separate query, not as a duplicate
  entry-point.

- **Cron-job detection inside tests.** Same shape as above: a test
  that schedules `Job::new_async(...)` for assertion purposes is
  reachable as `kind=test` from its own attribute; the cron-job
  emitter remains source-uniform and may emit a `:EntryPoint{kind=
  cron_job}` if the test source matches the existing call pattern.
  This is documented but not a behavior change.

- **SchemaVersion bump.** Per §2 and §4: the `kind` enum is open-set
  at the wire level; downstream consumers do not exhaustive-match.
  Adding variants is non-breaking.

- **Changes to `enrich_reachability` zero-entry-points degraded
  path.** `enrich/reachability.rs:80-91` (returns `ran: false` with
  the warning "no :EntryPoint nodes in keyspace") is preserved. A
  keyspace that has only test entry points (no production) hits the
  `--production-only` view as "every item unwired in production" —
  which is true and informative for a test-only crate, not a bug.

- **Backfill of old keyspaces.** Operators run `cfdb extract
  --features hir` again. There is no migration tool; the operator
  cost is one re-extract per keyspace.

- **A `:Test` or `:Bench` separate node label.** Considered and
  rejected: tests and benches are entry points (callers into the
  graph), not a distinct kind of fact. The `kind` discriminator on
  `:EntryPoint` is the right axis; a separate label would force the
  classifier to MATCH-UNION two labels and would split the
  reachability BFS unnecessarily.

## 7. Issue decomposition

Filed only after ratification (§2.4). Vertical slices, one issue each,
each carrying the verbatim §2.5 `Tests:` 4-row block.

- **Slice 042-A — extractor `:EntryPoint{kind=test|bench}` emission +
  fixture (cfdb-hir-extractor).** New probes `has_test_attr` /
  `has_bench_attr` in NEW file
  `entry_point_emitter/test_bench.rs` (NOT `registers_param.rs`). FN-branch dispatch
  extension in `scan_file`; file-location detection (`tests/` →
  kind=test, `benches/` → kind=bench); precedence rules per §3.1. New
  synthetic fixture under `crates/cfdb-hir-extractor/tests/fixtures/
  entry_points/test_bench/` per §3.4 with `EXPECTED.md`. Plus the §3.2
  descriptor edits to `crates/cfdb-core/src/schema/describe/nodes.rs`
  (the `:EntryPoint.kind` row extension + homonym disambiguation).
  `Tests:`
  - Unit: pure `has_test_attr(&ast::Fn) -> bool` /
    `has_bench_attr(&ast::Fn) -> bool` assertions on synthetic
    `ast::Fn` inputs constructed via
    `ra_ap_syntax::SourceFile::parse(src, Edition::Edition2021)`. Ten
    cases: `#[test]`, `#[tokio::test]`, `#[async_std::test]`,
    `#[given]`, `#[when]`, `#[then]` → `has_test_attr=true`; `#[bench]`
    → `has_bench_attr=true`; `#[tool]` → both false (precedence
    non-interference); `#[cfg(test)]` → both false (path segment is
    `cfg`, not `test`); bare fn no-attr → both false. File:
    `crates/cfdb-hir-extractor/src/entry_point_emitter/test_bench.rs`
    `#[cfg(test)] mod tests`. Plus the §3.4 synthetic-workspace
    fixture asserting `(kind, EXPOSES.target.qname)` per row.
  - Self dogfood (cfdb on cfdb):
    ```bash
    GREP_COUNT=$(rg -c '#\[test\]|#\[tokio::test\]|#\[given\]|#\[when\]|#\[then\]' \
      --include='*.rs' crates/ | awk -F: '{s+=$2} END {print s}')
    cfdb extract --workspace . --features hir --db .cfdb/db --keyspace cfdb-self
    QUERY_COUNT=$(cfdb query 'MATCH (e:EntryPoint{kind:"test"}) RETURN count(e)' \
      --db .cfdb/db --keyspace cfdb-self)
    [ "$QUERY_COUNT" -ge "$GREP_COUNT" ]
    ```
    Assertion: emitted count ≥ grep count (file-location detection may
    emit MORE, never less).
  - Cross dogfood (graph-specs-rust @ pinned SHA `913f06f`): run
    `ci/cross-dogfood.sh`. Zero new rows on the four existing
    `.cfdb/queries/*.cypher` rules
    (`arch-ban-unwrap-domain-ports`,
    `arch-context-no-application-in-domain`,
    `arch-context-no-cross-layer-unwrap`,
    `arch-context-no-syn-in-domain`). All four match on
    `cs.callee_path` / `caller.crate` patterns — none reads
    `:EntryPoint.kind` or `reachable_from_*` attrs. The cross-dogfood is a no-op regression per RFC §4
    SchemaVersion-stability invariant.
  - Target dogfood (qbot-core @ pinned SHA): report in PR body —
    `MATCH (e:EntryPoint) WHERE e.kind IN ["test","bench"] RETURN
    e.kind, count(e)` total; first 10 emitted `kind=test` qnames as
    sample; spot-audit confirmation that `JupiterCryptoBroker::new`
    (RFC §1 canonical example) is now reached by at least one
    `:EntryPoint{kind:"test"}`.

- **Slice 042-B — scope `--production-only` flag + `enrich_
  reachability` dual-BFS + classifier rule (cfdb-cli + cfdb-petgraph +
  embedded query).** Option (A) from §3.3 with trait-surface
  resolution: `PetgraphStore::enrich_
  reachability` (in `cfdb-petgraph/src/enrich_backend.rs:151-163`)
  internally calls `crate::enrich::reachability::run` twice with a
  module-private `ReachabilityFilter::{All, ProductionOnly}` enum;
  writes both `(reachable_from_entry, reachable_entry_count)` and
  `(reachable_from_production_entry, reachable_production_entry_
  count)` attribute pairs. `EnrichBackend` trait surface unchanged
  (`cfdb-core/src/enrich.rs:177` untouched). New `classifier-unwired-
  production.cypher` with sibling-naming header comment. `cfdb scope`
  accepts `--production-only`. Plus the §3.2 descriptor edits to the
  `:Item` node for the two new reachability attributes.
  `Tests:`
  - Unit: test `reachability::run` with a synthetic `KeyspaceState`
    containing two `:EntryPoint` nodes (one `kind=mcp_tool`, one
    `kind=test`), each EXPOSES-targeting a distinct `:Item`. With
    `ReachabilityFilter::All`, both items have
    `reachable_from_entry=true`. With `ReachabilityFilter::ProductionOnly`,
    only the mcp_tool-exposed item has
    `reachable_from_production_entry=true`. Assert determinism: two
    sequential runs produce byte-identical attribute writes.
  - Self dogfood (cfdb on cfdb):
    ```bash
    cfdb scope --context cfdb-extract --db .cfdb/db --keyspace cfdb-self --format json > default.json
    cfdb scope --context cfdb-extract --db .cfdb/db --keyspace cfdb-self --production-only --format json > prod.json
    default_unwired=$(jq '.findings_by_class.unwired | length' default.json)
    prod_unwired=$(jq '.findings_by_class.unwired | length' prod.json)
    [ "$prod_unwired" -gt "$default_unwired" ]
    ```
    Plus: assert at least one `:Item` in cfdb-self keyspace has
    `reachable_from_production_entry` populated (proves the dual-BFS
    actually ran).
  - Cross dogfood (graph-specs-rust @ pinned SHA `913f06f`): zero
    exit code; zero row change on existing four queries
    (`--production-only` is opt-in, no existing query reads the new
    attribute). Default-mode `unwired` count on graph-specs-rust
    unchanged from pre-cfdb-042-test-bench-entry-points baseline (determinism of all-kinds
    BFS).
  - Target dogfood (qbot-core @ pinned SHA): report in PR body —
    `cfdb scope --context trading` `unwired` count (default mode,
    expected ≥30% drop from 2057); same with `--production-only`
    (expected near 2057); diff table; spot-audit of ≥5 items
    reclassified from "unwired (default)" to "reached-from-test"
    (operator confidence that reclassification is genuine, not
    file-location-helper false-positive).

- **Slice 042-C — empirical close-out on issue #378.** Re-extract
  qbot-core after 042-A + 042-B land; capture `cfdb scope --context
  trading` numbers in default and `--production-only` modes; attach
  the JSON inventory diff and the JupiterCryptoBroker spot-audit (now
  classified as reached-from-test, not unwired) to issue #378 as the
  empirical close. Comment the diff on the issue; close.
  `Tests: none — rationale: cross-repo empirical report, not code.`

- **Companion follow-up (after 042-A + 042-B land on cfdb develop).**
  File a PR against `yg/graph-specs-rust` that adds
  `.cfdb/queries/arch-test-only-reachable-production-items.cypher` —
  the D4 deliverable (a single Cypher rule
  consuming the new `reachable_from_production_entry` attribute,
  expressing the cross-lens consensus on test-only-reachable
  production code as a layer-purity / vocabulary / ISP / vtable-cost
  smell). Intent: zero-violation policy from day one on
  graph-specs-rust. Re-extract graph-specs-rust against the new cfdb
  HEAD and confirm zero rows (OR catalog the small initial finding
  set with operator-confirmed disposition per finding). Wire into
  `ci/cross-dogfood.sh` so future cfdb PRs continue to pass on the
  companion.
  `Tests: the PR IS the
  council's real-code dogfood; the rule's zero-finding outcome on
  graph-specs-rust IS the test.`

## 8. Refs

- `yg/cfdb#378` (originating issue — premature impl, gated by this RFC)
- cfdb-029-code-facts-database (v0.2 :EntryPoint vocabulary, the under-counting prediction)
- cfdb-032-v02-extractor (v0.2 extractor, the `scan_file` dispatch shape this extends)
- cfdb-037-schema-producer-alignment (schema-producer alignment; the textual-attribute heuristic
  contract)
- `crates/cfdb-hir-extractor/src/entry_point_emitter.rs` (touch site)
- `crates/cfdb-hir-extractor/src/entry_point_emitter/registers_param.rs`
  (probe-helper home)
- `crates/cfdb-core/src/schema/describe/nodes.rs:292-296` (descriptor)
- `crates/cfdb-petgraph/src/enrich/reachability.rs` (BFS extension)
- `crates/cfdb-cli/src/scope.rs` / `scope/classifier.rs` (flag plumb)
