// smoke-skip: parameterized ($context) — bound by RFC-036 classifier suite
// classifier-context-homonym.cypher — §A2.1 class 2 (ContextHomonym).
//
// Same last-segment name across distinct bounded contexts, with divergent
// signatures. Ports the #47 `signature-divergent.cypher` shape into the
// classifier's `Finding`-compatible projection.
//
// # Inputs
// - `:Item.bounded_context` — populated by `enrich_bounded_context`.
// - `:Item.signature` — populated by the HIR extractor (syn-only
//   keyspaces do NOT carry this prop; the rule returns empty rows on
//   such keyspaces, which is the correct degradation).
// - `signature_divergent(a, b)` UDF — issue #47, hard-wired in the
//   petgraph evaluator.
// - `last_segment(qname)` UDF — path-tail helper.
//
// # Parameters
// - `$context` — bounded context to scope the finding to. The rule
//   emits only the "a" side of the pair when `a.bounded_context =
//   $context`; the b-side is recorded by the finding for the other
//   context (run twice to get both sides in their respective
//   inventories, which is the correct surgical behaviour).
//
// # Output shape
// Projects `Finding`-compatible columns for the `a` item — the offending
// item in the requested context. The `b` item's qname is NOT surfaced as
// a Finding column; it lives in the evidence trail (future work — v0.3
// emits `evidence[]` per council §A2.2 `:Finding` schema, v0.1 `Finding`
// carries only the structural coordinates).
//
// # DDD guardrail
// This rule is the load-bearing discriminator for class 2 vs class 1.
// IDENTICAL signature + cross-context = Shared Kernel (NOT a finding).
// DIVERGENT signature + cross-context = Context Homonym (finding).
// Misclassification destroys bounded-context isolation (RFC §A2.1 class
// 2 note, gate v0.2-8).
//
// # Determinism
// `ORDER BY qname` — stable row order required by G1.

// NOTE: no `a.qname < b.qname` dedup — we anchor on `a.bounded_context =
// $context` instead, which is itself a one-sided filter: every homonym
// pair surfaces exactly once per (query, context) because only one side
// of the pair carries the queried context. Running the rule twice with
// different `$context` values would emit the other side, which is the
// surgical behaviour the Finding inventory wants.
MATCH (a:Item), (b:Item)
WHERE a.kind IN ['fn', 'method']
  AND b.kind IN ['fn', 'method']
  AND a.qname <> b.qname
  AND a.is_test = false
  AND b.is_test = false
  AND a.bounded_context = $context
  AND b.bounded_context <> $context
  AND last_segment(a.qname) = last_segment(b.qname)
  AND a.signature <> b.signature
  AND signature_divergent(a.signature, b.signature) = true
RETURN a.qname AS qname,
       a.name AS name,
       a.kind AS kind,
       a.crate AS crate,
       a.file AS file,
       a.line AS line,
       a.bounded_context AS bounded_context
ORDER BY qname ASC
