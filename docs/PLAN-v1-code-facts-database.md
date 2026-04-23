# Code Facts Database — v1 prospective plan

**Status:** historical — substrate for `docs/RFC-cfdb.md` (ratified, v0.1 shipped). Retained for context on the problems cfdb was built to solve.
**Date:** 2026-04-13 (trimmed 2026-04-22 — removed prior-prototype migration detail and obsolete infrastructure specifics that were resolved by v0.1).
**Superseded by:** `docs/RFC-cfdb.md` (what shipped) and `docs/PLAN-v2-solving-original-problems.md` (what remains).

---

## 0. Problem

### 0.1 The retrofit constraint

A mature Rust workspace **already contains the drift** the tool must characterize — a year of agentic-session accumulation across bounded contexts, thousands of `pub` items. This is not a greenfield exercise where patterns are fixed at commit #1 and enforced forward. Any tool must **characterize what exists first**, then help fix it. Prevention of future drift is a secondary benefit.

This eliminates the class of "write the rules up front, enforce in CI" solutions that work on a new project. Archaeology tooling is needed, not just gatekeeping.

### 0.2 The demand surface

The consumers of a fact base are the skills, agents, and humans asking structural questions about the codebase. They split into four classes:

| Class | Examples | Latency | Freshness | Output shape |
|---|---|---|---|---|
| **Generation-time grounding** | "does concept X already exist?" asked per issue, per refactor | sub-second per query | current branch | small JSON payload |
| **Audit-time detection** | duplicate clusters, layer violations, dead code | seconds per query | HEAD or last nightly | markdown report |
| **Refactor glue** | "what moves with this item?" | interactive | HEAD | structured target list |
| **Plan validation** | "is this raid plan complete and consistent?" | seconds per query | HEAD + plan input | pass/fail + holes list |

All four classes consume the same fact base via the same API (§6). They differ only in which Cypher they compose and which wire form they use. Consumers do not extend the API — they compose against it.

**The polyvalence claim is: one orthogonal API surface, consumed by N consumers via different compositions, with no per-consumer tool features.** If a consumer wants something the API cannot express, the answer is to extend the *schema* (§5), not to add a verb.

### 0.3 The detect → refactor loop

Detection without a refactor path is a demoralizing backlog. Refactor without detection is drift-by-omission. The loop:

1. **Detect** — a query finds drift (HSB, VSB, dead code, layer violation)
2. **Rank** — by blast radius, age, or consumer-specified weight
3. **Plan** — turn top-ranked drift into a migration blueprint
4. **Validate** — cross-check the blueprint against the fact base *before* execution
5. **Execute** — move-not-copy archaeology guided by the validated plan
6. **Re-audit** — re-extract, re-query, fixed drift drops out of the backlog

Step 4 is the highest-leverage step: a bad raid wrecks a sprint; a good raid depends on knowing every hidden caller *before* the first file moves.

### 0.4 The crux

The design question is not *"Glean-shaped platform or symbol-server-shaped index?"* — a false choice between two solution shapes. It is:

> **What is the minimum fact schema that serves the queries the four consumer classes actually need, at the right latency and freshness per class, on a codebase that already contains the drift we want to characterize?**

If the answer is "a schema smaller than §5," we underbuild. If it's "larger than §5," we extend. The schema is a first cut; the consumer enumeration is what drives it over time.

---

## 1. TL;DR

Replace ad-hoc audit scripts and prior LLM-extracted concept maps with a **deterministic code facts database** that answers two distinct problems from the same fact base:

1. **Horizontal split-brain (HSB)** — concept duplication across crates at the same architectural layer.
2. **Vertical split-brain (VSB)** — parameter provenance loss along the call chain from entry points down to the engine.

Both are graph queries over the same underlying graph of code facts. They differ only in which node labels and edge types they traverse. Separating **structural fact extraction** from **analysis** produces a polyvalent platform that answers the next eight questions for free (§7 lists 27).

The pattern is the local equivalent of **Glean** (Meta) / **CodeQL** (GitHub): one deterministic extractor produces typed facts; an open-ended library of Cypher queries answers questions over those facts; enrichments (LLM descriptions, embeddings, git history) layer on top without touching the structural fact base.

---

## 2. Problems on the table

### 2.1 Horizontal split-brain (HSB)

**Pattern:** the same business concept is implemented in multiple crates at the same architectural layer — sometimes under the same name, sometimes under synonyms, sometimes with subtle structural variations.

**Failure mode without detection:** agentic sessions create parallel abstractions because they don't see what already exists. After 50–100 issues, the codebase has 3 parallel systems doing the same thing under different names. Grep is structurally blind to synonyms.

**Multi-signal detector (§7 row 3):** structural hash collision + neighbor-set Jaccard + normalized name match + conversion-target sharing. A pair flagged by ≥2 signals is a candidate. Catches synonym-renamed duplicates that name-match cannot see, because two of the four signals don't look at names at all.

### 2.2 Vertical split-brain (VSB)

**Pattern:** a value enters at a top-level entry point (MCP tool, CLI command, HTTP route, cron job) and gets *re-resolved, re-defaulted, or transformed* at multiple layers on its way down to the engine. Each layer has its own fork of the resolution logic. When a bug surfaces, the coder finds and fixes one fork; the other forks still run; the bug persists end-to-end because the fix happened on the wrong branch.

**Concrete failure pattern:**

1. User calls an MCP tool with `tf=1h`.
2. Handler parses → `Timeframe::H1` (resolver v1).
3. Handler calls a use case passing the *string*, not the parsed type.
4. The use case re-parses with its own logic → `Timeframe::Hour1` (resolver v2).
5. The use case calls a port, which has yet another defaulting step for missing values.
6. Engine receives whatever the deepest resolver produced.
7. Coder fixes v1 site. CI passes. **Bug persists in production** because v2 is the one that wins.

This is the failure mode `Param-Effect Canary` and `MCP Boundary Fix AC` guardrails exist to prevent — but both are runtime-asserted. Neither is structurally enforced. VSB is invisible to every static check.

**Detector (§7 row 4):** for each `:EntryPoint`, BFS over `CALLS*` edges from the registered handler. For each parameter declared on the entry point, find every `:Item{kind:Fn}` reachable from the handler whose `RETURNS` type matches the param's conceptual type. Count > 1 ⇒ vertical split-brain. Output the call chain with each resolver annotated.

### 2.3 Bounded-context raid (plan validation)

**Pattern:** architects discover that a module has accumulated enough drift that incremental fixes are no longer viable. The right move is a raid — produce a clean blueprint, portage the clean parts as-is, rewrite the glue, drop the dirty parts. The failure mode is the blueprint misses a hidden caller, a dangling type reference, or a forgotten internal dependency, and the raid ships broken or leaves orphans.

**What a validated plan catches:**

1. **Incompleteness** — unnamed items in the source context.
2. **Dangling references** — "drop" items still referenced from "portage"+"glue".
3. **Hidden callers** — "portage" items called from outside the source context.
4. **Missing canonicals** — "rewrite" concepts with no named target carrying `CANONICAL_FOR`.
5. **Clean/dirty signal mismatch** — "portage (clean)" items where quality signals (unwrap count, test coverage, duplicate-cluster membership) disagree with the plan.

**The skill:** a `/gate-raid-plan` consumer takes a raid plan (YAML) + the fact base. Produces pass/fail + a list of plan holes. Runs *before* the raid starts as a go/no-go gate, and *again after each phase* to catch drift as files move.

**Why this is the highest-leverage consumer:** other classes use the fact base for *lookup* or *detection*. The raid consumer uses it for **decision support** — joining structural facts with quality signals to answer "is this plan safe to execute?" That's the first query in §7 genuinely not doable by eye + grep.

**Schema implication:** quality metrics (unwrap count, test coverage, duplicate-cluster membership, cyclomatic complexity) must live *on* `:Item` nodes as attributes, not in a parallel report. Otherwise the raid skill has to join across two data stores, which defeats the "one fact base" premise.

### 2.4 What unifies them

All three are *graph queries over code facts*. HSB is a structural-similarity query over `:Item` nodes. VSB is a reachability-and-multiplicity query over `:CallSite`/`:CALLS` edges between `:EntryPoint` and `:Item{kind:Fn}` nodes. The raid is a decision-support query joining structural facts with quality signals against a declared plan. Same underlying fact base; different edges and attributes exercised.

---

## 3. The insight — extract once, query N ways

This is how every serious code analysis platform works:

| Platform | Extraction layer | Query layer |
|---|---|---|
| **Glean** | typed facts via per-language indexers | Angle (Datalog-flavored) |
| **CodeQL** | per-language extractor → relational DB | object-oriented QL |
| **Soufflé** | tree-sitter or custom extractors | Datalog |
| **Kythe** | per-language indexers → graph | GraphQL-like |
| **rust-analyzer** | semantic database from incremental compilation | LSP queries |

Every one of them separates **structural fact extraction** (deterministic, expensive, run on schedule or on commit) from **analysis** (cheap, query-based, run on demand).

**The polyvalent move is not a clever query — it's a schema rich enough that clever queries are short.**

---

## 4. Architecture — four layers

```
┌───────────────────────────────────────────────────────────────┐
│ Layer 4 — Formatters                                          │
│   markdown · JSON · terminal table · dot graph                │
└───────────────────────────────────────────────────────────────┘
                              ▲
┌───────────────────────────────────────────────────────────────┐
│ Layer 3 — Query library                                       │
│   one .cypher per analysis · exposed via CLI / HTTP / Rust lib│
└───────────────────────────────────────────────────────────────┘
                              ▲ (Cypher over the graph store)
┌───────────────────────────────────────────────────────────────┐
│ Layer 2 — Optional enrichments (additive, never structural)   │
│   git blame → LAST_TOUCHED_BY · LLM descriptions for          │
│   undocumented items · embeddings · concept synonym table     │
└───────────────────────────────────────────────────────────────┘
                              ▲
┌───────────────────────────────────────────────────────────────┐
│ Layer 1 — Extractor (deterministic, content-addressable)      │
│   Rust binary · parses every file · emits typed facts         │
│   writes to graph store · same content → same graph, byte-    │
│   for-byte                                                    │
└───────────────────────────────────────────────────────────────┘
                              ▲
                     Rust source files
```

**The only LLM in the path is in Layer 2, and only as enrichment for undocumented items.** Layer 1 is fully deterministic and re-runnable on every commit. Drift between commits becomes detectable because the graph is reproducible.

**Each layer has an interface, not a coupling.** Layer 1 emits a fact schema (§5). Layer 2 reads facts and adds attributes/edges. Layer 3 reads anything. Layer 4 doesn't touch the graph. New analyses live entirely in Layer 3. New extraction targets (e.g. "Python files too") live entirely in Layer 1 and don't disturb anything else.

---

## 5. Fact schema (the contract Layer 1 must produce)

### 5.1 Nodes

| Label | Created by | Key fields | Meaning |
|---|---|---|---|
| `:Crate` | every Cargo package in workspace | `name`, `version`, `path` | a Cargo crate |
| `:Module` | every `mod` block (file or inline) | `qpath`, `crate`, `file`, `is_inline` | a Rust module |
| `:File` | every `.rs` file scanned | `path`, `crate`, `module_qpath`, `loc` | a source file |
| `:Item` | every `pub`/`pub(crate)` item | `qname`, `name`, `kind`, `crate`, `module_qpath`, `file`, `line`, `signature_hash`, `doc_text` | a struct/enum/trait/fn/impl/type/const/static |
| `:Field` | every struct field, tuple element | `parent_qname`, `name`, `index`, `type_path`, `type_normalized` | a struct field |
| `:Variant` | every enum variant | `parent_qname`, `name`, `index`, `payload_kind` | an enum variant |
| `:Param` | every fn/method parameter | `parent_qname`, `name`, `index`, `type_path`, `type_normalized`, `is_self` | a function parameter |
| `:CallSite` | every concrete call expression | `caller_qname`, `callee_qname` (best effort), `file`, `line`, `arg_count` | a call site (caller → callee) |
| `:EntryPoint` | every MCP tool / CLI command / HTTP route / cron registration | `kind` (`mcp`/`cli`/`http`/`cron`), `name`, `handler_qname`, `params` | a top-level entry into the system |
| `:Concept` | overlay layer — labels Items semantically | `name`, `assigned_by` (`doc`/`rule`/`llm`/`manual`) | a semantic label |

### 5.2 Edges

**Structural (always present):**

| Type | From | To | Meaning |
|---|---|---|---|
| `IN_CRATE` | any node with a crate | `:Crate` | ownership |
| `IN_MODULE` | `:Item`, `:File` | `:Module` | module containment |
| `IN_FILE` | `:Item` | `:File` | source location |
| `HAS_FIELD` | `:Item` (struct) | `:Field` | composition |
| `HAS_VARIANT` | `:Item` (enum) | `:Variant` | enumeration |
| `HAS_PARAM` | `:Item` (fn) | `:Param` | signature |
| `TYPE_OF` | `:Field`, `:Param`, `:Variant` payload | `:Item` (type) | type reference |
| `IMPLEMENTS` | `:Item` (impl) | `:Item` (trait) | trait implementation |
| `IMPLEMENTS_FOR` | `:Item` (impl) | `:Item` (type) | impl target |
| `RETURNS` | `:Item` (fn) | `:Item` (return type) | function return |
| `EXTENDS` / `SUPERTRAIT` | `:Item` (trait) | `:Item` (trait) | trait inheritance |

**Call graph (the addition that enables VSB):**

| Type | From | To | Meaning |
|---|---|---|---|
| `CALLS` | `:Item` (fn) | `:Item` (fn) | static call edge (best effort cross-crate) |
| `INVOKES_AT` | `:CallSite` | `:Item` (fn) | concrete invocation point |
| `RECEIVES_ARG` | `:CallSite` | `:Param` | which callee param this call binds |

**Entry-point graph:**

| Type | From | To | Meaning |
|---|---|---|---|
| `EXPOSES` | `:EntryPoint` | `:Item` (handler fn) | entry point dispatches to this fn |
| `REGISTERS_PARAM` | `:EntryPoint` | `:Param` | a registered tool param |

**Concept overlay:**

| Type | From | To | Meaning |
|---|---|---|---|
| `LABELED_AS` | `:Item` | `:Concept` | item carries this semantic label |
| `CANONICAL_FOR` | `:Item` | `:Concept` | designated authoritative impl |
| `EQUIVALENT_TO` | `:Concept` | `:Concept` | synonym (e.g. `TradeSide ≡ Direction`) |

**History (Layer 2, optional):**

| Type | From | To | Meaning |
|---|---|---|---|
| `INTRODUCED_IN` | `:Item` | `:Commit` | first commit that defined this item |
| `LAST_TOUCHED_BY` | `:Item` | `:Commit` | most recent commit touching this item |

### 5.3 Item kinds

`Struct` · `Enum` · `Trait` · `Impl` · `Fn` · `Const` · `TypeAlias`

`Service` and `Port` are **not** primitive kinds; they are concept labels assigned via `LABELED_AS` (Layer 2 rule: "any unit struct in `domain-*::services::` is labeled `Concept{name:Service, assigned_by:rule}`"). Kinds are structural facts (what the parser saw); labels are an overlay (what we infer).

### 5.4 Type normalization rules

Type normalization is load-bearing. Aggressive normalization causes false-positive duplicates; conservative causes false-negative misses. The v1 default rules:

1. Strip lifetimes (`'a` → ∅).
2. Strip `&`, `&mut`, `*const`, `*mut` outer wrappers.
3. Strip `Box<T>`, `Arc<T>`, `Rc<T>`, `Cow<'_, T>` for structural-hash purposes; keep the inner type.
4. Normalize `Option<T>` to `T?` (preserve nullability as a flag, not a wrapper).
5. Normalize `Vec<T>` and `[T; N]` to `T*` (preserve multiplicity as a flag).
6. Normalize `Result<T, E>` to `T!E` (preserve fallibility as a flag).
7. Generic parameter names erased; only arity preserved (`Vec<T>` and `Vec<U>` equivalent).
8. Fully-qualified path resolution where possible; unresolved paths preserved as-is.

These rules are recorded alongside the extractor. Every change is a schema version bump.

---

## 6. Tool API surface

The tool exposes a **small, orthogonal API** that consumers compose against — not a bag of named verbs per consumer. Every question a consumer might want to ask is a Cypher composition over §5's schema, not a new verb. **Keep the verb count under 20.** Every named verb is a long-term commitment.

### 6.1 Verb surface (11 verbs)

```
INGEST  (write side)

  extract(workspace_path, target_keyspace) -> ExtractReport
    Walks `cargo metadata` + parses every .rs file.
    Writes §5.1 nodes and §5.2 structural + entry-point edges.
    Deterministic: same workspace SHA -> byte-identical graph.
    Idempotent: re-running on the same SHA is a no-op after content-hash dedup.

  enrich_docs(keyspace)             -> EnrichReport   // doc_text from /// + LLM fallback
  enrich_metrics(keyspace)          -> EnrichReport   // unwrap_count, complexity, coverage
  enrich_history(keyspace, repo)    -> EnrichReport   // INTRODUCED_IN, LAST_TOUCHED_BY
  enrich_concepts(keyspace, rules)  -> EnrichReport   // LABELED_AS overlay (rules + LLM)


QUERY  (read side — every consumer goes through these two verbs)

  query(keyspace, cypher, params)                  -> JSON rows
  query_with_input(keyspace, cypher, params, sets) -> JSON rows
    Cypher is the composition language. The API does not invent its own DSL.
    `sets` are named external identifier lists (e.g. plan.yaml's portage/drop
    buckets) passed in as parameter tables. This is how external data joins
    against the graph without being persisted into it.


SNAPSHOT  (graph addressing)

  list_snapshots()                           -> [(keyspace, sha, ts, schema_v)]
  diff(keyspace_a, keyspace_b, entity_kinds) -> {added, removed, changed}
  drop(keyspace)                             -> ()
    Keyspace naming convention: cfdb_<repo>_<sha-prefix>.
    Drift queries are diff() over two keyspaces; nothing else is needed.


SCHEMA  (introspection — used by tests and clients)

  schema_version(keyspace) -> SemVer
  schema_describe()        -> JSON   // nodes, edges, attributes, version
```

**11 verbs total.** Adding a 12th verb requires council approval and an API version bump.

### 6.2 Wire forms

| Form | Use case | Latency model |
|---|---|---|
| **CLI** (`cfdb query …`) | humans, shell scripts, ad-hoc audits | per-invocation cold start |
| **HTTP** (`POST /v1/query`) | external consumers, latency-sensitive callers | warm process, sub-second per query |
| **Rust lib** (`use cfdb::query;`) | tests, Rust-native consumers, in-process composition | function call |

Consumers pick the form that fits their latency and integration model. Adding a fourth form (gRPC, stdio JSON-RPC, etc.) is straightforward because the verbs are the contract — not any wire format.

### 6.3 Determinism guarantees (the contract)

```
G1. Same workspace SHA + same schema version  ->  byte-identical graph.
G2. query() is read-only. No query mutates the graph.
G3. enrich_*() is additive. No enrichment deletes structural facts.
G4. schema_version() is monotonic within a major: v1.1 graphs are
    queryable by v1.0 consumers (additive-only changes within a major).
G5. Snapshots are immutable. Once a keyspace is written, it is never
    rewritten in place — only dropped or replaced wholesale.
```

Without G1, drift queries are meaningless. Without G2, queries are unsafe to share between consumers. Without G3, re-running enrichments is destructive. Without G4, every consumer breaks on every schema change. Without G5, queries are racy under concurrent ingest.

### 6.4 What is explicitly NOT in the API

- **Named queries per use case** (`vsb()`, `raid_completeness()`, `prescribe_canonicals()`). Those are Cypher files in §7, consumed via `query()`.
- **Output formatting** (markdown, dot, PR comments, terminal tables). `query()` returns JSON rows; formatting is the consumer's job.
- **Skill integration adapters.** Each consuming skill wraps `query()` in its own glue. cfdb ships no skill bindings.
- **Refactoring actions.** Read-only at the source level; never rewrites Rust files.
- **Multi-language support in v1.** Rust-only. Adding a language means a new extractor targeting the same schema; the API is unchanged.
- **Opinionated workflows.** cfdb does not know what a "raid" is or what `/prescribe` wants.
- **Caching.** Consumers cache in their own caches. cfdb is a stateless query server over an immutable snapshot.

### 6.5 How consumers compose against the API

| Consumer class | Wire form | Verbs used | External inputs |
|---|---|---|---|
| Generation-time grounding | HTTP | `query` | params only (concept name, scope) |
| Audit-time detection | CLI | `query`, `diff` | none |
| Refactor glue | CLI or Rust lib | `query` | optional: target qnames |
| Plan validation | HTTP | `query_with_input` | plan.yaml's bucket sets |

If a consumer needs something the 11 verbs cannot express via Cypher composition, that is a signal to **extend the schema (§5)** — not the API.

---

## 7. Example compositions catalog

**Not API surface — example Cypher compositions.** Each row is one Cypher file consumers copy, modify, or replace. **Adding a row does not extend the API. Removing a row does not break the API.**

| # | Analysis | Query shape |
|---|---|---|
| 1 | HSB by name | `MATCH (a:Item),(b:Item) WHERE a.name=b.name AND a.crate<>b.crate` |
| 2 | HSB by structural hash | same with `a.signature_hash = b.signature_hash` |
| 3 | HSB clusters (multi-signal) | union of name + structural + neighbor-jaccard + conv-target signals, ≥2 signals = candidate |
| 4 | **VSB / multi-resolver** | for each `:EntryPoint`, BFS `CALLS*`, find `:Item{kind:Fn}` whose `RETURNS` matches an entry param type. Count > 1 ⇒ VSB |
| 5 | **Param provenance trace** | for an entry point + param, return full call subgraph with type transformations annotated |
| 6 | Param drop detection | `:CallSite` where caller has param P but callee has no `Param` of compatible type ⇒ candidate drop |
| 7 | Default duplication | `:Item{kind:Fn}` whose body contains literal X for param of concept T, where ≥ 2 such fns exist along one call chain |
| 8 | Bounded-context surface map | `MATCH (i:Item)-[:IN_CRATE]->(:Crate{name:$c}) RETURN i, fan_in` grouped by kind |
| 9 | Refactor blast radius | reverse-`CALLS*` ∪ reverse-`TYPE_OF*` from a target qname |
| 10 | Dead code / orphans | `:Item` with zero incoming `CALLS`, `TYPE_OF`, `IMPLEMENTS_FOR` |
| 11 | Hexagonal violation | `:Item{crate=~'domain-.*'}-[:TYPE_OF]->:Item{crate=~'adapters-.*'}` |
| 12 | Decorator bypass | `:EntryPoint -[:EXPOSES]-> :Item -[:CALLS*1..2]-> :Item{kind:Impl}` skipping known decorator chain |
| 13 | Drift between commits | diff two graph snapshots; new HSB/VSB, removed canonical, changed signatures |
| 14 | Concept ownership | `:Concept` reachable only from items in one crate ⇒ that crate owns it |
| 15 | Test-coverage gap | `:Concept` with no `LABELED_AS` from any item under `tests/` or `#[cfg(test)]` |
| 16 | Param-canary coverage | for each `:EntryPoint` param, exists path to a test item that mentions both the param name and the engine's resolved type? |
| 17 | High-fan-in extraction candidate | `:Item` with ≥ N incoming `TYPE_OF` from distinct crates |
| 18 | Cyclomatic hot spots | `:Item{kind:Fn}` with high branch count |
| 19 | Trait coherence | `:Item{kind:Trait}` whose `IMPLEMENTS_FOR` targets span ≥ N crates |
| 20 | Stale code | `:Item` with `LAST_TOUCHED_BY` older than threshold AND zero incoming refs |
| 21 | **Raid completeness** | for source crate $C, list every `:Item{crate:$C}` not named in plan's portage/rewrite/glue/drop sets |
| 22 | **Raid dangling-drop** | for each "drop" Item in plan, find incoming `CALLS`/`TYPE_OF` from any "portage"+"glue" Item |
| 23 | **Raid hidden-callers** | for each "portage" Item, find incoming edges from `:Item` outside source crate |
| 24 | **Raid missing-canonical** | for each "rewrite" concept in plan, check plan names a target `:Item` with `CANONICAL_FOR` |
| 25 | **Raid signal-mismatch** | join "portage (clean)" Items with quality attributes (unwrap, coverage, dup-cluster); flag where plan and signals disagree |
| 26 | **Canonical lookup** | for concept C, return `:Item` nodes with `CANONICAL_FOR` C ⇒ `/prescribe` grounding payload |
| 27 | **Scope neighborhood** | for an issue's file set, return `:Item` in same modules + their `LABELED_AS` concepts |

The two original problems are rows 3 and 4. The bounded-context raid (§2.3) is rows 21–25. Generation-time grounding is rows 26–27. The other 18 are *the same tool* with different `.cypher` files. **That's the polyvalence.**

---

## 8. Options considered

| Option | Approach | Verdict |
|---|---|---|
| **A. Code facts database (this proposal)** | Deterministic Rust extractor + graph store + query library | ✅ recommended |
| **B. Adopt CodeQL** | GitHub's CodeQL for Rust | ❌ Rust support is preview; vendor-locked; doesn't know MCP entry points or domain concepts; not local-first |
| **C. Adopt Glean** | Meta's open-source Glean | ❌ massive infra for a single-project use case |
| **D. Use rust-analyzer's database directly** | Query `ra`'s on-disk index | ⚠️ defer; revisit if `syn` proves insufficient for cross-crate call resolution |
| **E. Datalog (Soufflé / Crepe)** | Express analyses as Datalog rules over facts | ⚠️ defer; Cypher first |

**Fallback if A blocks:** if `syn`-based cross-crate call resolution proves intractable, escalate to using `ra-ap-*` crates as the extractor frontend. The schema and Layer 3 query library are unchanged — only Layer 1's implementation swaps. Valid escape hatch precisely because the layers have interfaces, not couplings.

---

## 9. Risks

1. **Cross-crate call resolution with `syn` alone is hard.** `syn` is a single-file parser. Resolving "this `foo()` call refers to `domain_strategy::resolver::foo`" requires a workspace-wide symbol table built from `use` statements + qualified paths. **Mitigation:** start with intra-crate resolution; emit unresolved cross-crate calls as edges to `:Symbol` placeholders. Cross-crate resolution can land late without blocking other queries.

2. **Macro-defined items are invisible to `syn`.** `define_id!`, `derive`, and any project-internal item-emitting macro produce items the AST visitor never sees. **Mitigation:** explicit special-case detection for known macros; audit list via `rg 'macro_rules!.*pub (struct|enum)'`. Long-term: invoke `cargo expand` per crate and parse the expanded output.

3. **VSB detection precision depends on type normalization.** "Same conceptual type" needs `Timeframe`, `&Timeframe`, `Option<Timeframe>`, `Result<Timeframe>` to all match. Aggressive normalization causes false positives; conservative causes false negatives. **Mitigation:** §5.4 rules documented and tested against handcrafted scar cases drawn from known guardrails (compound stop, MCP boundary, Param-Effect Canary).

4. **Entry-point catalog is hand-coded.** Each registration mechanism needs its own detector. **Mitigation:** target the three known mechanisms first (MCP tool registry, clap derive, axum routes). Adding new mechanisms is one detector each.

5. **Graph store performance at workspace scale.** **Mitigation:** add indices on `qname`, `crate`, `kind`, `signature_hash`; test query latencies against a stated budget. The storage layer is pluggable — swappable without touching schema or queries if scale forces the question.

6. **Determinism is non-negotiable but easy to break.** HashMap iteration order, unstable sort, parallel write ordering — any of these can produce non-deterministic graphs across runs. **Mitigation:** every collection sorted before serialization; single-threaded write; CI check that diffs two consecutive runs against the same SHA.

7. **The schema will need to change.** **Mitigation:** version the schema (`schema_version` attribute on every node), accept that the first shipped schema is v1.0 and there will be a v1.1.

8. **Estimated catch rate is unmeasured.** Treat all numerical estimates as Fermi-grade; replace with measured values after the first real consumer ships.

---

## 10. Out of scope (explicitly)

- Multi-language support. Rust-only for v1. Other languages are plausible later but not in this plan.
- IDE integration. Not an LSP server. Not a VS Code extension. CLI + HTTP API only.
- Visualization. Beyond `dot` output for one-off graphs. No web UI, no dashboard.
- Real-time / incremental updates. Re-extraction is the model. If incremental is needed later, reconsider with `ra` as the substrate.
- CI auto-fix. The tool surfaces findings; humans (or consumer skills) act on them.
- Replacing `cargo`, `clippy`, `rust-analyzer`, or any existing tool. v1 is *additive*.

---

## 11. References

- Glean: https://github.com/facebookincubator/Glean
- CodeQL: https://github.com/github/codeql
- rust-analyzer `ra-ap-*` crates: https://github.com/rust-lang/rust-analyzer

---

**End of plan.** Historical substrate for `docs/RFC-cfdb.md` and `docs/PLAN-v2-solving-original-problems.md`.
