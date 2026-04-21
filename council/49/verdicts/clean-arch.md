# Verdict — clean-arch

## Read log
- [x] docs/RFC-034-query-dsl.md
- [x] council/49/BRIEF.md
- [x] crates/cfdb-query/src/lib.rs
- [x] crates/cfdb-query/Cargo.toml
- [x] crates/cfdb-query/src/skill_routing.rs
- [x] crates/cfdb-cli/src/check.rs
- [x] crates/cfdb-cli/src/commands.rs
- [x] crates/cfdb-cli/src/main_command.rs
- [x] crates/cfdb-cli/Cargo.toml
- [x] crates/cfdb-concepts/src/lib.rs
- [x] crates/cfdb-concepts/Cargo.toml
- [x] crates/cfdb-petgraph/Cargo.toml (first 30 lines)
- [x] council/RATIFIED.md §A.2, §A.14

---

## Reframe

VERDICT: RATIFY

The RFC's reframe — "no new DSL grammar, no new crate, extend cfdb-query with
param_resolver + extend cfdb-cli with check-predicate" — is correct and
proportionate. The existing Cypher subset already covers every predicate form
enumerated in #49 (RFC §3.3 predicate-form table). A new DSL grammar would
duplicate the chumsky parser at `crates/cfdb-query/src/parser/mod.rs` without
producing a qualitatively different execution surface; a new crate
`cfdb-query-dsl` would create a fourth "query assembly" crate (joining
cfdb-query, cfdb-core, cfdb-query/builder) for a feature that amounts to two
files (~350 LOC). The reframe is load-bearing; the council should confirm it
explicitly. I do.

---

## Per-lens questions

### Q-CA-1: Is `cfdb-cli → cfdb-query → cfdb-concepts` acceptable?

**YES, with one significant correction to the RFC's premise.**

The RFC claims at §5.1: "cfdb-query already consumes `toml` + reads
`.cfdb/skill-routing.toml`". This is true for `toml` (confirmed in
`crates/cfdb-query/Cargo.toml`) but the RFC further implies that `cfdb-concepts`
is already a transitive dep of `cfdb-query`. This is **false**.

Current deps as of this worktree:
- `cfdb-query/Cargo.toml`: `cfdb-core`, `serde`, `serde_json`, `thiserror`,
  `chumsky`, `regex`, `toml` — no `cfdb-concepts`.
- `cfdb-cli/Cargo.toml`: `cfdb-core`, `cfdb-query`, `cfdb-petgraph`,
  `cfdb-extractor` — no direct `cfdb-concepts`.
- `cfdb-concepts/Cargo.toml`: `serde`, `thiserror`, `toml` — no cfdb-core or
  cfdb-query (zero heavy deps by design, documented in its Cargo.toml comment).

So adding `cfdb-concepts` to `cfdb-query`'s `[dependencies]` is a NEW
Cargo.toml edit, not just a code addition. This is a **load-bearing dependency
introduction** the RFC understates.

**Is it architecturally sound?** Yes. The dependency direction is:

```
cfdb-cli → cfdb-query → cfdb-concepts → (serde, thiserror, toml only)
                      → cfdb-core     → (serde, serde_json, thiserror, indexmap)
```

This is strictly layered. `cfdb-concepts` carries zero heavy deps
(no syn, no chumsky, no ra-ap-*). Its comment at `crates/cfdb-concepts/Cargo.toml`
explicitly anticipates `cfdb-query` as a future consumer:

> "Extracted from `cfdb-extractor/src/context.rs` in Issue #3 to break a
>  latent split-brain: two consumers (`cfdb-extractor` + the future
>  `cfdb-query` DSL evaluator) would otherwise each embed their own
>  `ContextMap` implementation."

The crate was purpose-designed for this dependency. No cycle risk: cfdb-concepts
does not depend on cfdb-query, cfdb-cli, or cfdb-core. Direction is clean.

**The alternative — putting the param resolver in `cfdb-cli` directly — would be
worse.** `cfdb-cli/src/commands.rs` already uses `parse_and_execute`
(line 631), `lint_shape`, and `parse` from `cfdb-query`. Putting param
resolution in cfdb-cli would mean the CLI wires params before passing a `Query`
to cfdb-query, making the param → Query assembly seam incoherent: the
`resolve_params` call would live at the CLI layer but produce a value
(`BTreeMap<String, Param>`) that the evaluator's existing `Query.params` bag
already handles. Clean arch prefers the composition to happen as close to the
domain as possible — `cfdb-query` is that layer.

**Required RFC correction:** §5.1 should state "cfdb-concepts is NOT yet a dep
of cfdb-query; Slice 1 adds it to `cfdb-query/Cargo.toml`". The omission
doesn't change the verdict but the implementer must not be surprised.

**Cycle check:** No cycles introduced. cfdb-concepts ← cfdb-query is a
strictly inward arc on the existing `cfdb-core ← cfdb-concepts ← cfdb-query ←
cfdb-cli` cone. cfdb-concepts does not import from cfdb-query or cfdb-cli.
Confirmed by reading all four `Cargo.toml` files above.

### Q-CA-2: Should `cfdb check-predicate` collapse into `cfdb violations --rule`?

**NO. Two verbs are correct. The difference is composition root, not query engine.**

`cfdb violations --rule <path>` (implemented at `crates/cfdb-cli/src/commands.rs:580`)
takes a raw `.cypher` file and executes it verbatim. It has zero param-binding
logic; its internal `run_cypher_rule` helper (line 600) calls `parse_and_execute`
with no param resolver.

`cfdb check-predicate --name <name> [--param ...]` does three things
`violations` cannot:
1. Loads a predicate by name from `.cfdb/predicates/` (named library, not
   arbitrary path).
2. Resolves `--param context:<name>` into a `Param::List` by reading
   `.cfdb/concepts/*.toml` via `cfdb_concepts::load_concept_overrides`.
3. Returns a typed `PredicateRunReport` for JSON consumers, not just a raw
   `QueryResult`.

Collapsing them would require `violations` to grow `--param` + context-resolver
logic, blurring its documented responsibility ("run a rule file and exit 1 if
any violations are found", `main_command.rs:336`). Its current doc comment
explicitly frames it as the *drop-in replacement for handwritten Rust
architecture tests* — a flat, path-based, no-param verb. Adding context-aware
param resolution to `violations` would violate that contract.

The distinction also maps to separate consumers: `violations` is used by the
dogfood CI gate (`cfdb violations --rule .cfdb/queries/*.cypher`), while
`check-predicate` is used by `check-prelude-consistency` (a qbot-core skill)
and would use the `.cfdb/predicates/` library. These are different audiences
with different authoring conventions. Keeping them separate preserves the
Screaming Architecture principle: `violations` screams "ban-rule enforcement";
`check-predicate` screams "parameterized predicate library execution".

The shared plumbing (`parse_and_execute` in `commands.rs:631`) is already
factored out and will naturally be reused by `check_predicate.rs` — there is no
code duplication argument for merging.

---

## Cross-lens binding

### Issue decomposition (§7)

The 5-slice sequencing is sound. Slices 1 and 2 have no file overlap; both must
precede Slice 3. Slice 4 (dogfood + determinism) correctly blocks on Slice 3.
Slice 5 (docs) ships last.

One gap: Slice 1's test block says "Wire `toml` + `cfdb-concepts` deps (both
already present)" — this is incorrect; `cfdb-concepts` is NOT currently present
in `cfdb-query/Cargo.toml`. Slice 1 must add the dep explicitly. The `Tests:`
block for Slice 1 is otherwise well-specified.

Slice 3's test block mentions `cfdb check-predicate --name path-regex --param
pat:literal:'cfdb-query/.*\\.rs'` — the `--param` format in the CLI signature
at §3.4 is `--param <name>:<form>:<value>`, so this would be `--param
pat:literal:cfdb-query/.*\\.rs`. The escaping in the test block uses `--param
pat:literal:'...'` with shell quoting around a regex that contains `\\.` — this
is a test authoring detail the implementer should verify against the shell
expansion, but is not a verdict-level issue.

### Non-goals (§6)

The non-goals are well-scoped. Two merit explicit confirmation:

- "Not a Shell-grep escape hatch" — the RFC replaces `#49`'s "shell-grep for
  file-path checks" with `MATCH (f:File) WHERE f.path =~ $pat`. This is
  correct: the `path-regex.cypher` seed predicate covers this use case within
  the Cypher evaluator. No shell-out is needed. I confirm this reframe.

- "Not `.cfdb/queries/` extension" — this is load-bearing. `§A.14` of
  `council/RATIFIED.md` explicitly ratifies `list_items_matching` as the 16th
  verb and describes `.cfdb/queries/` as the domain of `cfdb violations`. The
  RFC correctly keeps `.cfdb/predicates/` as a sibling, not an extension.

### Invariants (§4)

§4.6 (Canonical-bypass non-regression) is the clean-arch load-bearing
invariant: the param resolver MUST delegate to `cfdb_concepts::load_concept_overrides`
and MUST NOT contain an inline TOML parser. This is stated clearly at §4.6 and
repeated in Slice 1 scope description. The invariant is correctly formulated.

§4.5 (Param-resolver hermeticity) is clean-arch-correct: `resolve_param` takes
`&Path` + `&str` and returns `(String, Param)`. No environment variable
dependency beyond the workspace root path. Pure-function boundary makes the
composition root clean.

The `StoreBackend` trait purity claim at §3.2 ("unchanged") is credible: the
RFC adds no new node labels, no new edge kinds, no new query AST variants. The
evaluator in `cfdb-petgraph/src/eval/` is untouched. `StoreBackend` is not
modified.

---

## Overall

VERDICT: RATIFY

The dependency direction is sound once `cfdb-concepts` is explicitly added to
`cfdb-query/Cargo.toml` (Slice 1). The two-verb design (`check-predicate` vs
`violations`) is correct; they have different composition responsibilities.
`StoreBackend` purity is unaffected. The composition root placement
(`cfdb-cli/src/check_predicate.rs` + `main_dispatch.rs`) mirrors the
established `check.rs` / `violations` pattern.

**Specific change requests (for RFC author, not blocking ratification):**

1. §5.1 should be corrected to state that `cfdb-concepts` is NOT yet a dep of
   `cfdb-query` and that Slice 1 adds it. The current text implies it is already
   present transitively, which is false.
2. Slice 1 scope description: replace "Wire `toml` + `cfdb-concepts` deps (both
   already present)" with "Wire `toml` dep (already present) and add
   `cfdb-concepts = { path = \"../cfdb-concepts\" }` to `cfdb-query/Cargo.toml`
   (new dep)."

These corrections are editorial. They do not change the architecture; they
prevent the implementer from being confused by a false premise. The verdict is
RATIFY.

---

## Peer concerns

**For ddd-specialist:** The RFC's `cfdb-concepts/src/lib.rs` module comment
(line 26-27) names cfdb-query as a future consumer of cfdb-concepts
specifically for a "ContextMap type". The RFC-034 param resolver exposes
`cfdb_concepts::load_concept_overrides` (which returns `ConceptOverrides`, not
a `ContextMap`). If the DDD spec for issue #49 uses the term "context-map" as a
semantic concept, the ddd lens should confirm that `ConceptOverrides` is the
correct type to expose under that name, or recommend a type alias / rename to
surface the intent more explicitly.

**For solid-architect:** The RFC states cfdb-query's seventh responsibility
(param resolver) is "the same shape as (5)" (SkillRoutingTable loader). This is
partially true: both are TOML-backed loaders producing strongly-typed bindings.
However, the SkillRoutingTable loader (`skill_routing.rs`) is a self-contained
parser with no external crate dep, while the param resolver would introduce a
new cross-crate dep (`cfdb-concepts`). The solid lens should address whether
that dep boundary is consistent with cfdb-query's current ISP surface and
whether it changes the crate's "reason to change" calculus.
