# cfdb Pattern B — vertical split-brain

**Status:** v0.2 MVP (issue #44). Full §A1.3 form defers to the
enrichment-layer work under RFC addendum §A2.2; this doc describes
what ships today and what the TODOs on the query file gate onto.

Related:
- RFC specification: `docs/RFC-cfdb.md` §A1.3
- Rule: `examples/queries/vertical-split-brain.cypher`
- Ground-truth fixture: `examples/queries/fixtures/vertical-split-brain/`
- Scar tests: `crates/cfdb-petgraph/tests/pattern_b_vertical_split_brain.rs`
- Sibling patterns: A (horizontal split-brain — `hsb-by-name.cypher`),
  C (canonical bypass — `ledger-canonical-bypass.cypher`)

## What Pattern B is

Pattern B asks the inverse question of Pattern A:

- **Pattern A (horizontal split-brain).** "Two items with the same
  name in two crates — are they the same concept, duplicated?"
- **Pattern B (vertical split-brain).** "Starting at a user-facing
  entry point and walking DOWN the call chain, does the system have
  more than one way to resolve the same concept?"

Horizontal asks about definitions, vertical asks about reachable
resolution paths. A codebase can be free of horizontal split-brain
(no duplicated type definitions) yet still carry vertical split-brain
(two distinct functions both live on reachable chains from one MCP
tool, both resolving the same param — so changing one without the
other leaves a dormant branch).

## The three divergence kinds (RFC §A1.3 goal)

The full specification enumerates three kinds. v0.2's MVP ships the
first; the other two gate onto enrichment passes that the v0.2
addendum §A2.2 sequences.

### 1. `fork` — resolver fork (ships in v0.2 MVP)

Two distinct resolver `:Item`s are both reachable from the same
`:EntryPoint` through the `CALLS` graph. The shared concept is
inferred from the resolver-name shape: both names match
`^(\w+)_(from|to|for|as)_(\w+)$` with an identical
`<concept>_<keyword>_` prefix and divergent suffixes.

**Worked example (`scar_2651_compound_stop_emits_one_fork_row`):**

```
:EntryPoint{Cli}
    -[:EXPOSES]-> :Item{vsb_fixture::Cli}
                    -[:CALLS]-> :Item{Cli::handle}
                                  -[:CALLS]-> :Item{Engine::build_stop}
                                                -[:CALLS]-> :Item{StopLoss::stop_from_bps}   ← resolver A
                                                -[:CALLS]-> :Item{StopLoss::stop_from_pct}   ← resolver B
```

Cypher output — exactly one row:

| entry_point | entry_qname          | concept_prefix | resolver_a_qname                       | resolver_b_qname                       | divergence_kind |
|-------------|----------------------|----------------|----------------------------------------|----------------------------------------|-----------------|
| `Cli`       | `vsb_fixture::Cli`   | `stop_from_`   | `vsb_fixture::StopLoss::stop_from_bps` | `vsb_fixture::StopLoss::stop_from_pct` | `fork`          |

The rule is pair-wise and orders `a.qname < b.qname`. A three-way
scatter produces `C(3,2) = 3` rows; a seven-way scatter produces 21.

### 2. `drop` — param-drop across layers (v0.3+)

A param `k` enters at an entry point, gets decoded at layer K, but
layer K+1 reads a *different* key `k'` that was never populated from
the original input. Requires `:Param` node + `REGISTERS_PARAM` edge
emission, deferred to RFC addendum §A2.2 enrichment pass 5.

**TODO(#44-followup-param)** on the query file marks this gap.

### 3. `divergent_default` — divergent `::default()` returns (v0.3+)

Two `::default()` implementations are reachable from the same entry
point, return different struct values, and both get used downstream.
Requires extractor-side fingerprinting of `Default::default` bodies,
deferred to RFC addendum §A2.2 enrichment pass 6.

**TODO(#44-followup-default)** on the query file marks this gap.

## What the v0.2 MVP rule knows — and doesn't

### Knows

- `:EntryPoint` exists and carries `kind` ∈ `{cli_command, mcp_tool,
  http_route, cron_job, websocket}` (all five land via issues #86,
  #124, #125 and are green on develop).
- `EXPOSES` edges point from the `:EntryPoint` to its handler item.
- `CALLS` edges connect `:Item` nodes for resolved method dispatch
  (HIR extractor emits `resolved=true` on every edge — issue #94).

### Doesn't know

- Free-function call resolution — the HIR extractor MVP covers
  method dispatch only. A free function `resolve_stop(...)` called
  via a path expression is not in the `CALLS` graph. When both
  resolvers are free functions the rule cannot see them. (Targeted
  by RFC addendum §A1.5 acceptance gate v0.2-4.)
- Concept identity — the rule's `regexp_extract` join on resolver
  names is a heuristic. Two unrelated resolvers happening to share a
  prefix (e.g. `time_from_bar` and `time_from_epoch` — both genuinely
  *are* converting to a `Time`, and neither is unwired) will produce
  a row. Triage removes these; `:Concept` + `LABELED_AS` overlay
  eliminates the FP surface entirely (TODO on the query file).
- Cross-entry-point shapes — a shape like "one resolver reachable
  only from MCP, the other only from CLI, but they encode divergent
  defaults for the same user intent" is a legitimate Pattern B
  failure that the rule misses. The enrichment-layer `:Concept`
  overlay picks these up; the MVP cannot.

## How to use the rule

### Run it ad-hoc against an extracted keyspace

```bash
cfdb query --db .cfdb/db --keyspace <ks> \
  "$(cat examples/queries/vertical-split-brain.cypher)"
```

Expected output on a clean tree: no rows. Any row is a vertical
split-brain candidate.

### Dogfood it against cfdb's own tree

```bash
./target/release/cfdb extract --workspace . --db .cfdb/db --keyspace cfdb
./target/release/cfdb query --db .cfdb/db --keyspace cfdb \
  "$(cat examples/queries/vertical-split-brain.cypher)"
```

The MVP heuristic is conservative — cfdb's own tree is expected to
produce zero rows. Any row here is either a real finding or a FP in
the heuristic that needs promotion to the concept overlay.

### Run the scar tests

```
cargo test -p cfdb-petgraph --test pattern_b_vertical_split_brain
```

Four scar cases: #2651 (one row), #3522 (one row, named concept
prefix), #3545 (3-way scatter, three rows), #3654 (7-way scatter, 21
rows). Plus three negative cases (single resolver → zero rows,
distinct entry points → zero rows, test-tagged resolvers excluded).

## Triage guidance — same as Pattern A

When a row fires on a real codebase, route the finding through the
RFC addendum §A2.3 SkillRoutingTable:

- **Same bounded context.** Two resolvers for one concept inside one
  context → Class 1 (Duplicated Feature) → `/sweep-epic`. Pick the
  head resolver (most calls, canonical naming, or most recent
  migration RFC), `pub use` the other, delete the loser.
- **Different bounded contexts.** Two resolvers in two contexts →
  Class 2 (Context Homonym) → `/operate-module` + council. This is a
  Context Mapping decision (ACL / Shared Kernel / Conformist /
  Published Language), NOT a mechanical dedup. Mechanically
  consolidating a cross-context fork destroys legitimate bounded-
  context isolation — the same warning that guards Pattern A's
  triage note on `hsb-by-name.cypher`.

The bounded-context column is v0.2 enrichment-pass work
(`enrich_bounded_context`, RFC addendum §A2.2 pass 4). Until that
pass lands, hold findings that span visibly different crate
namespaces and route them only once the context label is available.

## Known FP surfaces (triage before routing)

The MVP heuristic produces false positives in these shapes:

1. **Two genuine-but-distinct concepts that share a prefix.**
   `time_from_bar(bar_id)` and `time_from_epoch(epoch_ms)` both
   return `DateTime` but they are two different APIs — the entry
   point intentionally uses one or the other depending on caller
   state. Triage: confirm both resolvers are *actively* selected by
   the entry point (branch condition rather than dead code), then
   add `.cfdb/concepts/<concept>.toml` entries that promote both to
   `CANONICAL_FOR` — the enrichment pass will silence the FP.
2. **Adapter-specific constructors.** Bybit and Capital.com both
   implement `order_from_rest_response`, reachable from a single
   MCP `list_orders` tool through an ACL. The concept is "Order" and
   both are canonical *for their adapter*. Triage as above: the
   resolvers are NOT a fork, they are canonical routings and the
   concept overlay resolves the overlap.
3. **Builder vs constructor idiom.** `build_foo_from_raw` and
   `foo_from_parts` can both appear reachable from the same entry
   point when one is a legacy convenience wrapper over the other.
   Triage: check whether one delegates to the other (via `CALLS`);
   if yes, the inner is the canonical and the outer is fine. Mark
   the outer as `#[doc = "delegates to foo_from_parts"]` or reshape
   so the entry point calls the canonical directly.

These FPs are acceptable for the MVP because the triage cost is
bounded (one grep per row) and they surface real drift-risk surfaces
anyway — a reviewer who looks twice at a false positive often spots
a nearby actual issue.
