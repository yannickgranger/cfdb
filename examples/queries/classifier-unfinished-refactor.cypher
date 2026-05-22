// smoke-skip: parameterized ($context) — bound by RFC-036 classifier suite
// classifier-unfinished-refactor.cypher — §A2.1 class 3 (UnfinishedRefactor).
//
// Items marked `#[deprecated]` still present in the tree. A deprecation
// attribute is an explicit author intent that the item's callers should
// migrate to a canonical replacement; an item that has been deprecated
// but not yet removed IS an unfinished refactor by construction.
//
// # Inputs
// - `:Item.is_deprecated` — extractor-time prop (slice 43-C / issue #106,
//   `extract_deprecated_attr` in cfdb-extractor). Present on every `:Item`
//   for every kind. Does NOT require HIR.
// - `:Item.bounded_context` — populated by `enrich_bounded_context`.
// - `:Item.is_test` — extractor-time filter flag.
//
// # Parameters
// - `$context` — bounded context to scope the finding to.
//
// # Signal vs the §A2.1 ideal
// The addendum §A2.1 signals for class 3 are:
//   - recent commit cluster referencing a context-owning RFC/EPIC (NOT
//     checked here — requires `enrich_git_history` + `enrich_rfc_docs`
//     joined via the owning-context declaration)
//   - `TODO(#issue): migrate` comment (NOT checked here — extractor
//     does not index code comments)
//   - `#[deprecated]` attribute (CHECKED — the v0.1 load-bearing signal)
//   - age_delta > 60 days (NOT checked here — G1 forbids query-time
//     date arithmetic; requires a re-enrichment pass)
//
// The `#[deprecated]` attribute alone is sufficient to classify as
// `UnfinishedRefactor` in v0.1: if an author marked the item deprecated
// and it still exists + is in a non-test module, the migration is by
// definition unfinished. False positives (authors who deprecate but
// never intend to remove) are acceptable — those items are routing
// signal for `/sweep-epic --mode=port` either way, and the operator
// confirms the intent at raid-plan time.
//
// # Degradation
// Empty result on keyspaces where no `#[deprecated]` items exist in
// the requested context — the correct signal ("no unfinished refactor
// in this context" OR "author discipline prevents deprecation drift").
// The CLI orchestrator does NOT attach a warning for this class — the
// signal is unambiguous.
//
// # Determinism
// `ORDER BY qname` — stable row order required by G1.

MATCH (i:Item)
WHERE i.is_deprecated = true
  AND i.is_test = false
  AND i.bounded_context = $context
RETURN i.qname AS qname,
       i.name AS name,
       i.kind AS kind,
       i.crate AS crate,
       i.file AS file,
       i.line AS line,
       i.bounded_context AS bounded_context
ORDER BY qname ASC
