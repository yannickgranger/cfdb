// vertical-split-brain-drop.cypher — Pattern B `drop` kind (issue #297 / #44 follow-up).
//
// Companion to `vertical-split-brain.cypher` (which encodes the `fork`
// kind only — see that file's `# TODOs — promote to full §A1.3 form
// in v0.3+` block, specifically `TODO(#44-followup-param)`).
//
// `drop` shape — RFC-cfdb.md Addendum B §A1.3:
//   - An `:EntryPoint` registers a wire-level param key K (clap field,
//     MCP arg, etc. — the `:EntryPoint -[:REGISTERS_PARAM]-> :Field|
//     :Variant|:Param` edge produced by `cfdb-hir-extractor`'s entry-
//     point emitter).
//   - The handler chain reaches a resolver `:Item` whose `:Param` named
//     K matches — the wire key flows through this layer correctly.
//   - The handler chain ALSO reaches a sibling resolver `:Item` whose
//     `:Param` reads a DIFFERENT key K' that the entry point never
//     registered. K' is "dropped" — the wire-K is silently replaced
//     by an unwired key in this branch.
//
// This is the qbot-core #2651 compound-stop failure mode: `stop_atr_mult`
// accepted by MCP, dropped at one of the stop-policy layers because that
// layer's resolver reads `active_mult` (a different key) from the same
// config struct, and `active_mult` is never wired through the
// REGISTERS_PARAM surface — `stop_atr_mult=1.5` and `stop_atr_mult=8`
// produced byte-identical output to 25 decimal places on BTC/USDT
// Momentum VT20 because the active-multiplier branch read a key the
// wire form never populated.
//
// `hsb-by-name` and `signature-divergent` cannot see this: the failure
// requires call-graph traversal joined with param-key resolution, which
// before #209 (`:Param`) + #219 (`REGISTERS_PARAM`) had no schema slot.
//
// # Why a separate file (not extending vertical-split-brain.cypher)
//
// The cfdb-query v0.1 subset has no `UNION` (verified at
// `crates/cfdb-query/src/parser/`) — the `fork` rule and the `drop`
// rule have structurally different MATCH shapes (`fork` joins on a
// name-shape regex; `drop` joins on `REGISTERS_PARAM`/`HAS_PARAM`
// graph-edge equality). They cannot share a single query body. Per
// RFC-039 §3.5.1 sentinel-pattern semantics, one cypher per sentinel
// keeps each rule independently testable + smoke-runnable (#339).
//
// # Constraints satisfied by this rule
//
// - Anchor on `:EntryPoint` because the rule is meaningless without a
//   wire-registered K — empty-binding is the desired pass shape (zero
//   `:EntryPoint` nodes => no rows => smoke green; matches the `fork`
//   rule's existing convention).
// - BFS bound `*1..8` mirrors the `fork` rule per RFC-cfdb §A1.5
//   v0.2-4 gate (DEFAULT_VAR_LENGTH_MAX = 8).
// - Skip test fns via `is_test = false` — same convention.
// - Require `layer_k <> layer_kp1` so the rule never reports a single
//   resolver against itself.
// - Require K ≠ K' so a resolver that reads the wire key on multiple
//   params doesn't fire spuriously.
//
// # Known false-positive class — "both keys wire-registered"
//
// The §A1.3 ideal predicate also requires that the divergent key K'
// is NOT itself wire-registered (i.e. the user does not supply K' at
// the wire form). In Cypher that would be:
//
//     AND NOT EXISTS { MATCH (ep)-[:REGISTERS_PARAM]->(other)
//                      WHERE other.name = divergent.name }
//
// **The cfdb-query v0.1 subset does not bind outer-scope variables
// inside `NOT EXISTS { ... WHERE ... }` subqueries** — verified
// empirically: `WHERE other.name = wire.name` evaluates as if
// `wire.name` is `Null`, and `compare_propvalues` returns false on
// Null comparison, so EXISTS is always false / NOT EXISTS always true,
// making the filter a no-op. The subset's `NOT EXISTS` is restricted
// to constant predicates (e.g. `WHERE other.name = "literal"`).
//
// Consequence: this rule fires on the legitimate "compound stop
// accepts both keys" shape — when the entry point registers BOTH the
// matching key K and the divergent key K', a real reviewer would
// classify this as "wire-form has redundant aliases", not a drop.
// Operator triages such rows manually until the v0.2 query subset
// gains outer-scope bindings inside `NOT EXISTS`. This is the same
// trade-off the `fork` rule's name-shape heuristic accepts vs the
// future `LABELED_AS`-based concept join.
//
// Mitigations operator can apply post-rule: `cfdb query
// 'MATCH (ep:EntryPoint)-[:REGISTERS_PARAM]->(other) WHERE
//  other.name = "<divergent_key>" RETURN ep.name'` — if the divergent
// key is itself wire-registered for the same entry point, the row is
// the legitimate alias case, not a drop.
//
// # Output columns
//
// - `entry_point`        — entry-point display name
// - `entry_qname`        — handler qname disambiguator
// - `wire_param`         — name of the wire-registered key K
// - `matching_resolver`  — qname of the resolver that reads K (layer K)
// - `divergent_resolver` — qname of the resolver that reads K' (layer K+1)
// - `divergent_key`      — name of the dropped key K' the divergent
//                          resolver reads instead
// - `divergence_kind`    — always `'drop'` for this rule
//
// # Known motivating bugs
//
//   - qbot-core #2651  — compound-stop param drop (stop_atr_mult vs
//                        active_mult); the qbot-core #4102 sub-finding
//                        F1 has the same shape (FillModelKind handler
//                        bypasses domain FromStr).
//
// # Usage
//
//   cfdb query --db <dir> --keyspace <ks> "$(cat vertical-split-brain-drop.cypher)"
//
// Expected: empty on a clean tree. Any row is a `drop` candidate —
// route per the §A2.3 SkillRoutingTable (compound-stop layer-isolation
// or canary-test scar) when the same bounded context owns both
// resolvers; otherwise context-mapping decision via `/operate-module`.

// The cfdb-query subset does not allow:
//   (a) appending an edge after a variable-length pattern (`*1..8`)
//       inside the same path — the parser terminates the path at the
//       `:Item` node, so the `:Item -[:HAS_PARAM]-> :Param` hop has
//       to be a separate comma-separated path pattern.
//   (b) MATCH after WITH — the subset's pipeline is `MATCH (+ OPTIONAL
//       MATCH)+ → WITH → WHERE → RETURN`; a second MATCH after the
//       first WITH is rejected.
//
// Both verified at `crates/cfdb-query/src/parser/`. So the rule lives
// in a single MATCH clause whose patterns are joined by commas.

MATCH (ep:EntryPoint)-[:REGISTERS_PARAM]->(wire),
      (ep)-[:EXPOSES]->(handler:Item),
      (handler)-[:CALLS*1..8]->(layer_k:Item),
      (handler)-[:CALLS*1..8]->(layer_kp1:Item),
      (layer_k)-[:HAS_PARAM]->(matched:Param),
      (layer_kp1)-[:HAS_PARAM]->(divergent:Param)
WHERE matched.name = wire.name
  AND divergent.name <> wire.name
  AND layer_k.qname <> layer_kp1.qname
  AND layer_k.is_test = false
  AND layer_kp1.is_test = false
RETURN ep.name AS entry_point,
       ep.handler_qname AS entry_qname,
       wire.name AS wire_param,
       layer_k.qname AS matching_resolver,
       layer_kp1.qname AS divergent_resolver,
       divergent.name AS divergent_key,
       'drop' AS divergence_kind
ORDER BY entry_point ASC, wire_param ASC, matching_resolver ASC, divergent_resolver ASC
