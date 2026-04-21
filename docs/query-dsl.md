# cfdb query DSL — user guide

This is the user-facing guide to **named predicates** — the machine-checkable Non-negotiable queries shipped under `.cfdb/predicates/`. If you are writing a new predicate, invoking `cfdb check-predicate` from a CI step, or building an external skill that consumes predicate output, start here.

- **Spec authority:** [`docs/RFC-034-query-dsl.md`](./RFC-034-query-dsl.md) — ratified 2026-04-21 by `council-49-query-dsl` (4 lenses: clean-arch, ddd-specialist, solid-architect, rust-systems).
- **Council verdicts + load-bearing decisions:** [`council/49/RATIFIED.md`](../council/49/RATIFIED.md).
- **Implementation slices:** #145 param-resolver (PR #152) · #146 seed library + static check (PR #153) · #147 verb handler (PR #154) · #148 dogfood + CI (PR #155) · #149 this guide (current).

## 1. What shipped, what didn't

**What IS in the DSL:**

- A **shared Cypher-subset parser** at `cfdb-query` (the same one that powers `cfdb query`, `cfdb violations`, `cfdb list-callers`).
- A **named-predicate library** at `.cfdb/predicates/<name>.cypher` — one Cypher file per predicate, with a mandatory two-line first-line comment documenting params and the three-column return shape.
- A **`--param <name>:<form>:<value>` CLI-arg resolver** (internal to `cfdb-cli`) that maps CLI strings to `cfdb_core::query::Param` values via four forms.
- A **`cfdb check-predicate` verb** that loads a predicate, resolves its params, executes against a pinned keyspace, and emits the canonical three-column `(qname, line, reason)` violation format.
- **CI-level dogfood + determinism gates** — `ci/predicate-determinism.sh` + an integration test that iterates every shipped seed against cfdb's own keyspace.

**What is NOT in the DSL** (explicit non-goals, per RFC-034 §6):

- No new grammar beyond the Cypher subset `cfdb-query` already parses.
- No new crate (`cfdb-query-dsl` was rejected by the council; the resolver lives in `cfdb-cli` per CRP).
- No UDF registration mechanism. The `eval_call` dispatch table at `crates/cfdb-petgraph/src/eval/predicate.rs` is the documented extension point if a future RFC needs non-Cypher predicate forms.
- No template composition / `INCLUDE` / `MACRO` / `IMPORT` — one file = one predicate.
- **No `.cfdb/queries/` overlap.** `.cfdb/queries/*.cypher` are self-hosted ban rules run by `cfdb violations`. `.cfdb/predicates/*.cypher` are cross-repo consistency predicates run by `cfdb check-predicate`. Different consumer, different verb, different change vector.
- **No `RE_EXPORTS` edge in the schema.** The original issue body's example shape — "re-exported FROM crate IN context-map[portfolio] WITHOUT TranslationAdapter impl" — is not expressible today. It requires HIR Phase B re-export resolution (see [RFC-032 §8.2](./RFC-032-v02-extractor.md)) + a follow-up RFC that adds the edge. Slice #146 ships a reframed seed (`context-homonym-crate-in-multiple-contexts`) that exercises the same param-resolver + composition pathway without depending on deferred edges.
- **No metric ratchets.** No `--baseline`, no `--allowlist`, no `.predicates-baseline.toml`. Every predicate is zero-tolerance against a hard threshold defined in the `.cypher` body; the only way to "raise a threshold" is to edit the predicate file in a reviewed PR.

## 2. Homonym note — `Predicate` vs `predicate`

Two distinct concepts share a lexical root:

| Concept | Location | Layer |
|---|---|---|
| `cfdb_core::query::Predicate` | `crates/cfdb-core/src/query/ast.rs` | In-memory AST enum (`Compare`, `In`, `Regex`, `NotExists`, `And`, `Or`, `Not`, `Ne`) — produced by the parser, consumed by the evaluator |
| `predicate` file | `.cfdb/predicates/*.cypher` | On-disk storage artefact — a Cypher template that the `cfdb check-predicate` verb loads, parses, and executes |

These are **deliberately distinct**, not split-brain. They share a bounded context (`cfdb-query`) but different layers:

- `Predicate` is the AST-level concept — "a WHERE expression that evaluates to `bool`".
- `predicate` is the storage-level concept — "a named Cypher query that returns violation rows".

Per ddd-specialist R1 verdict on RFC-034 (council/49/verdicts/ddd.md), alternative directory names (`.cfdb/rules/`, `.cfdb/checks/`) were considered and rejected: `rules` collides with `.cfdb/queries/*.cypher` ban rules, and `checks` collides with the `cfdb check --trigger` editorial-drift verb (#101). `predicates/` is the least-collision name.

When authoring code or docs that references both, disambiguate by fully-qualifying the AST type (`cfdb_core::query::Predicate`) and writing "predicate file" or "predicate library" for the storage-level concept.

## 3. Canonical examples

Three seed predicates ship in `.cfdb/predicates/`. They exercise the three predicate shapes enumerated in RFC-034 §3.3 + three out of four param-resolver forms.

### 3.1. `path-regex.cypher` — file-path regex fallback

```cypher
// Params: $pat (regex:<pattern>)
// Returns: (qname, line, reason) — canonical three-column violation format.
MATCH (f:File)
WHERE f.path =~ $pat
RETURN f.path AS qname, 0 AS line, 'file path matched regex' AS reason
ORDER BY qname
```

**Invocation:**

```
cfdb check-predicate \
  --db .cfdb/db --keyspace cfdb \
  --workspace-root . \
  --name path-regex \
  --param 'pat:regex:cfdb-query/.*\.rs'
```

This replaces the "shell-grep escape hatch" from the original RFC-034 §49 issue-body constraints — Cypher's `=~` regex on `:File.path` covers it deterministically, inside the gate, with no shell-out.

### 3.2. `context-homonym-crate-in-multiple-contexts.cypher` — context-membership set intersection

```cypher
// Params: $context_a (context:<name>), $context_b (context:<name>)
// Returns: (qname, line, reason) — canonical three-column violation format.
MATCH (c:Crate)
WHERE c.name IN $context_a
  AND c.name IN $context_b
RETURN c.name AS qname, 0 AS line, 'crate is a member of both contexts — candidate DDD homonym' AS reason
ORDER BY qname
```

**Invocation** (crate in both `trading` AND `portfolio` contexts → candidate homonym):

```
cfdb check-predicate \
  --db .cfdb/db --keyspace cfdb \
  --workspace-root . \
  --name context-homonym-crate-in-multiple-contexts \
  --param 'context_a:context:trading' \
  --param 'context_b:context:portfolio'
```

This is the load-bearing shape reframed during the RFC-034 R1 synthesis: the original "re-export without adapter" example required a non-existent `RE_EXPORTS` edge, so the council substituted a context-homonym detector that exercises the same param-resolver + composition pathway using only schema vocabulary that ships today.

### 3.3. `fn-returns-type-in-crate-set.cypher` — fn signature regex + crate-set filter

```cypher
// Params: $type_pattern (regex:<pattern>), $fin_precision_crates (list:<a,b,c>)
// Returns: (qname, line, reason) — canonical three-column violation format.
MATCH (i:Item)
WHERE i.kind = 'fn'
  AND i.visibility = 'pub'
  AND i.signature =~ $type_pattern
  AND i.crate IN $fin_precision_crates
RETURN i.qname AS qname, i.line AS line, 'public fn signature matches type-pattern in precision-crate set' AS reason
ORDER BY qname
```

**Invocation** (find every `pub fn` returning `Decimal` in a pre-declared set of precision-sensitive crates):

```
cfdb check-predicate \
  --db .cfdb/db --keyspace cfdb \
  --workspace-root . \
  --name fn-returns-type-in-crate-set \
  --param 'type_pattern:regex:Decimal' \
  --param 'fin_precision_crates:list:cfdb-core,cfdb-query,cfdb-petgraph'
```

## 4. Param-resolver syntax

CLI-supplied params are strings of shape `<name>:<form>:<value>`. The Slice-1 resolver (`crates/cfdb-cli/src/param_resolver.rs`) dispatches on the `<form>` token and produces a `cfdb_core::query::Param` value bound to `<name>`.

| Form | CLI literal | Result | Use case |
|---|---|---|---|
| **context** | `<param>:context:<concept-name>` | `Param::List` of every crate whose `.cfdb/concepts/<concept-name>.toml` context-set includes it, **sorted ascending** for determinism | Bind a named bounded-context's crate-set as a list, so the predicate can do `WHERE c.name IN $<param>` |
| **regex** | `<param>:regex:<pattern>` | `Param::Scalar(PropValue::Str(pattern))` | Bind a string that the predicate uses in `=~` comparisons — path matches, type-signature matches, qname matches |
| **literal** | `<param>:literal:<value>` | `Param::Scalar(PropValue::Str(value))` | Bind a single string that the predicate uses in `=` / `IN` comparisons against specific values |
| **list** | `<param>:list:<a,b,c>` | `Param::List` of comma-separated strings, **preserving input order** (RFC-034 §3.4 semantic) | Bind a user-supplied set of strings for `WHERE c.name IN $<param>` without going through a concept file |

Three load-bearing invariants of the resolver:

1. **No inline TOML parser.** `.cfdb/concepts/*.toml` reading delegates to `cfdb_concepts::load_concept_overrides` (the canonical loader). RFC-034 §4.6.
2. **Hermeticity.** The resolver takes `(workspace_root, cli_arg)` and performs zero environment reads, zero subprocess spawns, zero network. RFC-034 §4.5.
3. **Distinct sort semantics per form.** `context:` sorts the resolved crate list ascending (for determinism). `list:` preserves user-supplied order (semantic — the user chose the order). `regex:` / `literal:` are scalars.

The resolver is `pub(crate)` in `cfdb-cli` — consumers invoke it indirectly by passing `--param` to `cfdb check-predicate`.

## 5. How to add a predicate

One file per predicate, flat under `.cfdb/predicates/`. No sub-directories (`.cfdb/predicates/trading/` is forbidden per RFC-034 §6). Naming convention: `<scope>-<noun>-<qualifier>.cypher` with hyphenated slugs.

### Step 1 — Author the `.cypher` file

Start with a mandatory two-line first-line comment per RFC-034 §3.5:

```cypher
// Params: $<name> (<form>), $<name2> (<form2>), ...
// Returns: (qname, line, reason) — canonical three-column violation format.
<optional longer-form explanation of the predicate's intent>
MATCH ...
WHERE ...
RETURN <expr> AS qname, <expr> AS line, '<reason text>' AS reason
ORDER BY qname
```

**Grammar constraints** (Slice 2 shipping scope — RFC-034 §6 non-goals):

- No positive `EXISTS { ... }`. Only `NOT EXISTS` is supported by the parser (`crates/cfdb-query/src/parser/predicate.rs:43-48`).
- No `IN` / `AND` / `OR` / `NOT` inside a `NOT EXISTS { ... }` inner WHERE. Inner scope is Compare-only.
- No outer-variable references inside a subquery. The inner MATCH runs in a fresh evaluator scope; see `examples/queries/t1-concept-unwired.cypher` first-line comment for the precedent.
- Only schema vocabulary from `cfdb_core::schema::{Label, EdgeLabel}` constants. New labels/edges require a schema RFC.

### Step 2 — Add a `SeedCase` to the integration test

Edit `crates/cfdb-cli/tests/predicate_library_dogfood.rs` and add a `SeedCase` entry with your predicate's canonical param set + expected lower-bound row count:

```rust
SeedCase {
    name: "your-new-predicate",
    params: your_new_predicate_params,
    min_rows: 0,  // adjust based on expected behaviour against cfdb's own keyspace
},
```

The `seed_cases_cover_every_shipped_predicate` assertion rejects new `.cypher` files without matching `SeedCase` — this catches drift automatically.

### Step 3 — Add a branch to `ci/predicate-determinism.sh`

Add the predicate name to `KNOWN_SEEDS` and a case arm in the run loop with its canonical param set. The script's "unknown seed" check catches drift automatically.

### Step 4 — Verify locally

```
cargo test -p cfdb-cli --test predicate_library_dogfood
cargo test -p cfdb-query --test predicate_schema_refs
CFDB_BIN=./target/release/cfdb ./ci/predicate-determinism.sh .
```

All three MUST pass. `predicate_schema_refs` (from Slice 2) is the schema-reference static check — it catches typo'd or out-of-schema vocabulary.

### Step 5 — Invocation surface

Once merged, consumers invoke your predicate via:

```
cfdb check-predicate \
  --db <path> --keyspace <name> \
  --workspace-root <path> \
  --name your-new-predicate \
  --param '<name>:<form>:<value>' \
  [--format text|json] \
  [--no-fail]
```

The verb exits non-zero iff the predicate returns ≥ 1 row (zero-tolerance per RFC-034 §4.3). `--no-fail` is for CI inventory runs that want to capture row counts without failing the pipeline.

## 6. Output formats

`cfdb check-predicate` emits two shapes, selected by `--format`:

### `--format text` (default)

One line per row on stdout as `qname<TAB>line<TAB>reason`. Summary line on stderr: `check-predicate: <N> (predicate: <name>)`. Matches the rhythm of `cfdb violations`.

### `--format json`

One pretty-printed `PredicateRunReport` on stdout:

```json
{
  "predicate_name": "path-regex",
  "predicate_path": "/abs/path/to/.cfdb/predicates/path-regex.cypher",
  "row_count": 42,
  "rows": [
    {"qname": "...", "line": 0, "reason": "file path matched regex"},
    ...
  ]
}
```

Rows are sorted ascending by `(qname, line)` before serialization (RFC-034 §4.1 determinism). The `row_count` scalar is the authoritative number consumers should parse for exit-code / gate-count decisions; it will never disagree with `rows.length`.

Library consumers (tests, skill adapters) can import `cfdb_cli::{check_predicate, PredicateRunReport, PredicateRow}` directly and inspect `rows` without parsing stdout.

## 7. Downstream consumer contracts

- **`/freshness` Step 2g (pre-council triggers)** — unchanged. Tier-1 triggers continue to consume the `check-prelude-triggers` binary, separate from this DSL.
- **`/discover`, `/prescribe`** — may call `cfdb check-predicate --name X --format json` as a sub-routine to augment concept inventory with "is concept Y context-homonym-free" checks.
- **`check-prelude-consistency` (qbot-core skill, out-of-repo)** — consumes `cfdb check-predicate --name X --format json` as a subprocess. Contract: `PredicateRunReport` JSON shape is frozen (RFC-034 §3.1 + this guide §6).
- **`/ship`** — unchanged.
- **`/gate-contract`** — may run `cfdb check-predicate --name contract-adapters-only-in-adapter-crates` (or similar, if/when such a predicate is filed) to enforce port/adapter separation.

## 8. Deferred capabilities (future RFCs)

Kept here so consumers know what NOT to wait for in the current library:

| Capability | Why deferred | Unblock condition |
|---|---|---|
| Re-export predicate (`RE_EXPORTS` edge traversal) | The `cfdb-extractor` syn-based pipeline does NOT emit re-export edges today; resolution is HIR Phase B (RFC-032 §8.2) | HIR Phase B + new schema RFC that adds the edge + a new predicate seed |
| Positive `EXISTS { ... }` in parser | Current parser only supports `NOT EXISTS` | Separate parser-extension RFC |
| `IN` / `AND` / `OR` / `NOT` inside subquery inner WHERE | Current parser is Compare-only in inner scope | Same parser-extension RFC |
| UDF registration mechanism | `eval_call` dispatch table exists as the extension point, but there's no registration surface yet | A future RFC that names a concrete predicate form Cypher + param resolver cannot express |
| Template composition (`INCLUDE` / `MACRO`) | One-file-per-predicate is the shipped invariant | A future RFC articulating the composition semantic |

If you hit one of these limits, **file an issue referencing RFC-034 §6 non-goals** rather than working around it in a predicate file.

## 9. Cross-references

- **RFC:** [`docs/RFC-034-query-dsl.md`](./RFC-034-query-dsl.md)
- **Council:** [`council/49/RATIFIED.md`](../council/49/RATIFIED.md), [`council/49/SYNTHESIS-R1.md`](../council/49/SYNTHESIS-R1.md), [`council/49/verdicts/`](../council/49/verdicts/)
- **Predicate library:** [`.cfdb/predicates/README.md`](../.cfdb/predicates/README.md)
- **CLI inventory:** `crates/cfdb-cli/src/main.rs` module docstring (authoritative for the 20+ shipped verbs)
- **Schema vocabulary:** `crates/cfdb-core/src/schema/labels.rs` — `Label::<CONST>` + `EdgeLabel::<CONST>`
- **Static schema-ref check:** `crates/cfdb-query/tests/predicate_schema_refs.rs` — the test that forbids typo'd labels in seeds
