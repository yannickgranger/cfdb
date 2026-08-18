# cfdb classifier (issue #48) — `:Finding` taxonomy

The classifier wires the six-class `DebtClass` taxonomy declared in
RFC-cfdb.md §A2.1 into the `cfdb scope` verb. Each
`Finding` row in `ScopeInventory::findings_by_class[<class>]` is emitted
by a dedicated Cypher rule in `examples/queries/classifier-*.cypher`
and populated by `cfdb_classify::ClassifyEngine::scope` (crate
`cfdb-classify`, RFC-059); the `cfdb-cli` handler in
`cfdb-cli/src/scope.rs` only loads the keyspace, prints and exits. The
whole layer rides `cfdb-cli`'s default-on `classify` cargo feature — a
facts-only build (`--no-default-features --features lang-rust`) has no
`scope` / `classify` / `check` verbs and no `cfdb-classify` dependency.

## DIP invariant — skill routing is external

`Finding` does NOT carry a `fix_skill` field. The data layer (classifier)
does not know about the skill layer (orchestration): which skill acts on a
`DebtClass` is the consumer's decision, kept outside this repository. The
architecture tests `crates/cfdb-classify/tests/finding_no_skill_field.rs`
(no routing key on `Finding`) and
`crates/cfdb-query/tests/skill_routing_deleted.rs` (no routing table or
loader in the tree) pin this invariant.

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
| When to trust | High confidence when HIR extraction is available. Empty bucket on syn-only keyspaces is a degradation, not an all-clear — the scope warnings say so. |

### 3. `UnfinishedRefactor`

Items carrying `#[deprecated]` that still exist in the tree. An explicit
authorial signal that the item's callers should migrate.

| Aspect | Value |
|---|---|
| Rule | `examples/queries/classifier-unfinished-refactor.cypher` |
| Required inputs | `:Item.is_deprecated` (always present in syn-only extracts), `:Item.bounded_context` |
| Finding columns | qname, name, kind, crate, file, line, bounded_context |
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
| When to trust | Medium confidence — `cargo-udeps` / `cargo-machete` can cross-validate. On a pure library crate with no `:EntryPoint` nodes, every fn is trivially unreachable and the bucket floods; consumers handle that explicitly. |

## Degradation semantics

Each classifier rule projects empty rows — not errors — when its
required inputs are absent. The CLI orchestrator surfaces per-class
warnings on empty buckets that name the likely missing input
(`--features hir`, `enrich-concepts`, `enrich-reachability`). See
`class_empty_bucket_note` in `cfdb-classify/src/scope.rs`.

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
