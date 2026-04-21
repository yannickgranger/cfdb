# `.cfdb/predicates/` — named Cypher predicates

RFC-034 (ratified 2026-04-21 by `council-49-query-dsl`) — cfdb's **Non-negotiable predicate library**. One `.cypher` file per predicate; each is loaded by `cfdb check-predicate --name <basename>` (Slice 3 / #147) and evaluated against a pinned keyspace with CLI-resolved `--param` bindings.

**Not to be confused with `.cfdb/queries/`** — that directory is for `cfdb violations` self-dogfood **ban rules**. Different consumer, different verb, different change vector.

## File shape

Every file is a single Cypher query (one top-level `MATCH ... RETURN ...` with optional `OPTIONAL MATCH`, `WHERE`, `WITH`, `UNWIND`, `ORDER BY`, `LIMIT`) using `$param` placeholders for CLI-resolved values.

**Mandatory first-line comment** (RFC-034 §3.5). Two lines:

```cypher
// Params: $foo (form), $bar (form), ...
// Returns: (qname, line, reason) — canonical three-column violation format.
```

The `Params:` line names every `$name` placeholder + the expected CLI-arg `<form>` (`context` / `regex` / `literal` / `list`) the caller binds it from. The `Returns:` line names the output columns — invariantly `(qname, line, reason)` so `cfdb check-predicate` can emit the same three-column text as `cfdb violations`.

## Grammar constraints (Slice 2 shipping scope)

- **NO positive `EXISTS { ... }`.** The parser only supports `NOT EXISTS`. (`crates/cfdb-query/src/parser/predicate.rs:43-48`.)
- **NO `IN` / `AND` / `OR` / `NOT` inside a `NOT EXISTS { ... }` inner WHERE.** Inner scope is Compare-only. (`crates/cfdb-query/src/parser/predicate.rs:131-139`.)
- **NO outer-variable references inside a subquery.** The inner MATCH runs in a fresh evaluator scope; `NOT EXISTS { MATCH (i)-[:X]->(t) WHERE outer.name = t.name }` evaluates `outer.name` to `None` (documented at `examples/queries/t1-concept-unwired.cypher` first-line comment).
- **ONLY schema vocabulary.** Every `:Label` must exist in `cfdb_core::schema::Label` constants. Every `[:EdgeLabel]` must exist in `cfdb_core::schema::EdgeLabel` constants. The `predicate_schema_refs` integration test in `cfdb-query/tests/` enforces this on every PR.

## How to add a predicate

1. Pick a hyphenated slug: `<scope>-<noun>-<qualifier>.cypher` (e.g. `context-homonym-crate-in-multiple-contexts.cypher`, `fn-returns-type-in-crate-set.cypher`).
2. Author the Cypher body using only the grammar + vocabulary above. Test-parse locally with `cfdb query --db <keyspace> --keyspace <name> "$(cat .cfdb/predicates/<slug>.cypher)" --input <params>.yaml`.
3. Write the mandatory two-line first-line comment.
4. Ensure every `:Label` / `[:EdgeLabel]` is a real schema constant. The CI static-check test will fail the PR otherwise.
5. Bind the predicate to a verb caller: either in Slice 4's dogfood list (#148, for CI-enforced runs against cfdb's own keyspace) or as an on-demand predicate runnable via `cfdb check-predicate --name <slug>`.
6. Every new predicate is zero-tolerance by contract — exit non-zero iff row count > 0. No `--baseline`, no `--ceiling`, no allowlist file.

## Shipped seeds (Slice 2, 2026-04-21)

| File | Purpose | Params | Pattern |
|---|---|---|---|
| `context-homonym-crate-in-multiple-contexts.cypher` | detect a `:Crate` whose `.name` appears in 2+ context-map crate-sets (candidate DDD homonym) | `$context_a`, `$context_b` (both `context:<name>`) | top-level `MATCH (c:Crate) WHERE c.name IN $context_a AND c.name IN $context_b` |
| `fn-returns-type-in-crate-set.cypher` | detect public `fn` items whose rendered signature matches a type-regex AND whose `.crate` is in a set (e.g. `.cfdb/published-language-crates.toml` → `financial-precision` set) | `$type_pattern` (`regex`), `$fin_precision_crates` (`list`) | top-level `MATCH (i:Item) WHERE i.kind='fn' AND i.visibility='pub' AND i.signature =~ $type_pattern AND i.crate IN $fin_precision_crates` |
| `path-regex.cypher` | file-path regex fallback (RFC-034 §6 "Not a shell-grep escape hatch" — Cypher regex covers it) | `$pat` (`regex`) | top-level `MATCH (f:File) WHERE f.path =~ $pat` |

All three use ONLY top-level patterns — no subqueries — so the inner-scope limitations above do not apply.

## Deferred (future RFC)

The original RFC-034 seed #1 `context-member-reexport-without-adapter` required a `RE_EXPORTS` edge that does not exist on develop (re-export tracking is HIR Phase B). Deferred to a future RFC that adds the edge; current seed #1 covers the adjacent "context-homonym" use case without that dependency.
