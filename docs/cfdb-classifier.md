# cfdb classifier (issue #48) — `:Finding` taxonomy and skill routing

The classifier wires the six-class `DebtClass` taxonomy declared in
RFC-cfdb-v0.2-addendum-draft.md §A2.1 into the `cfdb scope` verb. Each
`Finding` row in `ScopeInventory::findings_by_class[<class>]` is emitted
by a dedicated Cypher rule in `examples/queries/classifier-*.cypher`
and populated by the CLI orchestrator in `cfdb-cli/src/scope.rs`.

## DIP invariant — skill routing is external

`Finding` does NOT carry a `fix_skill` field. The data layer (classifier)
does not know about the skill layer (orchestration). Skill routing lives
in `.cfdb/skill-routing.toml` and is parsed by
`cfdb_query::SkillRoutingTable`. The architecture test
`crates/cfdb-query/tests/finding_no_skill_field.rs` pins this invariant.

## The six classes

### 1. `DuplicatedFeature`

Two independent implementations of the same concept **within the same
bounded context**. The Pattern A horizontal split-brain shape restricted
to same-context struct/enum/trait pairs.

| Aspect | Value |
|---|---|
| Rule | `examples/queries/classifier-duplicated-feature.cypher` |
| Required inputs | `:Item.name`, `:Item.kind`, `:Item.bounded_context` (always present in syn-only extracts) |
| Finding columns | qname, name, kind, crate, file, line, bounded_context |
| Skill route | `/sweep-epic` (council not required) |
| When to trust | High confidence — exact name + kind match within one context is an unambiguous split-brain signal. |

### 2. `ContextHomonym`

Same last-segment name across **distinct bounded contexts** with
divergent signatures. The load-bearing discriminator for "Shared Kernel
(identical sig = intentional co-ownership) vs Homonym (divergent sig =
accidental name collision)".

| Aspect | Value |
|---|---|
| Rule | `examples/queries/classifier-context-homonym.cypher` |
| Required inputs | `:Item.signature` (HIR-only), `:Item.bounded_context`, `signature_divergent(a, b)` UDF |
| Finding columns | qname, name, kind, crate, file, line, bounded_context |
| Skill route | `/operate-module` with `council_required = true`. **NEVER `/sweep-epic`** — mechanical dedup on a homonym destroys bounded-context isolation. |
| When to trust | High confidence when HIR extraction is available. Empty bucket on syn-only keyspaces is a degradation, not an all-clear — the scope warnings say so. |

### 3. `UnfinishedRefactor`

Items carrying `#[deprecated]` that still exist in the tree. An explicit
authorial signal that the item's callers should migrate.

| Aspect | Value |
|---|---|
| Rule | `examples/queries/classifier-unfinished-refactor.cypher` |
| Required inputs | `:Item.is_deprecated` (always present in syn-only extracts), `:Item.bounded_context` |
| Finding columns | qname, name, kind, crate, file, line, bounded_context |
| Skill route | `/sweep-epic --mode=port` (council not required) |
| When to trust | Medium confidence — the attribute is a clear signal of intent, but authors sometimes mark items deprecated without actual migration plans. The raid-plan operator confirms at invocation time. |

### 4. `RandomScattering`

Pattern B "fork" shape: two resolvers with shared concept prefix and
divergent suffixes, both reachable from one `:EntryPoint`, both in the
same bounded context.

| Aspect | Value |
|---|---|
| Rule | `examples/queries/classifier-random-scattering.cypher` |
| Required inputs | `:EntryPoint` + `EXPOSES` + `CALLS` (HIR-only), `:Item.bounded_context` |
| Finding columns | qname, name, kind, crate, file, line, bounded_context (of resolver A — the lex-smaller side) |
| Skill route | `/boy-scout` (council not required) — fix inline during adjacent work |
| When to trust | Medium confidence — the name-shape heuristic (`^(\w+)_(from\|to\|for\|as)_(\w+)$`) is conservative. False negatives on trait-impl / bare-word names. v0.3 replaces the heuristic with `:Concept` overlay joins. |

### 5. `CanonicalBypass`

Items declared `CANONICAL_FOR` some `:Concept` that no `:EntryPoint`
reaches. Either callers bypass the canonical wire form, or the canonical
has no callers at all.

| Aspect | Value |
|---|---|
| Rule | `examples/queries/classifier-canonical-bypass.cypher` |
| Required inputs | `:Concept` + `CANONICAL_FOR` edges (via `cfdb enrich-concepts`), `reachable_from_entry` (HIR-only, via `cfdb enrich-reachability`), `:Item.bounded_context` |
| Finding columns | qname, name, kind, crate, file, line, bounded_context |
| Skill route | `/sweep-epic` (council not required) — rewire callers through canonical or delete if dead |
| When to trust | Medium confidence — the classifier's generic form surfaces CANONICAL_UNREACHABLE (a superset of BYPASS_*). Per-concept BYPASS_REACHABLE / BYPASS_DEAD rules (`examples/queries/canonical-bypass-{reachable,dead}.cypher`) remain available for targeted triage when the concept's bypass method name is known. |

### 6. `Unwired`

fn / method items with `reachable_from_entry = false` that are not
themselves `:EntryPoint` handlers. Code that compiles but no user
action triggers.

| Aspect | Value |
|---|---|
| Rule | `examples/queries/classifier-unwired.cypher` |
| Required inputs | `reachable_from_entry` (HIR-only, via `cfdb enrich-reachability`), `:Item.bounded_context`, `:Item.kind` |
| Finding columns | qname, name, kind, crate, file, line, bounded_context |
| Skill route | `/boy-scout` delete (council not required). Orchestrator routes `TODO(#N)`-tagged items to the issue owner at invocation time, not via a separate config row. |
| When to trust | Medium confidence — `cargo-udeps` / `cargo-machete` can cross-validate. On a pure library crate with no `:EntryPoint` nodes, every fn is trivially unreachable and the bucket floods; consumers handle that explicitly. |

## `SkillRoutingTable` — the external policy

File: `.cfdb/skill-routing.toml` (workspace-local, sibling to
`.cfdb/concepts/`).

```toml
schema_version = 1

[classes.duplicated_feature]
skill = "sweep-epic"
council_required = false

[classes.context_homonym]
skill = "operate-module"
council_required = true

[classes.unfinished_refactor]
skill = "sweep-epic"
mode = "port"
council_required = false

[classes.random_scattering]
skill = "boy-scout"
council_required = false

[classes.canonical_bypass]
skill = "sweep-epic"
council_required = false

[classes.unwired]
skill = "boy-scout"
council_required = false
```

Consumers load it via:

```rust
use cfdb_query::{DebtClass, SkillRoutingTable};

let table = SkillRoutingTable::from_path(Path::new(".cfdb/skill-routing.toml"))?;
if let Some(route) = table.route(DebtClass::ContextHomonym) {
    assert_eq!(route.skill, "operate-module");
    assert!(route.council_required);
}
```

## Degradation semantics

Each classifier rule projects empty rows — not errors — when its
required inputs are absent. The CLI orchestrator surfaces per-class
warnings on empty buckets that name the likely missing input
(`--features hir`, `enrich-concepts`, `enrich-reachability`). See
`class_empty_bucket_note` in `cfdb-cli/src/scope.rs`.

## Follow-ups deferred to v0.3+

- `classifier.cypher` as a single UNION/CASE query instead of six rules
  — parser gap (no `UNION`, no `CASE WHEN`). Tracked under a v0.3
  parser-scope RFC.
- `signature_hash` Jaccard clustering for class 1 / class 4 — requires
  HIR-mode keyspaces to carry the `signature_hash` prop.
- `enrich_git_history` + `enrich_rfc_docs` join for class 3 refinement
  — adds RFC-reference + age-delta signals beyond `#[deprecated]`.
- `:Finding` `id` / `confidence` / `evidence[]` columns — the RFC's
  §A2.2 `:Finding` schema envisions richer rows; v0.1 ships the
  structural coordinates only and defers the richer projection to a
  follow-up slice.
