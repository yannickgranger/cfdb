// vertical-split-brain.cypher — Pattern B v0.2 MVP instance (issue #44).
//
// Finds **resolver-fork** vertical split-brain: from a single
// `:EntryPoint`, two distinct resolver-shaped `:Item`s are reachable
// through the call graph and resolve the *same* concept under
// divergent names. When both are reachable from one user-facing entry
// point, the system has two resolution paths for one concept — the
// canonical Pattern B failure mode (RFC-cfdb-v0.2-addendum §A1.3).
//
// # What this MVP detects (v0.2 reduced form)
//
// The RFC §A1.3 goal enumerates three divergence kinds:
//   - `fork`              — same param key, two resolvers reachable
//   - `drop`              — param enters at layer K, different key read
//                           at K+1 never populated
//   - `divergent_default` — two `::default()` impls reachable from one
//                           entry point with divergent values
//
// v0.2 ships primitives for (`EntryPoint`, `EXPOSES`, `CALLS`, `Item`)
// but does NOT yet ship the `:Concept` / `REGISTERS_PARAM` /
// `LABELED_AS` / `CANONICAL_FOR` overlay edges that would let the
// query join on a declared concept identifier. `drop` and
// `divergent_default` need `:Param` nodes + param-key name resolution
// from entry points down through the call chain — work deferred to
// the §A2.2 enrichment pipeline.
//
// This MVP therefore encodes the **`fork`** kind only, using a
// name-shape heuristic on `:Item.name` to infer concept identity:
// two items whose names share a common `<concept>` prefix or segment
// but diverge on the suffix (the resolution source / input type) are
// candidate divergent resolvers. Example match:
//
//     stop_loss_from_bps          ← resolver A (reachable from ep)
//     stop_loss_from_pct          ← resolver B (reachable from ep)
//     ──────────────▲─────
//     shared concept prefix
//
// If both resolvers are reachable from the SAME entry point, one of
// them is an unwired parallel path — the #2651 compound-stop failure
// shape.
//
// # TODOs — promote to full §A1.3 form in v0.3+
//
// TODO(#44-followup-concept): Once `:Concept` nodes + `LABELED_AS`
//   edges land (RFC addendum §A2.2 enrichment pass 3), replace the
//   name-shape heuristic with a join on `a-[:LABELED_AS]->c<-[:LABELED_AS]-b`
//   — same concept, different items, same entry point.
// TODO(#44-followup-param): Once `:Param` + `REGISTERS_PARAM` edges
//   land (RFC addendum §A2.2 enrichment pass 5), add the `drop` kind:
//   entry point registers param K, reachable resolver reads K', K ≠ K'.
// TODO(#44-followup-default): Once `::default()` impl fingerprints
//   are emitted, add the `divergent_default` kind by joining on two
//   `kind="fn"` items named `default` whose parent structs share a
//   concept prefix.
// TODO(#44-followup-evidence): v0.2 `:Item` nodes carry `file` and
//   `line` — surface them as `evidence_a` / `evidence_b` once the
//   projection layer supports per-row expression chaining. Currently
//   qname alone identifies the call site to a human reader.
//
// # Scope
//
// - Matches only method-dispatch call chains (the HIR extractor's
//   coverage in v0.2 — free-function path calls do not yet emit
//   `CALLS` edges). Accepted limitation; RFC §A1.5 gate v0.2-4.
// - Name-shape heuristic is intentionally conservative: it requires
//   both resolvers to match the `^(\w+)_(from|to|for|as)_(\w+)$`
//   shape, which is the common Rust resolver idiom. This keeps the
//   false-positive rate low at the cost of missing resolvers that
//   use trait-impl / bare-word names. Promotion to concept-overlay
//   eliminates the heuristic entirely.
// - BFS bound `*1..8` — v0.2 `DEFAULT_VAR_LENGTH_MAX = 8` (evaluator
//   default). Deeper call chains truncate; acceptable for MVP.
//
// # Output columns
//
// - `entry_point`      — entry-point display name (CLI verb, HTTP
//                        path, MCP tool name, cron schedule)
// - `entry_qname`      — entry-point handler qname (disambiguates two
//                        MCP tools with the same display name)
// - `concept_prefix`   — shared name segment inferred from both
//                        resolver names
// - `resolver_a_qname` — first resolver's qname
// - `resolver_b_qname` — second resolver's qname
// - `divergence_kind`  — always `"fork"` in this MVP; v0.3 adds
//                        `"drop"` and `"divergent_default"`
//
// Evidence columns (`file`, `line`) are v0.3 work — see TODOs above.
//
// Known motivating bugs (from RFC §A1.3):
//   - qbot-core #2651  — compound-stop param not propagated through
//                        all resolver paths
//   - qbot-core #3522  — pair-resolution splits between `lookup_pair`
//                        and `resolve_pair_by_alias`
//   - qbot-core #3545  — `build_resolved_config` 3-way scatter
//   - qbot-core #3654  — 7 split resolution points for one concept
//
// # Usage
//
//   cfdb query --db <dir> --keyspace <ks> "$(cat vertical-split-brain.cypher)"
//
// Expected: empty on a clean tree. Any row is a vertical split-brain
// candidate — two resolvers for one concept reachable from one entry
// point. Route per the §A2.3 `SkillRoutingTable`:
//   - Same bounded context → /sweep-epic (consolidate)
//   - Cross context        → /operate-module (context-mapping decision)

MATCH (ep:EntryPoint)-[:EXPOSES]->(handler:Item),
      (handler)-[:CALLS*1..8]->(a:Item),
      (handler)-[:CALLS*1..8]->(b:Item)
WHERE a.kind = 'fn'
  AND b.kind = 'fn'
  AND a.qname < b.qname
  AND a.name =~ '^(\w+)_(from|to|for|as)_(\w+)$'
  AND b.name =~ '^(\w+)_(from|to|for|as)_(\w+)$'
  AND regexp_extract(a.name, '^(\w+)_(?:from|to|for|as)_') =
      regexp_extract(b.name, '^(\w+)_(?:from|to|for|as)_')
  AND regexp_extract(a.name, '_(?:from|to|for|as)_(\w+)$') <>
      regexp_extract(b.name, '_(?:from|to|for|as)_(\w+)$')
  AND a.is_test = false
  AND b.is_test = false
RETURN ep.name AS entry_point,
       ep.handler_qname AS entry_qname,
       regexp_extract(a.name, '^(\w+)_(?:from|to|for|as)_') AS concept_prefix,
       a.qname AS resolver_a_qname,
       b.qname AS resolver_b_qname,
       'fork' AS divergence_kind
ORDER BY entry_point ASC, concept_prefix ASC, resolver_a_qname ASC
