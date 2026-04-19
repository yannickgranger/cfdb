# Gate 1 — Oxigraph

**Candidate:** Oxigraph
**Query language:** SPARQL 1.1 Query / Update / Federated Query (+ optional RDF 1.2 / SPARQL 1.2 via `rdf-12` cargo feature, which brings RDF-star / quoted triples)
**Version evaluated:** 0.5.6 (released 2026-03-14, current `max_stable_version` on crates.io)
**Docs sources:**
- https://oxigraph.org (redirects to GitHub)
- https://github.com/oxigraph/oxigraph and `lib/oxigraph/README.md`
- https://docs.rs/oxigraph/0.5.6/oxigraph/
- https://docs.rs/oxigraph/0.5.6/oxigraph/store/struct.Store.html
- https://docs.rs/oxigraph/0.5.6/oxigraph/store/struct.BulkLoader.html
- https://docs.rs/oxigraph/0.5.6/oxigraph/store/struct.Transaction.html
- https://docs.rs/oxigraph/0.5.6/oxigraph/sparql/struct.SparqlEvaluator.html
- https://docs.rs/oxigraph/0.5.6/oxigraph/sparql/struct.PreparedSparqlQuery.html
- https://www.w3.org/TR/sparql11-query/#propertypaths
- https://www.w3.org/TR/rdf11-concepts/#section-triples
**Rust binding:** `oxigraph` crate, pure Rust, RocksDB backend by default (in-memory fallback if `rocksdb` default feature disabled)
**Date:** 2026-04-13

## Summary

| Axis | Score | Threshold | Verdict |
|---|---|---|---|
| Features (F1–F9) | 6.5 / 9 | ≥ 7 | FAIL |
| Schema (S1–S7) | 5.5 / 7 | ≥ 6 | FAIL |
| **Gate 1** | | | **DROP** |

## Structural note

Oxigraph is RDF + SPARQL, not property-graph + Cypher. The 9 patterns were designed against property-graph semantics, so Gate 1 scoring for Oxigraph factors the **structural mismatch**. Three rows are structurally hostile:

1. **F2 (variable-length path)** — SPARQL 1.1 property paths have `*`, `+`, `?` but **no bounded repetition** syntax (`{1,5}` / `{3}`). Confirmed against the W3C spec: "There is no syntax for bounded ranges or exact repetition counts in SPARQL 1.1's property path language." This directly collides with the cfdb need for bounded hop counts (e.g. "CALLS at most 5 hops").
2. **F9 (multi-valued repeated edges) and S5 (bag semantics)** — RDF is **set-semantic by definition**: "An RDF graph is a set of RDF triples." Two identical triples collapse. RDF-star / quoted triples can fake bag semantics by attaching a per-occurrence key as an annotation, but this is a reification workaround, not native support.
3. **S1 (typed node labels) and S4 (edge properties)** — RDF has no first-class node labels (you encode them as `?x rdf:type :Item` triples) and no first-class edge properties in classic RDF (you need RDF-star / quoted triples, enabled via the `rdf-12` cargo feature, which itself is flagged "preliminary" for RDF 1.2 / SPARQL 1.2 drafts).

Score accordingly: tagging convention = 0.5, reification convention = 0.5.

## Features

### F1 — Fixed-hop label + property match
**Score:** 1
**On-paper test query:**
```sparql
PREFIX cfdb: <https://cfdb.dev/vocab#>
SELECT ?caller ?callee WHERE {
  ?caller a cfdb:Function ;
          cfdb:visibility "pub" ;
          cfdb:calls ?callee .
  ?callee a cfdb:Function ;
          cfdb:name ?name .
  FILTER(STRSTARTS(?name, "qbot_"))
}
```
**Notes:** Basic graph pattern + FILTER is the bread-and-butter of SPARQL; works directly once the `rdf:type` tagging convention is in place. Full credit for the query shape even though the *data model* (S1) is a 0.5.

### F2 — Variable-length path
**Score:** 0
**On-paper test query:**
```sparql
# What we WANT (bounded to 5 hops) — NOT EXPRESSIBLE:
#   ?a cfdb:calls{1,5} ?b .
# What SPARQL 1.1 allows (unbounded, no hop ceiling):
SELECT ?caller ?callee WHERE {
  ?caller cfdb:calls+ ?callee .
}
```
**Notes:** SPARQL 1.1 property paths only support `*`, `+`, `?` — **no bounded repetition**. For cfdb's pattern H (fallback-path missing) and A (split-brain within N hops) we need a finite hop ceiling so the query terminates in bounded time on 80k-edge graphs. Unbounded `+` over a per-developer embedded store with no query optimization ("SPARQL query evaluation has not been optimized yet", Oxigraph README) is a performance cliff, not a feature. Expressing bounded hops requires hand-unrolling the pattern into N UNION clauses (`{?a :p ?b} UNION {?a :p/:p ?b} UNION ...`) which is a code-generation workaround and doesn't generalize. Score 0.

### F3 — Property regex in WHERE
**Score:** 1
**On-paper test query:**
```sparql
SELECT ?fn ?name WHERE {
  ?fn a cfdb:Function ; cfdb:name ?name .
  FILTER regex(?name, "^should_not_", "i")
}
```
**Notes:** `FILTER regex(?x, "pattern", "flags")` is in SPARQL 1.1 core. Direct hit.

### F4 — OPTIONAL MATCH
**Score:** 1
**On-paper test query:**
```sparql
SELECT ?fn ?doc WHERE {
  ?fn a cfdb:Function ; cfdb:name ?name .
  OPTIONAL { ?fn cfdb:hasDocComment ?doc }
}
```
**Notes:** `OPTIONAL { }` is native SPARQL 1.1. Direct hit.

### F5 — External parameter sets
**Score:** 1
**On-paper test query:**
```sparql
SELECT ?item WHERE {
  VALUES ?forbidden { "panic!" "todo!" "unimplemented!" "unwrap" }
  ?item a cfdb:Function ; cfdb:calls/cfdb:name ?forbidden .
}
```
**Notes:** `VALUES` clause is SPARQL 1.1 native. Direct hit.

### F6 — NOT EXISTS
**Score:** 1
**On-paper test query:**
```sparql
SELECT ?fn WHERE {
  ?fn a cfdb:MoneyPathFn .
  FILTER NOT EXISTS { ?fn cfdb:usesType cfdb:Decimal }
}
```
**Notes:** `FILTER NOT EXISTS { }` is SPARQL 1.1 native. Direct hit.

### F7 — Aggregation + grouping
**Score:** 1
**On-paper test query:**
```sparql
SELECT ?module (COUNT(?fn) AS ?n) WHERE {
  ?fn a cfdb:Function ; cfdb:definedIn ?module .
} GROUP BY ?module HAVING (COUNT(?fn) > 10)
```
**Notes:** `COUNT` / `GROUP BY` / `HAVING` / `SUM` / `MIN` / `MAX` / `AVG` / `GROUP_CONCAT` / `SAMPLE` are SPARQL 1.1 native. Direct hit.

### F8 — Parameterized queries
**Score:** 1
**On-paper test query:**
```rust
let prepared = SparqlEvaluator::new()
    .parse_query("SELECT ?callee WHERE { ?caller cfdb:calls ?callee }")?
    .substitute_variable(Variable::new("caller")?, NamedNode::new("cfdb:foo")?);
let results = prepared.on_store(&store).execute()?;
```
**Notes:** Confirmed via `PreparedSparqlQuery::substitute_variable(variable, term) -> Self` (docs.rs/oxigraph/0.5.6). Takes a `Variable` and an RDF term, returns `Self` for chaining. This is **true typed parameter binding**, not string concatenation — injection-safe, parse-once. Direct hit and one of Oxigraph's strongest points.

### F9 — Multi-valued repeated edges
**Score:** 0.5
**On-paper test query:**
```sparql
# Classic RDF: IMPOSSIBLE (set semantics collapses duplicates).
# RDF-star workaround (requires rdf-12 cargo feature):
SELECT ?caller ?callee (COUNT(?ann) AS ?count) WHERE {
  << ?caller cfdb:calls ?callee >> cfdb:callSite ?ann .
} GROUP BY ?caller ?callee
```
**Notes:** Core RDF is set-semantic — two identical `(:f, :calls, :g)` triples collapse to one. The only way to preserve multiplicity is RDF-star quoted-triple reification: attach a distinct annotation (line number, call site) to each occurrence, then aggregate over annotations. This (a) requires the `rdf-12` feature flag, which the Oxigraph README describes as "preliminary support for 1.2 RDF and SPARQL drafts", (b) changes the data shape — every edge becomes a quoted triple + annotation — which increases storage and forces every query to deal with quoted triples, and (c) is a convention, not native multi-edge support. Score 0.5, and consider this the hardest row: if the RDF 1.2 draft churns, the schema churns.

## Schema requirements

### S1 — Typed node labels
**Score:** 0.5
**Evidence:** RDF has no first-class node labels. The convention is `?x rdf:type :Function`, which is a regular triple. All queries must add `?x a :Function` clauses to filter by type, and the type is carried in the triple store like any other edge. Per the structural-note scoring rule above, tagging convention = 0.5, not 1. Source: W3C RDF 1.1 Concepts, §3 "RDF graphs are sets of triples"; no dedicated label primitive exists.

### S2 — Typed edge labels
**Score:** 1
**Evidence:** RDF predicates *are* edge labels and are native. `?a cfdb:calls ?b` has `cfdb:calls` as a first-class typed edge. This is the one row where RDF maps cleanly to the cfdb need. Direct hit.

### S3 — Node properties
**Score:** 1
**Evidence:** RDF literals with XSD datatypes (`xsd:string`, `xsd:integer`, `xsd:boolean`, `xsd:dateTime`, `xsd:decimal`) are native. `?fn cfdb:loc "142"^^xsd:integer` stores a typed integer literal. SPARQL 1.1 filter functions (`xsd:integer(?x)`, arithmetic on typed literals) work natively. Direct hit.

### S4 — Edge properties
**Score:** 0.5
**Evidence:** Classic RDF cannot attach a property to a triple — a triple has exactly 3 slots (s, p, o). RDF-star / quoted triples (RDF 1.2) fixes this: `<< ?a cfdb:calls ?b >> cfdb:lineNumber 42`. Oxigraph's README states RDF 1.2 / SPARQL 1.2 support is behind the `rdf-12` cargo feature and is "preliminary". Available, but (a) behind a draft-status feature flag, (b) reification-shaped — every edge-with-properties costs more storage and more pattern complexity than a property graph, (c) every query that touches edge properties must use `<< >>` quoted-triple syntax. Workaround, not direct. Score 0.5.

### S5 — Multi-valued repeated edges (bag semantics)
**Score:** 0.5
**Evidence:** Same story as F9 and S4 combined. Classic RDF collapses duplicates (set semantics, W3C RDF 1.1 Concepts §3). RDF-star can fake it by attaching a disambiguating annotation per occurrence (call-site ID, line number). This is the standard reification trick and it works, but it changes the schema: every call edge needs a unique annotation key or two `f → g` calls on different lines look identical. Again gated on `rdf-12` preliminary feature. 0.5.

### S6 — Bulk insert
**Score:** 1
**Evidence:** `Store::bulk_loader() -> BulkLoader` returns a dedicated bulk-load API with `parallel_load_from_file`, `parallel_load_from_slice`, `with_num_threads`, `with_max_memory_size_in_megabytes`. Docs state "default memory consumption targets approximately 2GB per thread" and "default thread count matches the machine's available processors; parallel loading available for N-Triples and N-Quads formats." Accepts Turtle, N-Triples, N-Quads, TriG, RDF/XML via `RdfParser`. Direct hit — this is a first-class feature, explicitly documented, with tuning knobs. Source: https://docs.rs/oxigraph/0.5.6/oxigraph/store/struct.BulkLoader.html

### S7 — Read-only mode / snapshot isolation
**Score:** 1
**Evidence:** `Store::open_read_only(path)` is documented: "Opens a read-only Store from disk." Additionally `Store::start_transaction()` provides "repeatable read" isolation per the docs. Caveat from the docs on `open_read_only`: "Opening while another process writes is undefined behavior" — for the cfdb per-developer-embedded deployment this is fine (extract phase finishes, then the query phase opens read-only). Direct hit. Source: https://docs.rs/oxigraph/0.5.6/oxigraph/store/struct.Store.html

## Detailed notes

- **SPARQL 1.1 coverage.** Oxigraph README: "SPARQL 1.1 Query, SPARQL 1.1 Update, and SPARQL 1.1 Federated Query" with "nearly fully conformant" compliance. All the core algebra (BGP, OPTIONAL, UNION, MINUS, FILTER NOT EXISTS, VALUES, GROUP BY, aggregates, property paths, subqueries) is supported. Strong caveat from the same README: **"SPARQL query evaluation has not been optimized yet"** — for a 15k-node / 80k-edge workload with unbounded `+` property paths, this is a real runtime risk worth spike-testing before trusting.
- **RDF-star / quoted triples status.** Not in Oxigraph's default build. Enabled by the `rdf-12` cargo feature, which the README describes as "preliminary support for 1.2 RDF and SPARQL drafts." SPARQL-star / RDF-star are **not** independently listed as supported specs — they come in via the RDF 1.2 / SPARQL 1.2 bundle only. This means the three rows that depend on it (F9, S4, S5) ride on a preliminary, draft-tracking feature flag. For a cfdb production decision this is a meaningful maturity risk.
- **Rust API parameterization.** `SparqlEvaluator::parse_query(&str) -> PreparedSparqlQuery`, then `PreparedSparqlQuery::substitute_variable(Variable, Term) -> Self`, then `.on_store(&store).execute()`. This is **true typed parameter binding** — you build a Rust `NamedNode`, `Literal`, or `BlankNode`, hand it to the prepared query, and it's substituted at execute time. Not string concatenation. Injection-safe. F8 scores 1 with confidence.
- **Bulk-load API and claims.** `BulkLoader` supports parallel loading of N-Triples and N-Quads, ~2 GB memory per thread default, configurable thread count, `commit()` finalizer. Sequential `load_from_reader` / `load_from_slice` also available. First-class story, well documented.
- **Read-only mode.** `Store::open_read_only(path)` exists. Transactional `start_transaction()` gives repeatable-read isolation. For per-developer-embedded cfdb this covers S7 cleanly.
- **FLAG: the F9 / S4 / S5 triple of rows is structurally hostile to RDF.** These three rows are all variations of the same structural issue: RDF is a set of triples, not a multigraph with edge properties. RDF-star patches it, but RDF-star is behind the `rdf-12` preliminary feature flag in Oxigraph 0.5.6. If the cfdb workload actually needs to distinguish three separate `f → g` call edges by line number, you're buying into (a) the draft RDF 1.2 spec, (b) Oxigraph's preliminary support of it, and (c) reification-shaped queries forever. This is the core reason Oxigraph does not make the gate.
- **FLAG: F2 is the killer.** Even setting aside the RDF-star bet, F2 fails outright. SPARQL 1.1 property paths have no bounded-hop operator. The fallback is either unbounded `+` (performance risk given Oxigraph's explicit "not optimized yet" caveat) or hand-unrolling into N UNIONs (code-generation workaround, doesn't generalize, blows up query size for N > 5). Cfdb patterns A and H both need bounded-hop reachability. This is a direct 0.

## Verdict

**DROP.** Oxigraph scores **6.5 / 9 on features** (fails the ≥7 threshold, failing row is F2 — no bounded property paths in SPARQL 1.1) and **5.5 / 7 on schema** (fails the ≥6 threshold, failing rows are S1 tagging-convention typing, S4 edge-properties-via-RDF-star, and S5 bag-semantics-via-RDF-star). Both axes fail. Three of the failing rows (F9, S4, S5) compound on the same draft `rdf-12` cargo feature, which means even the 0.5 partial credit rides on a preliminary spec. F2 is the cleanest kill: SPARQL 1.1 property paths simply do not have bounded repetition, and cfdb needs bounded-hop reachability for patterns A and H. Oxigraph has real strengths — F8 typed parameter binding via `substitute_variable` is best-in-class, S6 bulk loader is first-class, S7 read-only mode is native, and S2/S3 map cleanly — but the structural mismatch between property-graph query patterns and RDF's set-of-triples model cannot be papered over for this workload. Do not advance to the integration spike.
