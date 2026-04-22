# cfdb Cypher UDFs — reference

**Status:** six built-in functions, hard-wired in the evaluator
(`crates/cfdb-petgraph/src/eval/predicate.rs::eval_call`). There is no
user-facing registry — adding a new UDF is an RFC-gated change.

Related:
- RFC specification: `docs/RFC-cfdb.md` §A1.5
- Implementation: `crates/cfdb-petgraph/src/eval/predicate.rs`
- Parser: `crates/cfdb-query/src/parser/expression.rs` (`Expr::Call`)

Every UDF appears in the Cypher `WHERE` clause as a first-class
expression, composable with property access, literals, and the other
UDFs. Type-mismatch behavior is uniform: any UDF returns `None` when
given arguments of the wrong shape, and the enclosing predicate treats
`None` as "unknown" — the binding is dropped rather than coerced.

## `regexp_extract(text, pattern) -> String`

Returns the first substring of `text` matching the regex `pattern`, or
`None` if no match.

- Inputs: `text: String`, `pattern: String` (Rust `regex` crate syntax).
- Output: `String`.
- Type-mismatch: returns `None` if either arg is not a string or the
  pattern is invalid regex.

```cypher
// Extract the concept prefix from a resolver fn name
WHERE regexp_extract(a.name, '^(\w+)_(?:from|to|for|as)_') =
      regexp_extract(b.name, '^(\w+)_(?:from|to|for|as)_')
```

## `size(text) -> Int`

Returns the character count (`chars().count()`, NOT byte length) of a
string.

- Input: `text: String`.
- Output: `Int` (i64).
- Type-mismatch: returns `None` if arg is not a string.

```cypher
WHERE size(a.qname) > 40
```

## `starts_with(text, prefix) -> Bool`

Returns `true` when `text` begins with `prefix`.

- Inputs: `text: String`, `prefix: String`.
- Output: `Bool`.
- Type-mismatch: returns `None` if either arg is not a string.

```cypher
WHERE starts_with(a.qname, 'qbot_domain::') = true
```

## `ends_with(text, suffix) -> Bool`

Returns `true` when `text` ends with `suffix`. Symmetric to
`starts_with`.

- Inputs: `text: String`, `suffix: String`.
- Output: `Bool`.
- Type-mismatch: returns `None` if either arg is not a string.

```cypher
WHERE ends_with(a.file, '_test.rs') = false
```

## `last_segment(text) -> String`

Returns the substring after the last `:` character. When there is no
`:`, returns the input unchanged. The double-colon of Rust qnames
collapses naturally — `last_segment("foo::bar::baz") = "baz"`.

- Input: `text: String`.
- Output: `String`.
- Type-mismatch: returns `None` if arg is not a string.

```cypher
// Join two :Items on their last qname segment
WHERE last_segment(a.qname) = last_segment(b.qname)
```

## `signature_divergent(sig_a, sig_b) -> Bool`

Returns `true` when two `:Item.signature` strings differ after
whitespace normalization. Introduced in issue #47; load-bearing for the
RFC-029 §A1.5 v0.2-8 gate and the #48 Finding classifier (Context
Homonym discrimination, class 2 in §A2.1).

- Inputs: `sig_a: String`, `sig_b: String` — typically
  `a.signature` and `b.signature` for two fn / method `:Item` nodes.
- Output: `Bool`.
- Type-mismatch: returns `None` if either arg is not a string (this
  includes the case where `:Item.signature` is absent, which occurs on
  non-fn kinds — struct / enum / trait / const / impl_block /
  type_alias / static).

### Normalization contract

Both inputs are normalized before comparison:

1. Outer whitespace is trimmed.
2. Any run of internal whitespace (spaces, tabs, newlines) is
   collapsed to a single ASCII space.

Parameter names are NOT re-normalized at UDF time because the producer
(`cfdb-extractor::type_render::render_fn_signature`) already strips them
at extract time — the signature string carries parameter TYPES only.
Receivers render as `&Self`, `&mut Self`, or `Self`; modifier order is
fixed as `[const ][async ][unsafe ]fn(...) -> ...`.

### Semantics summary

Given two fn / method `:Item` nodes `a` and `b`:

- `signature_divergent(a.signature, b.signature) = false` → the two
  items have the same calling contract. In combination with `last
  qname segment` equality and `a.bounded_context <> b.bounded_context`,
  this is the Shared Kernel signal (RFC §A1.5 v0.2-8 / DDD R1).
- `signature_divergent(a.signature, b.signature) = true` → divergent
  calling contract despite name / bounded-context / concept overlap.
  This is the Context Homonym signal — route to `/operate-module`, NOT
  `/sweep-epic`, per RFC §A2.3 SkillRoutingTable.

### Example — Context Homonym rule

```cypher
MATCH (a:Item), (b:Item)
WHERE a.kind IN ['fn', 'method']
  AND b.kind IN ['fn', 'method']
  AND a.qname < b.qname
  AND a.bounded_context <> b.bounded_context
  AND last_segment(a.qname) = last_segment(b.qname)
  AND signature_divergent(a.signature, b.signature) = true
RETURN a.qname, b.qname
```

The full rule with test filters and evidence columns ships at
`examples/queries/signature-divergent.cypher`.

### Adding a new UDF

Adding a new built-in is an RFC-gated change per `CLAUDE.md` §3. The
change lands as one atomic PR touching:

1. `crates/cfdb-petgraph/src/eval/predicate.rs` — new arm in
   `eval_call` + a `call_<name>` helper mirroring the existing shape.
2. `docs/udfs.md` — this file, with a section following the above
   template (inputs, output, type-mismatch, normalization, example).
3. A ratified RFC (`docs/RFC-<topic>.md`) naming the UDF and the
   motivating rule set.

There is no UDF registry. The number of builtins is small (six as of
issue #47), their surface is stable, and a registry would be premature
abstraction — adding a match arm is idiomatic Rust and keeps the
dispatch path a single cache-friendly jump.
