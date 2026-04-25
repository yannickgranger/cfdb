// classifier-random-scattering.cypher — §A2.1 class 4 (RandomScattering).
//
// Surfaces the Pattern B "fork" vertical split-brain shape: from a single
// `:EntryPoint`, two distinct resolver-shaped `:Item`s are reachable with
// the same concept prefix but divergent suffixes. Copy-paste-flavoured
// resolver drift reachable from one user-facing action.
//
// # Inputs
// - `:EntryPoint` nodes + `EXPOSES` edges — HIR extractor only.
// - `CALLS` edges — HIR extractor only.
// - `:Item.bounded_context` — `enrich_bounded_context`.
//
// # Parameters
// - `$context` — bounded context to scope the finding to. Restricts BOTH
//   resolvers to the context (scatter within a context). A cross-context
//   fork is a ContextHomonym signal handled by class 2.
//
// # Output shape
// Projects `Finding`-compatible columns for resolver A (the lex-smaller
// side). Resolver B is not surfaced as a Finding column — it lives in
// the evidence trail (v0.3 `:Finding.evidence[]`).
//
// # Signal vs §A2.1 ideal
// The addendum §A2.1 class 4 signals are:
//   - identical / near-identical AST shape (NOT checked — `signature_hash`
//     Jaccard clustering is v0.3)
//   - no refactor intent, no RFC reference (NOT checked — requires
//     `enrich_rfc_docs` cross-join)
//   - no deprecation marker (NOT checked — would subsume class 3)
//   - age_delta < 14 days (NOT checked — G1 forbids date arithmetic)
//   - typically short functions (NOT checked — no LoC on `:Item` yet)
//
// The `fork` shape alone is v0.1's pragmatic signal: two divergent
// resolvers reachable from one entry point, both inside one bounded
// context, is the scatter pattern in its simplest form. Refinement to
// the full §A2.1 signal matrix is v0.3 (tracked as a TODO in
// `vertical-split-brain.cypher`).
//
// # Degradation
// Empty result on keyspaces without HIR extraction — no `:EntryPoint`
// nodes → no match. CLI orchestrator attaches a warning naming the HIR
// dependency.
//
// # Determinism
// `ORDER BY qname` — stable row order required by G1.

// NOTE: traversal is via `:Item.reachable_from_entry`, pre-computed by
// `enrich-reachability` — it walks BOTH `INVOKES_AT` + `CALLS` (HIR's
// two-hop dispatch shape). Using a raw `-[:CALLS*1..8]->` chain here
// would miss the HIR shape because CALLS edges originate from CallSite
// nodes, not directly from Items. The `reachable_from_entry` bool
// sidesteps that without expanding the Cypher parser.
// `kind IN ['fn', 'method']` matches the Context-Homonym rule's surface —
// impl-block methods are syntactically `fn` items in the syn AST but cfdb
// emits them with kind='method' (item_visitor distinction). A resolver
// fork often lives as sibling methods on a dispatcher struct; restricting
// to bare fn would miss that shape. The scar fixture's `Dispatcher::
// compute_qty_from_{bps,pct}` pair is deliberately method-based because
// HIR's call_site_emitter resolves MethodCallExpr (producing CALLS edges
// required by reachability BFS) but not CallExpr.
MATCH (a:Item), (b:Item)
WHERE a.kind IN ['fn', 'method']
  AND b.kind IN ['fn', 'method']
  AND a.qname < b.qname
  AND a.reachable_from_entry = true
  AND b.reachable_from_entry = true
  AND a.is_test = false
  AND b.is_test = false
  AND a.bounded_context = $context
  AND b.bounded_context = $context
  AND a.name =~ '^(\\w+)_(from|to|for|as)_(\\w+)$'
  AND b.name =~ '^(\\w+)_(from|to|for|as)_(\\w+)$'
  AND regexp_extract(a.name, '^(\\w+)_(?:from|to|for|as)_') =
      regexp_extract(b.name, '^(\\w+)_(?:from|to|for|as)_')
  AND regexp_extract(a.name, '_(?:from|to|for|as)_(\\w+)$') <>
      regexp_extract(b.name, '_(?:from|to|for|as)_(\\w+)$')
RETURN a.qname AS qname,
       a.name AS name,
       a.kind AS kind,
       a.crate AS crate,
       a.file AS file,
       a.line AS line,
       a.bounded_context AS bounded_context
ORDER BY qname ASC
