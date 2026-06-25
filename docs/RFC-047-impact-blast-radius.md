# RFC-047 — Impact / blast-radius query (`cfdb impact`)

- **Status:** DRAFT — pending architect hardening + council. (Borrowed candidate **C1** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md); validated against cfdb in [`studies/003`](../studies/003-cfdb-understand-discovery.md).)
- **Issue:** none yet (filed only after ratification).
- **Schema impact:** **none.** No new node/edge label, no attribute, **no `SchemaVersion` bump.** Composes existing facts only.
- **Companion:** none (no schema surface → no `graph-specs-rust` lockstep).
- **Origin:** `Understand-Anything` `understand-diff` skill (changed files → trace edges → affected components).

---

## 1. Problem

"If I change `fn X`, what transitively breaks?" is the highest-frequency question a code graph can answer, and cfdb already holds every fact needed to answer it — the 2197-function `CALLS` graph (`studies/003 §1`) plus reachability enrichment (`reachable_from_entry`, `reachable_from_production_entry`). What is missing is a **first-class affordance**: today a consumer must hand-author a variable-length `CALLS*` traversal and know the seed qnames. Downstream consumers (qbot-core, agentry) want "what is the blast radius of this PR" on every change, keyed off a git ref, not off hand-written Cypher.

This is a pure *ergonomics + composition* gap, not a missing-fact gap — which is exactly why it is the lowest-risk, highest-leverage borrow.

## 2. Scope

**Ships:**
1. A **canonical reverse-reachability query pattern** (parameterised Cypher-subset) that, given a set of seed `:Item` qnames, returns the transitively-affected callers.
2. A **`cfdb impact` CLI dispatch branch** that resolves "changed items" from a git ref (`--since <ref>` → `git diff --name-only` → `:Item`s whose `file` attribute matches) **or** explicit `--item <qname>` seeds, builds the canonical query, and runs it through the existing `query` verb.
3. Optional **production-reachability ranking** — intersect affected items with `reachable_from_production_entry = true` so consumers see "of the blast radius, what is reachable from a production entry point."

**Does not ship:** any new `StoreBackend`/`EnrichBackend` trait method (the 11-verb ceiling is untouched — see §4), any new fact, any diff renderer.

## 3. Design

### 3.1 No new vocabulary, no trait verb
`impact` is an **adapter-level CLI dispatch branch** in `cfdb-cli` that *composes* the existing `query` verb by constructing a parameterised `Query` AST — the same pattern RFC-046 §3.5 set for trace ingest (a dedicated dispatch branch, no new `cfdb-core` trait method). The 7+7 trait surface and the 11-verb API ceiling (`RFC-036 §3`) are not touched.

### 3.2 Canonical query
Reverse traversal over `CALLS` from the seed set:

```cypher
MATCH (seed:Item)<-[:CALLS*1..]-(affected:Item)
WHERE seed.qname IN $seeds
RETURN DISTINCT affected.qname, affected.file, affected.reachable_from_production_entry
ORDER BY affected.reachable_from_production_entry DESC, affected.qname
```

The `$seeds` binding is supplied via `query_with_input` (the existing parameter-binding path). Variable-length `CALLS*1..` is already in the Cypher subset (`RFC-034`); if an upper bound is needed for very dense graphs, `--max-depth N` maps to `CALLS*1..N`.

### 3.3 Seed resolution (`cfdb-cli`, deterministic)
- `--item <qname>` (repeatable): seeds are exactly the given qnames.
- `--since <ref>`: run `git diff --name-only <ref>..HEAD` in the workspace, then resolve seeds as `MATCH (i:Item) WHERE i.file IN $changedFiles RETURN i.qname`. File-granular seeding is the v1 — it is deterministic and needs no second extract.
- Seeds that resolve to zero items emit a `Warning`, not an error (a docs-only or non-code change has an empty blast radius — a correct answer).

### 3.4 Output
Plain rows through the standard `QueryResult` surface (qname, file, production-reachable flag). Rendering/formatting is the consumer's job — cfdb returns the node set, honouring the "cfdb should not know about its clients" boundary.

## 4. Invariants

- **Read-only (`G2`).** `impact` issues only `query`/`query_with_input`; it never mutates the graph.
- **Determinism.** Given a keyspace and a seed set, the affected set is a deterministic function of the stored `CALLS` edges. `--since` seeding is deterministic given `(workspace SHA, ref)` because `git diff --name-only` is.
- **No schema surface.** No `SchemaVersion` bump, no `graph-specs-rust` companion PR, no recall-corpus *fact* extension (the recall gate concerns extracted facts; this adds none).
- **Verb ceiling.** Untouched — `impact` composes `query`, it is not a new trait verb (§3.1).

## 5. Architect lenses

> **DRAFT — to be filled by next-session architect hardening before council.** Expected focus areas, pre-seeded for the architects:
- **clean-arch:** confirm `impact` lives in `cfdb-cli` as a composition over the `query` port, with zero new logic leaking into `cfdb-core`; the seed-resolution `git` shell-out is an adapter concern — verify it does not contaminate the query layer.
- **ddd:** "impact" / "blast radius" — is this a new domain concept or just a query view? (Draft position: a *view*, not a concept; no `:Item` attribute, no new label.)
- **solid:** SRP of the new dispatch branch vs. existing `dispatch_*` branches; does `--since` git-seeding belong in `impact` or a shared seed-resolver?
- **rust-systems:** cost of `CALLS*1..` on the densest crate (`cfdb-petgraph`, 578 fns) without a depth bound; whether `--max-depth` should default to bounded.

## 6. Non-goals

- Signature-precise seeding (diff `:Item.signature_hash` across two keyspaces to seed only *structurally* changed items) — deferred; v1 seeds at file granularity.
- Severity/cost weighting beyond the production-reachability flag — no edge weights (the `Understand-Anything` `weight` float is explicitly rejected in `studies/002 §4`).
- Forward "what does X depend on" (downstream) — trivially the same pattern with direction flipped, but out of v1 scope unless an architect folds it in.
- Any rendering, PR comment, or diff visualisation — consumer concern.

## 7. Issue decomposition

### 47-A — Canonical reverse-reachability query + dogfood
Add the parameterised query (or a query-builder helper) and assert it against cfdb-self.
```
Tests:
  - Unit: query-builder produces the expected Query AST for a given seed list.
  - Self dogfood (cfdb on cfdb): seed a known leaf fn in cfdb-core; assert its known callers in cfdb-petgraph/cfdb-cli appear in the affected set.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): none — rationale: no schema change, no ban rule; cross-fixture unaffected.
  - Target dogfood (on qbot-core at pinned SHA): report blast-radius size for one representative changed fn in PR body.
```

### 47-B — `cfdb impact` CLI dispatch branch (file-seeded from `git diff`)
Wire the `--since`/`--item` seed resolution + dispatch composing `query_with_input`.
```
Tests:
  - Unit: --since seed resolution maps a changed-file set to the correct seed qnames (pure, given a fixture graph).
  - Self dogfood (cfdb on cfdb): `cfdb impact --since HEAD~1` on a real cfdb commit returns a non-empty, plausible affected set; a docs-only commit returns empty + a Warning.
  - Cross dogfood: none — rationale: adapter-only, no schema/ban surface.
  - Target dogfood (on qbot-core): run end-to-end against one PR ref; report affected-count in PR body.
```

### 47-C — Production-reachability ranking (optional, may fold into 47-A)
Intersect/annotate the affected set with `reachable_from_production_entry`.
```
Tests:
  - Unit: ordering places production-reachable affected items first.
  - Self dogfood: assert a test-only caller ranks below a production caller for the same seed.
  - Cross dogfood: none — rationale: no schema change.
  - Target dogfood: report the production-reachable fraction of one PR's blast radius.
```
