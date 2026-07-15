// layering-up-call.cypher — RFC-050 §7 slice 50-C: the up-call detector.
//
// Surfaces every CALLS edge that runs AGAINST the workspace tier ordering —
// a function in a LOWER-tier crate calling a function in a HIGHER-tier crate.
// This is the canonical architectural-layering violation the `crate_tier`
// overlay (RFC-050 50-A) exists to make queryable.
//
// # Tier semantics (RFC-050 §3.2)
//
// `:Crate.crate_tier` is the topological longest-path depth of a crate in the
// intra-workspace normal-`[dependencies]` DAG. A foundation crate with no
// in-workspace normal deps is tier 0 (`cfdb-core`); the composition root that
// (transitively) depends on everything is the highest tier (`cfdb-cli`). So
// dependencies point DOWN the stack: if A depends on B then tier(A) > tier(B).
//
// A legitimate call therefore runs from a HIGHER tier to a LOWER tier — you
// may only call code in a crate you depend on (tier(caller) > tier(callee)).
// The violation is the inverse: tier(caller_crate) < tier(callee_crate) — a
// crate calling UP into a layer above it, which no acyclic normal-deps DAG
// can justify. The join reaches each item's crate_tier in one `IN_CRATE` hop
// (RFC-050 killed the per-item `:Item.layer` denormalisation — 50-B).
//
// # HIR requirement (--features hir) — READ BEFORE RUNNING
//
// Cross-crate CALLS are only present when cfdb is built with `--features hir`
// and extracted with `--hir` (the rust-analyzer-backed resolver populates
// resolved `CALLS` + `EXPOSES`). A syn-only extract emits NO resolved
// cross-crate CALLS edges, so this query returns zero rows on a syn keyspace
// REGARDLESS of whether up-calls exist — a false "clean". The self-dogfood
// assertion (zero up-calls in cfdb) is therefore only meaningful on the
// `--hir` keyspace (`ci-hir-self-audit-nightly.yml`); on the PR-time syn
// keyspace this query is smoke-parsed only (row count ignored). Do NOT read a
// zero-row syn result as a passing layering assertion.
//
// # Second false-clean: stubbed callees carry no crate (--hir path)
//
// Even under --hir a genuine up-call can silently drop out. When the resolver
// produces a CALLS edge to a callee that was not itself extracted as a node,
// `cfdb-hir-petgraph-adapter::synthesize_callee_stubs` synthesises a stub
// :Item for it via `build_callee_stub` (`lib.rs:191-210`). That stub carries a
// `crate` STRING prop but NO `:IN_CRATE` edge to a `:Crate` node. Its own doc
// (`lib.rs:145-148`) admits the stub path fires for "an in-workspace callee
// that somehow missed syn extraction (conditional compilation, ...)". An
// up-call whose callee is such a stub never binds the
// `MATCH (callee)-[:IN_CRATE]->(dc:Crate)` hop, so the row is dropped and the
// violation is not reported — a false clean distinct from the syn-only one
// above. A zero-row result is therefore not proof of a clean layer when
// cfg-gated code is in play. The fix is upstream (the stub should emit
// IN_CRATE, or the join should read crate_tier off the `crate` prop), not in
// this query — recorded here so a reader does not over-trust the empty result.
//
// # Why the is_test filter is load-bearing (RFC-050 §3.2 / §5)
//
// The tier DAG is normal-`[dependencies]`-only: dev/build deps are excluded so
// the common test-only back-edge does not cycle (cfdb-self has
// `cfdb-cli --normal--> cfdb-hir-extractor` AND
// `cfdb-hir-extractor --dev--> cfdb-cli`). A dev-dep back-edge is the ONE way a
// lower-tier crate can physically reference a higher-tier symbol — from test
// code. Those calls carry `is_test = true` on the caller, so filtering
// `caller.is_test = false` is the query-side analog of the extractor's
// `kind == Normal` scoping: without it, cfdb-self's own dev-dep test calls
// would surface as spurious up-calls and falsify the "zero up-calls"
// assertion. `callee.is_test = false` drops the symmetric case (production
// code entering a test-only callee), matching the arch-ban convention.
//
// # Output columns
//
//   caller_crate  — :Crate.name of the LOWER-tier crate (the offender)
//   caller_tier   — its crate_tier (the smaller depth)
//   caller_qname  — qname of the calling :Item
//   callee_crate  — :Crate.name of the HIGHER-tier crate called up into
//   callee_tier   — its crate_tier (the larger depth)
//   callee_qname  — qname of the called :Item
//
// # Usage
//
//   cfdb query --db <dir> --keyspace <ks> "$(cat examples/queries/layering-up-call.cypher)"
//
// Expected: empty result on a clean tiered workspace. Any row is an up-call —
// a lower layer reaching up into a higher one.
//
// # Status — canonical query, candidate ban-rule (RFC-050 §6)
//
// RFC-050 §6 scopes ENFORCEMENT out of this slice ("Enforcing layering rules
// (banning up-calls) ... is a ban-rule built on top of this overlay, not part
// of it"). This file is therefore NOT named `arch-ban-*`: the self/cross
// dogfood loops auto-enforce only `examples/queries/arch-ban-*.cypher`, and the
// PR-time cross-dogfood runs syn-only where this rule would be vacuous. It
// ships as the canonical detector (smoke-parsed on both the syn and --hir
// keyspaces); promotion to an enforced HIR-gated ban rule is a later step —
// a same-directory rename to `arch-ban-up-call.cypher`, gated on the nightly
// `--hir` self-audit, once the zero-violation-on-develop proof is run under HIR.

MATCH (caller:Item)-[:CALLS]->(callee:Item)
MATCH (caller)-[:IN_CRATE]->(cc:Crate)
MATCH (callee)-[:IN_CRATE]->(dc:Crate)
WHERE cc.crate_tier < dc.crate_tier
  AND caller.is_test = false
  AND callee.is_test = false
RETURN cc.name AS caller_crate,
       cc.crate_tier AS caller_tier,
       caller.qname AS caller_qname,
       dc.name AS callee_crate,
       dc.crate_tier AS callee_tier,
       callee.qname AS callee_qname
ORDER BY caller_crate ASC, callee_crate ASC, caller_qname ASC, callee_qname ASC
