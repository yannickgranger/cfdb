# Code Facts Database — v1 prospective plan

**Status:** prospective, pre-RFC. Council fodder.
**Date:** 2026-04-13.
**Supersedes:** the v0 LLM-extracted concept graph documented in `.concept-graph/README.md`. v0 carries forward as a labeling overlay; the structural extractor and the schema are replaced. See §2 for what carries vs. what gets rebuilt.
**Scope:** side project / tooling. Not in an external RFC track. May spin out to a separate repo if council decides (see §12 question 1).
**Author:** drafted by Claude in session 2026-04-13, pushed direct to develop as council material.

---

## 0. Problem

**Before the architecture, the problem.** This section is upstream of §1–§14. If the framing here is wrong, the rest is wasted work. It was added after the original v1 draft surfaced the false dichotomy "Glean-shaped platform or symbol-server-shaped index" — both shapes are solution-shaped, and neither was the actual question.

### 0.1 The retrofit constraint

the target workspace **already contains the drift**. 23 crates, multiple bounded contexts, ~10000 pub items, a year of agentic-session-driven accumulation. This is not a greenfield exercise where we pick patterns up front and enforce them from commit #1. Any tool we build must **characterize what exists first**, then help us fix it. Prevention of future drift is a secondary benefit, not the primary job.

This eliminates the class of "write the rules up front, enforce in CI" solutions that work on a new project. We need archaeology tooling, not just gatekeeping.

### 0.2 The demand surface — the skill system

The consumer of whatever we build is the **the consuming skill system** (the `/gate-*`, `/quality-*`, `/port-epic`, `/prescribe`, `/ship`, `/work-issue` family defined in the gate pipeline). Skills are the natural demand surface because:

- Skills are where decisions get made. `/prescribe` decides REUSE-vs-CREATE; `/port-epic` decides what gets moved; `/verify-issue` decides pass/fail. The fact base's job is to feed those decisions.
- Skills have measurable latency budgets and freshness tolerances, so *"what should the fact base serve"* becomes a finite question instead of an infinite platform.
- Skills are already how Claude interacts with this repo. Plugging a fact base into skills is cheaper than inventing a new interface.

**Four consumer classes** cover the skills that care about structural facts:

| Class | Skills | Latency | Freshness | Output shape |
|---|---|---|---|---|
| **Generation-time grounding** | `/prescribe`, `/prepare-issue`, `/work-issue` | sub-second per query | current branch | small JSON payload, injected into context |
| **Audit-time detection** | `/quality-architecture`, `/quality-metrics`, `/simplify`, `/verify-issue` | seconds per query | HEAD or last nightly | markdown report or PR comment |
| **Refactor glue** | `/port-epic`, `/boy-scout` | interactive | HEAD | structured move-this-to-there targets |
| **Plan validation** | `/gate-raid-plan` (new — see §3.3) | seconds per query | HEAD + plan input | pass/fail + list of plan holes |

The fact base serves all four from one substrate via the **same orthogonal API** (§6A): a small set of read/write verbs (`extract`, `enrich_*`, `query`, `query_with_input`, `diff`, ...) that consumers compose against. The four classes are a *demand picture*, not a *supply picture*. Each class consumes the same API by writing different Cypher compositions and picking a wire form (CLI/HTTP/Rust lib) — none of them extends the API or asks the tool for a feature.

**The polyvalence claim is: one orthogonal API surface, consumed by N consumers via different compositions, with no per-consumer tool features.** If a consumer wants something the API cannot express, the answer is to extend the *schema* (§6), not to add a verb.

### 0.3 The detect → refactor loop

Detection without a refactor path is a demoralizing backlog. Refactor without detection is drift-by-omission. The two halves must close the loop or the tool fails:

1. **Detect** — a query or scheduled audit finds drift (HSB, VSB, dead code, layer violation, etc.)
2. **Rank** — drift is prioritized by blast radius, age, test coverage, or whatever the consumer skill asks for
3. **Plan** — a skill turns the top-ranked drift into a migration blueprint (who owns what, what moves where, what gets rewritten vs dropped)
4. **Validate** — a skill cross-checks the blueprint against the fact base *before* execution: no dangling references, no forgotten callers, no missing canonicals
5. **Execute** — `/port-epic`-style archaeology move-not-copy, guided by the validated plan
6. **Re-audit** — the fact base is re-extracted, the detection query re-runs, the fixed drift drops out of the backlog

Step 4 — plan validation — is the step the existing tooling has no answer for, and it's the highest-leverage because a bad raid wrecks a sprint and a good raid depends on knowing every hidden caller *before* the first file moves. See §3.3.

### 0.4 The crux

The design question this plan answers is not *"Glean-shaped platform or symbol-server-shaped index?"* (the false choice earlier drafts held). It is:

> **What is the minimum fact schema that serves the queries the four skill consumer classes actually need, at the right latency and freshness per class, on a codebase that already contains the drift we want to characterize?**

Everything in §1–§14 is downstream of that question. If the answer is "a schema smaller than §6," we underbuild. If it's "larger than §6," we extend. The schema in §6 is a first cut; the consumer-skill enumeration is what drives it over time.

### 0.5 What this reshapes in the v1 build

v0 was an LLM-driven audit tool — a one-shot weekly-report generator. v1 is not just "v0 but deterministic." v1 is **a fact base that four skill classes consume at different latencies**, which reshapes every build phase:

- The *extractor* isn't the end of Phase A — it's the minimum substrate for whichever consumer class we target first.
- The *query library* isn't a 20-query dump — it's the specific queries each consuming skill needs.
- The *enrichments* aren't generic metadata — they're the signals the consuming skill needs to make its decision (e.g. `/gate-raid-plan` needs unwrap counts and test coverage as node attributes, not in a parallel report).
- The *serving path* isn't "CLI only" — the API exposes three wire forms (CLI, HTTP, Rust lib; see §6A.2) and consumers pick whichever fits their latency and integration model. Wire forms are not per-consumer features; they're API forms.

**Restart signal.** The existing `.concept-graph/*.py` scripts are v0. They carry forward only as historical reference — no code from them seeds v1. The v1 build starts over as a Rust extractor + Cypher query library + per-skill serving paths, **sequenced by which consumer class we target first** rather than by the old "extractor → queries → enrichments" layer ordering. See §7 and §12 Q9 for the first-consumer decision.

---

## 1. TL;DR

**Problem context:** §0 (read first). This TL;DR summarizes the solution shape only.

Replace the LLM-extracted concept graph (v0) with a **deterministic code facts database** that answers two distinct problems from the same fact base:

1. **Horizontal split-brain (HSB)** — concept duplication across crates at the same architectural layer (the v0 problem, solved properly).
2. **Vertical split-brain (VSB)** — parameter provenance loss along the call chain from MCP/CLI entry points down to the engine. The pattern where a value is parsed, defaulted, or transformed at multiple layers, the deepest fork wins, bug fixes hit the wrong fork, and E2E correctness silently regresses.

Both problems are *graph queries over the same underlying graph of code facts*. They differ only in which node labels and edge types they traverse. The current tool conflates extraction, query, and presentation; separating them and building each layer independently produces a polyvalent platform that answers the next eight questions for free (§8 lists 20).

The pattern is the local equivalent of **Glean** (Meta) / **CodeQL** (GitHub): one deterministic extractor produces typed facts over the workspace; an open-ended library of Cypher queries answers questions over those facts; enrichments (LLM descriptions, embeddings, git history) layer on top without touching the structural fact base.

The substrate (FalkorDB on LXC 501, the `:Concept`/`:Symbol`/`:Crate` schema, `query.py` as the CLI shape, `weekly-audit.py` reporting, `--update` purge-and-reingest semantics, SQLite progress tracking, the workstation-orchestrated extract pattern) carries forward from v0. What gets replaced is the LLM-driven extractor, and the schema is extended to hold call-graph and entry-point facts. **Build estimate: ~10 working days across four phases.**

---

## 2. v0 retrospective — what carries forward, what doesn't

**Built (2026-04-12 → 2026-04-13):** a persistent concept graph over `domain-*` and `ports*` crates in the target workspace. ~534 files ingested, ~1648 concepts, ~8198 edges. Extraction by qwen3:14b on vast.ai writing into FalkorDB on LXC 501. Queryable via `query.py` subcommands. PR #3616 fixed 3 of 5 split-brains the first audit flagged, validating the *concept of an indexed surface map* even though the extractor was structurally wrong.

**Carries forward (v0 → v1):**

- FalkorDB on LXC 501 as the persistent graph store. Cheap, fast, Cypher-capable, already deployed.
- Schema separation between authoritative definitions and references (the `:Concept` vs `:Symbol` distinction). Critical for any duplicate query to be meaningful — without it, every re-export looks like a duplicate.
- Workstation orchestration model (extraction is stateless, runs locally, writes to a server-side graph).
- The query CLI shape (`query.py` subcommands). v1 inherits this verbatim and extends with new subcommands.
- The `weekly-audit.py` canned-report pattern.
- The `--update` purge-and-reingest workflow for incremental refresh.
- `phase3.txt`-style file lists for scope control.
- The TrustGraph retrospective lesson (v0 README §1) — *before adopting a general-purpose tool, write down what you'd build yourself in 2 hours*. Applied here: do not adopt CodeQL or Glean unless the local build proves insufficient.

**Replaced in v1:**

- The LLM extractor. qwen3:14b reading file fragments and emitting JSON is replaced by a deterministic Rust extractor using `syn` (with `cargo metadata` for cross-crate metadata) that parses files into ASTs and emits typed facts. The LLM stays as an *enrichment layer* (Layer 2), only generating descriptions for items that lack `///` doc comments — never as the source of structural truth.
- The 8000-character chunk truncation (`extract.py:76`). Deterministic AST parsing has no chunk-size limit.
- The fragile column-0 regex chunker (`extract.py:112`). `syn::parse_file` handles modules, generics, macros, and visibility correctly.
- The schema, extended. v1 adds `:Item`, `:Field`, `:Variant`, `:Param`, `:CallSite`, `:EntryPoint` nodes and `CALLS`, `RETURNS`, `TYPE_OF`, `HAS_FIELD`, `HAS_PARAM`, `INVOKES_AT`, `EXPOSES` edges. The v0 nodes (`:Concept`, `:Symbol`, `:Crate`) remain, but `:Concept` becomes a *labeling overlay* on top of the structural facts, not the primary node type.

**The retrospective scar:** v0's structural correctness was unverified. No recall check against `rg`-counted `pub` items, no determinism check across reruns, no field-set extraction beyond enum variants, no call-graph at all. The graph was a paraphrase, not a model. **v1's load-bearing acceptance criterion is recall ≥ 95% per crate against `rg` ground truth, verified before any query is trusted.**

---

## 3. Problems and use cases on the table

### 3.1 Horizontal split-brain (HSB)

**Pattern:** the same business concept is implemented in multiple crates at the same architectural layer, often under the same name, sometimes under synonyms, sometimes with subtle structural variations.

**Concrete examples from this codebase (verified, all fixed by PR #3616):**

- `OrderStatus` defined in `domain-trading` AND `domain-portfolio`
- `PrunedStrategy` defined in `domain-strategy` AND `domain-portfolio`
- `RedistributedWeight` defined in `domain-portfolio` AND `ports-strategy`

**Failure mode without detection:** agentic sessions create parallel abstractions because they don't see what already exists. After 50–100 issues, the codebase has 3 parallel systems doing the same thing under different names. `/prescribe` attempts to force REUSE-vs-CREATE discipline via grep, but grep is structurally blind to synonyms.

**v0 detector:** name-equality query across crates. Caught the 3 same-name cases above. **Cannot catch:** `TradeSide` vs `Direction` synonyms, structurally-identical types under different names, or any duplicate where the LLM happened to skip the `pub struct` line during extraction.

**v1 detector (multi-signal, see §8 row 3):** structural hash collision + neighbor-set Jaccard + normalized name match + conversion-target sharing. A pair flagged by ≥2 signals is a candidate. Catches synonym-renamed duplicates that name-match cannot see, because two of the four signals don't look at names at all.

### 3.2 Vertical split-brain (VSB)

**Pattern:** a parameter or value enters at a top-level entry point (MCP tool, CLI command, HTTP route, cron job) and gets *re-resolved, re-defaulted, or transformed* at multiple layers on its way down to the engine. Each layer has its own fork of the resolution logic. When a bug surfaces, the coder finds and fixes one fork; the other forks still run; the bug persists end-to-end because the fix happened on the wrong branch.

**Concrete failure pattern:**

1. User calls MCP tool `screen_strategy` with `tf=1h`.
2. `qbot-mcp::handle_screen_strategy` parses `tf` → produces `Timeframe::H1` (resolution v1).
3. Handler calls `application::ScreenUseCase::execute(tf_str)` — passing the *string*, not the parsed type.
4. `ScreenUseCase` re-parses with its own logic → produces `Timeframe::Hour1` (resolution v2, different variant naming).
5. UseCase calls `Port::execute(tf2)`, which has yet another defaulting step for missing values.
6. Engine receives whatever the deepest resolver produced.
7. User reports a bug. Coder finds and fixes the resolver in the handler (`v1` site). CI passes. **Bug persists in production** because the application-layer resolver (`v2`) is the one that actually wins.

**This is exactly the failure mode CLAUDE.md §7 ("Param-Effect Canary") and "MCP Boundary Fix AC Template" exist to prevent.** Both rules are *runtime-asserted in canary tests*. Neither is *structurally enforced*. Vertical SB is invisible to every static check in the current toolchain. The "MCP Boundary Fix AC" template requires three things (parser delegates to domain `FromStr`, schema enumerates domain variants, error `valid_values` derived from enum) — but only at AC review time, against a single new handler. There's no tool that retroactively scans existing handlers to find the ones that violate it.

**v0 detector:** none. The concept graph has no call edges, no notion of dataflow, no entry-point catalog. VSB is structurally outside v0's vocabulary. No amount of polish on the v0 extractor (better prompts, embeddings, semantic clustering) can solve it — the underlying signal isn't in the graph.

**v1 detector (§8 row 4):** for each `:EntryPoint`, BFS over `CALLS*` edges from the registered handler. For each parameter declared on the entry point, find every `:Item{kind:Fn}` reachable from the handler whose `RETURNS` type matches the param's conceptual type. Count > 1 ⇒ vertical split-brain. Output the call chain with each resolver annotated.

### 3.3 Bounded-context raid (plan validation)

**Pattern:** architects discover that a module or bounded context has accumulated enough drift that incremental fixes are no longer viable. The right move is a raid — produce a clean blueprint from the existing module, portage the clean parts as-is, rewrite the glue, drop the dirty parts. The failure mode is the blueprint misses a hidden caller, a dangling type reference, or a forgotten internal dependency, and the raid ships broken or leaves orphans behind.

**Concrete shape in the target workspace:** the existing `/port-epic` skill already encodes this pattern — *archaeology first, move not copy, no new abstractions*. The methodology is right; the archaeology step today is a manual grep expedition that scales badly and misses things. The fact base turns archaeology from "several hours of grep + reading" into "one structured query per concern."

**Failure modes a validated plan catches:**

1. **Incompleteness** — the blueprint doesn't mention every `:Item` in the source context. Query: list every Item in source crate; diff against the plan's named set; report unnamed items.
2. **Dangling references** — the plan marks Item X as "drop" but Item Y (marked "portage") still calls or type-references X. Query: for each "drop" Item, find incoming `CALLS`/`TYPE_OF` from the portage+glue set. Any hit breaks the plan.
3. **Hidden callers** — the plan marks Item X as "portage" without accounting for callers *outside* the bounded context who will break when X moves. Query: for each "portage" Item, find incoming edges from outside the source context. Each hit is a caller the plan must explicitly address.
4. **Missing canonicals** — the plan marks concept C as "rewrite" without naming a target canonical. Query: for each "rewrite" concept, does the plan name a target `:Item` with `CANONICAL_FOR` intent? If not, the plan is silently incomplete.
5. **Clean/dirty signal mismatch** — the plan marks Item X as "portage (clean)" but the fact base's quality signals (unwrap count, test coverage, duplicate-cluster membership) disagree. Query: join structural facts with quality metrics; flag Items where plan and signals disagree.

**The skill:** `/gate-raid-plan`. Takes a raid plan (YAML) + the fact base as input. Produces a pass/fail + a list of plan holes (each hole names a specific Item, the query that failed, and a suggested fix). Runs *before* the raid starts as a go/no-go gate, and runs *again after each raid phase* to catch drift from the blueprint as files move.

**Why this is the highest-leverage consumer:** the other three classes (grounding, detection, refactor-glue) use the fact base for *lookup* or *detection* — "does X exist?", "find all drift of type Y." The raid consumer uses the fact base for **decision support** — it joins structural facts with quality signals to answer "is this plan safe to execute?" That is the first query in the catalog that's genuinely not doable by eye + grep. Everything else in §8 could (painfully) be done by hand; this one requires the graph.

**Schema implication:** this use case forces quality metrics (unwrap count, test coverage, duplicate-cluster membership, cyclomatic complexity) to live *on* `:Item` nodes as node attributes, not in a parallel report. Otherwise the raid skill has to join across two data stores, which defeats the "one fact base" premise. Several attributes in §6.1 are there specifically because of this use case.

### 3.4 What unifies them

All three are *graph queries over code facts*. HSB is a structural-similarity query over `:Item` nodes. VSB is a reachability-and-multiplicity query over `:CallSite`/`:CALLS` edges between `:EntryPoint` and `:Item{kind:Fn}` nodes. The raid is a decision-support query that joins structural facts with quality signals against a declared plan. They share the same underlying fact base — they differ only in which edges, node types, and attributes they exercise.

The current tool answers HSB badly, is structurally incapable of VSB, and cannot touch the raid use case at all. A polyvalent tool answers all three *and the next eighteen* without re-extraction, because the substrate is rich enough.

---

## 4. The insight — extract once, query N ways

This is how every serious code analysis platform works:

| Platform | Extraction layer | Query layer | Vendor |
|---|---|---|---|
| **Glean** | typed facts via Hack/C++/Python/Rust indexers | Angle (Datalog-flavored) | Meta, open source |
| **CodeQL** | per-language extractor → relational DB | object-oriented QL | GitHub, used for CVE detection |
| **Soufflé** | tree-sitter or custom extractors | Datalog | academia, security research |
| **Kythe** | per-language indexers → graph | GraphQL-like | Google, IDE backend |
| **rust-analyzer** | semantic database from incremental compilation | LSP queries | rust-lang official |

Every one of them separates **structural fact extraction** (deterministic, expensive, run on schedule or on commit) from **analysis** (cheap, query-based, run on demand). The current `.concept-graph/extract.py` conflates them: every analysis requires re-running the LLM extractor against vast.ai, and the result is non-deterministic across runs. **This is the architectural mistake to undo.**

**The polyvalent move is not a clever query — it's a schema rich enough that clever queries are short.**

---

## 5. Architecture — four layers

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Layer 4 — Formatters                                                     │
│   markdown report · JSON for /prescribe · terminal table · dot graph     │
└──────────────────────────────────────────────────────────────────────────┘
                                  ▲
                                  │
┌──────────────────────────────────────────────────────────────────────────┐
│ Layer 3 — Query library                                                  │
│   one .cypher per analysis · Python wrapper for pre/post-processing      │
│   exposed as `query.py <subcommand>` (inherits v0 CLI shape)             │
└──────────────────────────────────────────────────────────────────────────┘
                                  ▲
                                  │ (Cypher over FalkorDB)
                                  │
┌──────────────────────────────────────────────────────────────────────────┐
│ Layer 2 — Optional enrichments (additive, never structural)              │
│   git blame → LAST_TOUCHED_BY · LLM descriptions for undocumented        │
│   items · embedding vectors for semantic clustering · concept            │
│   synonym table → EQUIVALENT_TO edges                                    │
└──────────────────────────────────────────────────────────────────────────┘
                                  ▲
                                  │
┌──────────────────────────────────────────────────────────────────────────┐
│ Layer 1 — Extractor (deterministic, content-addressable)                 │
│   syn-based Rust binary · parses every file in workspace · emits         │
│   typed facts (Items, Fields, Params, CallSites, EntryPoints) ·          │
│   writes directly to FalkorDB · same content → same graph, byte-         │
│   for-byte                                                               │
└──────────────────────────────────────────────────────────────────────────┘
                                  ▲
                                  │
                        Rust source files
```

**The only LLM in the path is in Layer 2, and only as enrichment for undocumented items.** Layer 1 is fully deterministic and re-runnable on every commit. Drift between commits becomes detectable because the graph is reproducible.

**Each layer has an interface, not a coupling.** Layer 1 emits a fact schema (§6). Layer 2 reads facts and adds attributes/edges. Layer 3 reads anything. Layer 4 doesn't touch the graph. New analyses live entirely in Layer 3. New extraction targets (e.g. "extract Python files too") live entirely in Layer 1 and don't disturb anything else.

---

## 6. Fact schema (the contract Layer 1 must produce)

### 6.1 Nodes

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
| `:EntryPoint` | every MCP tool / CLI command / HTTP route / cron registration | `kind` (`mcp` / `cli` / `http` / `cron`), `name`, `handler_qname`, `params` (JSON of registered param names + types) | a top-level entry into the system |
| `:Concept` | overlay layer — labels Items semantically | `name`, `assigned_by` (`doc` / `rule` / `llm` / `manual`) | a semantic label |

### 6.2 Edges

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

**Call graph (the new addition — enables VSB):**

| Type | From | To | Meaning |
|---|---|---|---|
| `CALLS` | `:Item` (fn) | `:Item` (fn) | static call edge (best effort cross-crate) |
| `INVOKES_AT` | `:CallSite` | `:Item` (fn) | concrete invocation point |
| `RECEIVES_ARG` | `:CallSite` | `:Param` | which callee param this call binds |

**Entry-point graph:**

| Type | From | To | Meaning |
|---|---|---|---|
| `EXPOSES` | `:EntryPoint` | `:Item` (handler fn) | entry point dispatches to this fn |
| `REGISTERS_PARAM` | `:EntryPoint` | `:Param` (or virtual node) | a registered tool param |

**Concept overlay (replaces and extends v0):**

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

### 6.3 Item kinds

The seven values for `Item.kind`:

`Struct` · `Enum` · `Trait` · `Impl` · `Fn` · `Const` · `TypeAlias`

Note: `Service` and `Port` from v0 are **not** primitive kinds in v1. They're concept labels assigned via `LABELED_AS` (Layer 2 rule: "any unit struct in `domain-*::services::` is labeled `Concept{name:Service, assigned_by:rule}`"). This separation prevents the qwen3 misclassification problem where the same struct could come back as `Type` in one ingest and `Service` in the next — kinds are now structural facts (what `syn` saw), labels are an overlay (what we infer).

### 6.4 Type normalization rules

Type normalization is the load-bearing call. Aggressive normalization causes false-positive duplicates; conservative causes false-negative misses. The v1 default rules:

1. Strip lifetimes (`'a` → ∅) from all types.
2. Strip `&`, `&mut`, `*const`, `*mut` outer wrappers.
3. Strip `Box<T>`, `Arc<T>`, `Rc<T>`, `Cow<'_, T>` for the structural-hash purposes; keep the inner type.
4. Normalize `Option<T>` to `T?` (preserve nullability as a flag, not a wrapper).
5. Normalize `Vec<T>` and `[T; N]` to `T*` (preserve multiplicity as a flag).
6. Normalize `Result<T, E>` to `T!E` (preserve fallibility as a flag).
7. Generic parameter names are erased; only arity is preserved (`Vec<T>` and `Vec<U>` are equivalent).
8. Fully-qualified path resolution where possible. Unresolved paths preserved as-is and matched textually.

These rules are recorded in a `SCHEMA.md` file alongside the extractor. Every change is a schema version bump.

---

## 6A. Tool API surface

(Numbered §6A to mark this as a peer to §6 — the tool's interface to the data model — without renumbering §7–§14. Added 2026-04-13 to correct a pattern-matching drift in earlier drafts that conflated the tool's API with its consumers' use cases.)

The tool exposes a **small, orthogonal API** that consumers compose against. The API is decoupled from any specific use case or skill: it is the read/write contract over the fact base, **not** a bag of named verbs per consumer. The 27-row catalog in §8 is **example compositions**, not API surface — every row is a Cypher file consumers can copy, modify, or replace, and adding a row never extends the API.

**Design principle.** Keep the verb count under 20. Every named verb is a long-term commitment. Every question a consumer might want to ask is a Cypher composition over §6's schema, not a new verb. If the API grows beyond 20 verbs, the schema is wrong.

**Working name.** `cfdb` (code facts database) — placeholder; final naming is §12 Q8.

### 6A.1 Verb surface (11 verbs)

```
INGEST  (write side — extractor and enrichments)

  extract(workspace_path, target_keyspace) -> ExtractReport
    Walks `cargo metadata` + parses every .rs file via syn.
    Writes §6.1 nodes and §6.2 structural + entry-point edges into the keyspace.
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

**11 verbs total.** Everything else is Cypher composition over the §6 schema. Adding a 12th verb requires council approval and a §6A version bump.

### 6A.2 Wire forms

The API is exposed in three forms, all backed by the same in-process operations. The verbs are identical across forms; only the transport differs.

| Form | Use case | Latency model |
|---|---|---|
| **CLI** (`cfdb query …`) | humans, shell scripts, ad-hoc audits | per-invocation cold start (~100ms) |
| **HTTP** (`POST /v1/query`) | external consumers (skills running outside the cfdb process), latency-sensitive callers | warm process, sub-second per query |
| **Rust lib** (`use cfdb::query;`) | tests, Rust-native consumers, in-process composition | function call (microseconds) |

Consumers pick the form that fits their latency and integration model. Adding a fourth form (gRPC, stdio JSON-RPC, etc.) is straightforward because the verbs are the contract — not any wire format.

### 6A.3 Determinism guarantees (the contract)

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

These five guarantees are what makes the API composable. They're the load-bearing part of the contract; everything else (verb signatures, wire forms, schema details) can evolve, but breaking any of G1–G5 is a major-version event.

### 6A.4 What is explicitly NOT in the API

- **Named queries per use case** (`vsb()`, `raid_completeness()`, `prescribe_canonicals()`). Those are Cypher files in §8, consumed via `query()`. They are example compositions, not API verbs.
- **Output formatting** (markdown, dot, PR comments, terminal tables). `query()` returns JSON rows; formatting is the consumer's job.
- **Skill integration adapters.** Each consuming skill wraps `query()` in its own glue. `cfdb` ships no skill bindings and knows nothing about the skill system.
- **Refactoring actions.** `cfdb` is read-only at the source level; it never rewrites Rust files. Refactoring is the consumer's job.
- **Auto-fix / code modification.** Same reason.
- **Multi-language support.** Rust-only in v1. Adding a language means a new extractor targeting the same fact schema; the API is unchanged.
- **Opinionated workflows.** `cfdb` does not know what a "raid" is, what `/prescribe` wants, or what HSB means. Those are consumer-side compositions over a domain-agnostic API.
- **Caching.** Consumers cache results in their own caches. `cfdb` is a stateless query server over an immutable snapshot.

### 6A.5 How consumers compose against the API

Every consumer in §0.2's four-class table consumes the same 11 verbs. None extends the API. None requires a new verb. The differences are in *which Cypher they compose, which wire form they pick, and how they handle results* — not in what the tool exposes:

| Consumer class | Wire form | Verbs used | External inputs |
|---|---|---|---|
| Generation-time grounding | HTTP | `query` | params only (concept name, scope) |
| Audit-time detection | CLI | `query`, `diff` | none |
| Refactor glue | CLI or Rust lib | `query` | optional: target qnames |
| Plan validation | HTTP | `query_with_input` | plan.yaml's bucket sets |

If a consumer needs something the 11 verbs cannot express via Cypher composition, that is a signal to **extend the schema (§6)** — not the API. Schema extensions go through §12 Q2's schema-change policy. API extensions are major-version events and require council approval.

### 6A.6 Why this section exists

Earlier drafts of this plan drifted into pattern-matching the tool to its consumers — *one tool feature per skill, one serving path per consumer class*. That framing was wrong. It treats the tool as a bag of opinionated workflows instead of a fact base with a small composable API. The §0.2 four-consumer table remains useful as a *demand* picture (who wants what, with which latency), but it is not the *supply* picture. The supply picture is the 11 verbs above. **Consumers compose; the tool serves.**

This reframing changes the build sequencing in §7: Phase A ships the *minimum API surface needed to validate the API against one real consumer use case*, not "the first consumer class." The consumer is selected to stress-test the API early — see §12 Q9.

---

## 7. Build phases

**Restart posture.** The v1 build is a *restart* of the `.concept-graph/` tooling, not an extension of the v0 Python scripts. `extract.py`, `query.py`, and `weekly-audit.py` are archived as v0 reference — no code from them seeds v1. Phase A starts a new Rust extractor crate (dev-only, not part of the main workspace), a new FalkorDB keyspace (`qbot_v1`), and the §6A API surface.

**Phase ordering: ship the minimum API slice needed to validate the API against one real consumer use case, then expand.** This is a change from earlier drafts that sequenced by layer (extractor → queries → enrichments) *and* from intermediate drafts that sequenced "consumer class first." Both were wrong: the layer-first ordering deferred consumer feedback until everything was built; the consumer-class ordering pattern-matched the tool to the consumer instead of building the tool around its API. The new sequencing is:

1. Pick *one* validation use case (§12 Q9).
2. Identify the smallest §6 schema slice and the smallest §6A verb subset that use case requires.
3. Ship that slice end-to-end: extractor populates it, `query()` serves it, the consumer composes against it, feedback comes back.
4. Expand the schema and the example-composition catalog as more use cases come online — but the API verb count stays bounded.

The phases below describe the layer-wise deliverables, but each phase is scoped to "the slice the chosen validation use case needs," not "the whole layer."

**Which validation use case ships first is an open question — see §12 Q9.** Two leading candidates:

- **Grounding query for `/prescribe`** — cheapest schema slice (close to v0: `:Item`, `:Concept`, `CANONICAL_FOR`, `LABELED_AS`), highest touch frequency, directly addresses the "Claude invents parallel abstractions" failure mode that drives most of the drift. Stress-tests the `query()` verb plus the HTTP wire form. Smallest Phase A.
- **Plan-validation queries for `/gate-raid-plan`** — narrowest scope per use, highest leverage per use, forces quality signals (`unwrap_count`, `test_coverage`, `dup_cluster_id`) into the fact base from day one. Stress-tests `query_with_input()` (external sets) and forces the schema to carry quality attributes from Phase A. Larger Phase A but exercises more of the API up front.

Whichever is picked, **Phase A is not "the whole API surface" or "the whole §6 schema."** It's the slice the chosen use case needs. The other use cases ship in Phase B/C/D as the schema expands — but the API verb count stays at 11 throughout.

### Phase A — deterministic structural extractor (3 days)

**Goal:** Layer 1 produces the structural subset of the schema (§6.1 nodes + §6.2 structural and entry-point edges, plus call-site discovery) for the entire target workspace. Verified recall ≥ 95% per crate.

**Deliverables:**

- New crate `.concept-graph/extractor/` — Rust binary, dev-only, not part of the main workspace.
- Dependencies: `syn` (full feature), `walkdir`, `serde`, `serde_json`, `redis`, `cargo_metadata`, `blake3`.
- Walks `cargo metadata` to enumerate workspace crates and their source roots.
- Per file: `syn::parse_file`, visit top-level + nested items, emit JSON facts.
- Cross-file resolution: per-file symbol table from `use` statements + qualified paths. Unresolved targets become `:Symbol` placeholder nodes (preserves v0 schema escape hatch).
- Call-site discovery: visit `ExprCall` and `ExprMethodCall`, record `caller_qname`, best-effort `callee_qname` resolution.
- Entry-point discovery: detect MCP tool registrations, clap derive macros, axum routes, cron registrations.
- Direct write to FalkorDB graph `qbot_v1` (new keyspace; v0 graph `qbot` untouched).
- Recall verification harness: per-crate, compares `MATCH (i:Item {crate:$c}) RETURN count(i)` against `rg -c '^pub (struct|enum|trait|fn|type|const|static)' crates/$c/src/`.
- Macro special-case: detect `define_id!` and synthesize `:Item{kind:Struct}` entries from macro args. One special case per internal macro that defines pub items. Audit list via `rg 'macro_rules!.*pub (struct|enum)'` Day 1.

**Acceptance gate A:**

1. Recall ≥ 95% per crate (or documented gap in `KNOWN_GAPS.md` with explicit acknowledgement).
2. Determinism: same workspace SHA → same graph, byte-for-byte. Verified by running the extractor twice and `diff`-ing the FalkorDB dumps.
3. Full workspace ingested in `qbot_v1` (not just Phase 3 scope — adapters, application, CLI, all in).
4. Existing `query.py` subcommands work against `qbot_v1` with a `--graph` flag and produce reasonable output.

**Why 3 days, not 1:** the original 2-day plan estimated extractor at 1 day. That was for the *minimum* schema (just `:Item` + `TYPE_OF`). This phase includes call-site discovery (`:CallSite`, `INVOKES_AT`, `RECEIVES_ARG`) and entry-point cataloging (`:EntryPoint`, `EXPOSES`, `REGISTERS_PARAM`), both of which require an additional symbol-resolution pass. Honest estimate: 3 days to do this right with the recall verification gate.

### Phase B — query library v1 (2 days)

**Goal:** Layer 3 with the first eight queries from §8 wired as `query.py` subcommands.

**Deliverables:**

- `query.py clusters` — multi-signal HSB detector (structural + name + neighbor + conv signals).
- `query.py vertical` — VSB detector (per `:EntryPoint`, walk `CALLS*`, count distinct `RETURNS` of the entry param's type, flag count > 1).
- `query.py provenance <entry> <param>` — full call-chain trace for one param at one entry point, with each transformation annotated.
- `query.py surface <crate>` — bounded-context surface map (replaces v0 `crate` subcommand with structural-fact backing, adds fan-in counts).
- `query.py blast-radius <qname>` — reverse-`CALLS*` ∪ reverse-`TYPE_OF*` from a target Item.
- `query.py orphans` — items with zero incoming edges.
- `query.py hex-violations` — domain items referencing adapter items.
- `query.py drift <commit-a> <commit-b>` — diff two graph snapshots (requires Phase A determinism).

**Acceptance gate B:**

1. `query.py vertical` finds at least one real VSB candidate that maps to a CLAUDE.md scar (e.g. compound stop layer isolation, MCP boundary normalization, Param-Effect Canary precondition).
2. `query.py clusters` regression check: catches every duplicate v0 `query.py duplicates` flagged on Phase 3 scope, plus at least one new finding on the full workspace.
3. All eight subcommands run in < 30s on the full-workspace graph.
4. Spot-check: 10 randomly-sampled `clusters` results, ≥ 7 are real candidates (≥ 70% precision).

### Phase C — enrichments (2 days, partly parallelizable with B)

**Goal:** Layer 2 passes that add semantic richness without changing the structural fact base.

**Deliverables:**

- **Doc-comment extraction** — every `:Item` gets `doc_text` populated from `///` comments (already in Phase A, but called out here as the canonical description source — LLM is the *fallback*, not the default).
- **LLM description fallback** — for items with empty `doc_text`, batch-call a small local model (or vast.ai qwen3 if no local box) to generate a one-sentence description. Cache by content hash. Only runs on items missing docs; idempotent across reruns.
- **Embedding pass** — `name + doc_text + structural_summary` → vector via `all-minilm` (already loaded on the vast.ai box). Stored as a node attribute. Enables semantic-similarity queries in Layer 3 without re-running.
- **Concept synonym table** — hand-curated `EQUIVALENT_TO` edges between concept labels (`TradeSide ≡ Direction`, `Quantity ≡ Qty ≡ Amount`, `Position ≡ Holding`). Small (≤ 30 pairs), maintained in `.concept-graph/synonyms.toml`, applied via Layer 2 pass.
- **Git history pass** — populate `INTRODUCED_IN` and `LAST_TOUCHED_BY` from `git log --follow`. Enables age-based queries (e.g. "duplicates where one is 6 months old and one is 1 week old → newer is probably the unintentional fork").

**Acceptance gate C:**

1. Doc-comment recall ≥ 80% of items (depends on codebase doc density; flag the gap if low).
2. LLM enrichment is fully cached — second run does zero LLM calls if no items changed.
3. Concept synonym table loaded from file, never hand-edited in queries.
4. Embedding-pair similarity queries return < 50 candidates per crate at threshold 0.85 (sanity check on noise floor).

### Phase D — polish, integration, drift (3 days, low priority)

**Goal:** make the tool boring to run.

**Deliverables:**

- `make graph-refresh` target (workstation Makefile) — runs extractor against current HEAD, writes to `qbot_v1`.
- `make graph-audit` — runs `weekly-audit.py` against `qbot_v1`, produces markdown report.
- Pre-`/ship` hook (advisory) — runs `query.py drift HEAD~1 HEAD` to surface new HSB/VSB introduced by the current branch. Non-blocking initially; promote to blocking once trusted (council decision in §12 question 7).
- `query.py serve` — small HTTP wrapper exposing queries as JSON endpoints. Enables `/prescribe` to consume the graph programmatically without spawning Python processes.
- v0 archive: rename `extract.py` → `extract_llm_v0.py`, update v0 README to point to v1, archive the `qbot` graph as `qbot_v0_archive` for comparison studies.

---

## 8. Example compositions catalog

**Not API surface — example Cypher compositions.** The 27 rows below are sample queries demonstrating that the §6A API + §6 schema are rich enough to answer a wide range of structural questions. Each row is one Cypher file (or Cypher + small Python wrapper) consumers can copy, modify, or replace. **Adding a row does not extend the API. Removing a row does not break the API.** Consumers are free to compose their own Cypher and ignore this catalog entirely — the catalog is documentation of polyvalence, not the tool's interface.

Phase column is when the example ships in the bundled query library, separate from the API itself (which is fully present in Phase A regardless of which examples ship). Each row is added without touching extractor or schema unless explicitly noted.

| # | Analysis | Query shape | Phase |
|---|---|---|---|
| 1 | HSB by name | `MATCH (a:Item),(b:Item) WHERE a.name=b.name AND a.crate<>b.crate` | B |
| 2 | HSB by structural hash | same with `a.signature_hash = b.signature_hash` | B |
| 3 | HSB clusters (multi-signal) | union of name + structural + neighbor-jaccard + conv-target signals, ≥2 signals = candidate | B |
| 4 | **VSB / multi-resolver** | for each `:EntryPoint`, BFS `CALLS*`, find `:Item{kind:Fn}` whose `RETURNS` matches an entry param type. Count > 1 ⇒ vertical SB | B |
| 5 | **Param provenance trace** | for `:EntryPoint{name:X}` and param `P`, return full call subgraph with type transformations annotated | B |
| 6 | Param drop detection | `:CallSite` where caller has param P but callee has no `Param` of compatible type ⇒ candidate drop | C+ |
| 7 | Default duplication | `:Item{kind:Fn}` whose body contains literal X for param of concept T, where ≥ 2 such fns exist along one call chain | C+ |
| 8 | Bounded-context surface map | `MATCH (i:Item)-[:IN_CRATE]->(:Crate{name:$c}) RETURN i, fan_in` grouped by kind | B |
| 9 | Refactor blast radius | reverse-`CALLS*` ∪ reverse-`TYPE_OF*` from a target qname | B |
| 10 | Dead code / orphans | `:Item` with zero incoming `CALLS`, `TYPE_OF`, `IMPLEMENTS_FOR` | B |
| 11 | Hexagonal violation | `:Item{crate=~'domain-.*'}-[:TYPE_OF]->:Item{crate=~'adapters-.*'}` | B |
| 12 | Decorator bypass | `:EntryPoint -[:EXPOSES]-> :Item -[:CALLS*1..2]-> :Item{kind:Impl}` skipping known decorator chain | C+ |
| 13 | Drift between commits | diff two graph snapshots; new HSB/VSB, removed canonical, changed signatures | B |
| 14 | Concept ownership | `:Concept` reachable only from items in one crate ⇒ that crate owns it ⇒ ownership map | C |
| 15 | Test-coverage gap | `:Concept` with no `LABELED_AS` from any item under `tests/` or `#[cfg(test)]` | C |
| 16 | Param-canary coverage | for each `:EntryPoint` param, exists path to a test item that mentions both the param name and the engine's resolved type? | C+ |
| 17 | High-fan-in extraction candidate | `:Item` with ≥ N incoming `TYPE_OF` from distinct crates ⇒ candidate for shared base crate | C |
| 18 | Cyclomatic hot spots | `:Item{kind:Fn}` with high branch count (extracted by Phase A) | C |
| 19 | Trait coherence | `:Item{kind:Trait}` whose `IMPLEMENTS_FOR` targets span ≥ N crates ⇒ contract surface candidate | C |
| 20 | Stale code | `:Item` with `LAST_TOUCHED_BY` older than threshold AND zero incoming refs | C+ |
| 21 | **Raid completeness** | for source crate $C, list every `:Item{crate:$C}` not named in plan's portage/rewrite/glue/drop sets ⇒ forgotten items | B/raid |
| 22 | **Raid dangling-drop** | for each "drop" Item in plan, find incoming `CALLS`/`TYPE_OF` from any "portage"+"glue" Item ⇒ broken plan | B/raid |
| 23 | **Raid hidden-callers** | for each "portage" Item, find incoming edges from `:Item` outside source crate ⇒ external callers plan must address | B/raid |
| 24 | **Raid missing-canonical** | for each "rewrite" concept in plan, check plan names a target `:Item` with `CANONICAL_FOR` intent ⇒ silently incomplete plans | B/raid |
| 25 | **Raid signal-mismatch** | join "portage (clean)" Items with quality attributes (unwrap, coverage, dup-cluster); flag where plan and signals disagree | B/raid |
| 26 | **Canonical lookup** | for concept C, return `:Item` nodes with `CANONICAL_FOR` C ⇒ `/prescribe` grounding payload | B/ground |
| 27 | **Scope neighborhood** | for an issue's file set, return `:Item` in same modules + their `LABELED_AS` concepts ⇒ "what already exists nearby" for `/prepare-issue` | B/ground |

The two original problems are rows 3 and 4. The bounded-context raid (§3.3) is rows 21–25. Generation-time grounding is rows 26–27. The other 18 are *the same tool* with different `.cypher` files. **That's the polyvalence.**

**Phase column legend updated.** `B/raid` = ships in Phase B *if* plan-validation is the first consumer class (§12 Q9). `B/ground` = ships in Phase B *if* grounding is the first consumer class. The other Phase B rows ship regardless because they're the substrate for both.

---

## 9. Options considered

| Option | Approach | Pro | Con | Verdict |
|---|---|---|---|---|
| **A. Extend v0 (qwen3 + better prompts)** | Iterate the LLM extractor; add embedding clustering on top of LLM output | Reuses existing infra fully | LLM extraction is non-deterministic, low recall, and structurally unfit for call-graph data. Cannot solve VSB at any prompt-engineering effort | ❌ rejected |
| **B. Code facts database (this proposal)** | Deterministic Rust extractor + FalkorDB + query library | Polyvalent, reproducible, exits the LLM critical path, schema-driven extensibility | Build cost (~10 days of focused work). Initial schema design is load-bearing | ✅ recommended |
| **C. Adopt CodeQL** | Use GitHub's CodeQL for Rust | Production-grade analysis platform, mature query language | CodeQL Rust support is preview; vendor-locked; doesn't natively know about MCP entry points or the target workspace's domain concepts; query DSL is its own learning curve. Not local-first. Same trap as TrustGraph at a higher quality bar | ❌ heavy and external |
| **D. Adopt Glean** | Deploy Meta's open-source Glean | Closest to ideal architecture | Re-runs the TrustGraph mistake at smaller scale: massive infra for a single-project use case. The TrustGraph retrospective in v0 README §1 applies verbatim | ❌ same trap |
| **E. Use rust-analyzer's database directly** | Query `ra`'s on-disk index via LSP or in-process | Authoritative call graph, semantic, fast | `ra` database isn't designed as a public query surface. The `mcp__ra-query__*` tools are scoped to specific quality queries (unwraps, complexity) and don't expose the full HIR. Would require building a new query layer on top of `ra-ap-*` crates — at which point we've built our own extractor, just with `ra`'s frontend instead of `syn`'s | ⚠️ defer; revisit if `syn` proves insufficient for cross-crate call resolution |
| **F. Datalog (Soufflé / Crepe)** | Express analyses as Datalog rules over facts | Most powerful query semantics; recursive queries are first-class | Adds a second query language alongside Cypher; FalkorDB doesn't speak Datalog; Cypher is sufficient for everything in §8 | ⚠️ defer; Cypher first |

**Recommendation:** Option B. Rationale: it's the only option that solves both problems on the table, leverages existing infra (FalkorDB, schema, query CLI), exits the LLM dependency for structural truth, and produces an artifact that grows with the project. The build cost is real (~10 days) but bounded, and the deliverable is reusable across analyses indefinitely.

**Fallback if Option B blocks:** if `syn`-based cross-crate call resolution proves intractable in Phase A, escalate to Option E (use `ra-ap-*` crates as the extractor frontend). The schema and Layer 3 query library are unchanged — only Layer 1's implementation swaps. This is a valid escape hatch precisely because the layers have interfaces, not couplings.

---

## 10. Risks

1. **Cross-crate call resolution with `syn` alone is hard.** `syn` is a single-file parser. Resolving "this `foo()` call refers to `domain_strategy::resolver::foo`" requires a workspace-wide symbol table built from `use` statements + qualified paths. **Mitigation:** start with intra-crate resolution; emit unresolved cross-crate calls as edges to `:Symbol` placeholders (matches v0 escape hatch). Cross-crate resolution can land late in Phase A or early in Phase B without blocking other queries.

2. **Macro-defined items are invisible to `syn`.** `define_id!`, `derive`, and any project-internal item-emitting macro produce items the AST visitor never sees. **Mitigation:** explicit special-case detection for known macros; audit list via `rg 'macro_rules!.*pub (struct|enum)'`. Long-term: invoke `cargo expand` per crate and parse the expanded output. This is a known gap that should be tracked in `KNOWN_GAPS.md`.

3. **VSB detection precision depends on type normalization.** "Same conceptual type" needs `Timeframe`, `&Timeframe`, `Option<Timeframe>`, `Result<Timeframe>` to all match. Aggressive normalization causes false positives; conservative causes false negatives. **Mitigation:** Phase A normalization rules are documented in `SCHEMA.md` (§6.4 above) and tested against handcrafted scar cases drawn from CLAUDE.md scars (compound stop, MCP boundary, Param-Effect Canary).

4. **Entry-point catalog is hand-coded.** `:EntryPoint` extraction needs to know how MCP tools, CLI commands, and HTTP routes are registered. Each registration mechanism needs its own detector. **Mitigation:** Phase A targets the three known mechanisms (MCP tool registry, clap derive, axum routes). Adding new mechanisms is one detector each; document in `SCHEMA.md`.

5. **FalkorDB query performance at workspace scale.** Phase 3 was 1648 nodes / 8198 edges. Full workspace + call graph is plausibly 10x: ~15000 items, ~80000+ edges. **Mitigation:** add indices on `qname`, `crate`, `kind`, `signature_hash` at graph creation; test query latencies against acceptance gate B's 30s budget. If FalkorDB struggles, fallback is DuckDB with a relational schema (same fact set, different storage).

6. **Determinism is non-negotiable but easy to break.** HashMap iteration order, unstable sort, parallel write ordering, redis pipelining order — any of these can produce non-deterministic graphs across runs. **Mitigation:** every collection sorted before serialization; single-threaded write to FalkorDB; Phase A acceptance gate is a literal `diff` of two consecutive runs against the same SHA. Add a determinism CI check.

7. **The schema will need to change.** v1 isn't the final shape. **Mitigation:** version the schema (`schema_version` attribute on every node), add a `migrate.py` from day one, accept that Phase A's schema is v1.0 and there will be a v1.1.

8. **v1.1 estimated catch rate is unmeasured.** Estimated total population on full workspace: 75–125 split-brain clusters (Fermi reasoning, not a count). Estimated v1.1 recall: 80–90%. Marginal value over v0 + audit-split-brain: ~30–50 findings neither could surface. The actual numbers are unknown until Phase B ships and runs against the full graph. **Mitigation:** treat all numerical estimates in this document as Fermi-grade; replace with measured values after Phase B.

9. **Council bikeshed risk on naming, hosting, and ownership.** Questions in §12 are real but easy to defer. **Mitigation:** ship Phase A regardless of those decisions. Phase A's deliverable is independent of council answers — extractor + recall verification happens whether the tool is in-tree, side-repo, called `cfdb` or `concept-graph` or `qbot-graphtool`.

---

## 11. Migration from v0

**Strategy:** parallel coexistence, not cutover. v0 graph (`qbot`) and v0 extractor (`extract_llm_v0.py`) stay in place. v1 builds in `qbot_v1` keyspace with a separate extractor binary. Both are queryable from `query.py` via a `--graph` flag. Once v1 reaches Phase B and is trusted, v0 is archived.

**Timeline:**

- Day 1: scaffold v1 extractor, write to `qbot_v1`. v0 untouched.
- Day 5: Phase A complete. v1 graph populated, recall verified. v0 still primary for any prior workflow.
- Day 7: Phase B complete. v1 query library exposes new analyses. Both graphs queryable.
- Day 10: Phase C complete. Enrichments wired. v1 has structural + semantic richness.
- Day 13+: Phase D, v1 becomes default, v0 archived.

**Backout:** at any point, `qbot_v1` can be dropped and v0 continues working. The v0 schema and tooling are untouched throughout.

---

## 12. Open questions for council

1. **Is this a side project or in-tree tooling?** The work lives in `.concept-graph/` (under the consuming tree) or in a separate repo? Side-project repo enables independent iteration but separates the tool from the codebase it analyzes. In-tree keeps proximity but adds dev-deps to the workspace. **Recommendation:** in-tree under `.concept-graph/` (where v0 lives) until the tool ingests a second repo.

2. **Who maintains the schema?** This is the API. Schema changes need a versioning policy, a migration story, and a review gate. Should the schema live in a `SCHEMA.md` doc with explicit version bumps, or as a Rust file with `serde` types? **Recommendation:** both. `SCHEMA.md` is human-authoritative and council-reviewed. Rust types are derived from it and enforced by the extractor. Schema bumps are explicit commits with both files updated.

3. **Does this replace `audit-split-brain`?** The textual `audit-split-brain` tool finds 122 violations via a different mechanism (tokenized line matching). v1 finds them via structural facts. They overlap but don't subsume — `audit-split-brain` catches *literal duplicates* that v1 might miss if the items are macro-generated. Council decides whether to retire `audit-split-brain`, run both, or merge. **Recommendation:** run both for one quarter, compare findings, then merge or retire based on data.

4. **`syn` vs `ra-ap-*` for the extractor.** `syn` is simpler but can't resolve cross-crate references. `ra-ap-*` (rust-analyzer's library crates) gives you HIR with full resolution but is a much heavier dependency and a less stable API. Default recommendation is `syn` first, escalate if blocked. Council can override.

5. **Is FalkorDB the right substrate long-term?** It's a Redis module, single-node, no clustering. For a single-project knowledge graph this is fine. If the tool grows to ingest multiple repos (qbot-dashboard, agency-control, etc.), reconsider. Alternatives: Memgraph, Neo4j Community, DuckDB + relations. **Recommendation:** FalkorDB for v1; revisit at v2 when scale forces the question.

6. **LLM enrichment budget.** Phase C uses LLM for items missing doc comments. With ~10000 items and ~50% docs coverage, that's 5000 LLM calls per full re-extraction. Cached by content hash, so subsequent runs are cheap. Initial run: ~$5 if using Claude Haiku, ~free if using qwen3 on vast.ai. Council confirms the budget model and the LLM choice.

7. **Pre-`/ship` hook integration.** `query.py drift HEAD~1 HEAD` produces a per-PR diff of new HSB/VSB findings. Should this be (a) advisory comment on PR, (b) blocking gate, (c) just a manual `make` target? **Recommendation:** (a) advisory for first month, (b) blocking after, gated on precision metrics from §B.4.

8. **Naming.** The current name "concept graph" is misleading once the tool is structural-facts-based. Suggested rename: `qbot-cfdb` (code facts database) or `graphtool` or `workspace-index`. Council picks. **Recommendation:** keep `.concept-graph/` directory name to avoid churn; rename only if council strongly prefers.

9. **Which validation use case ships first?** (Added 2026-04-13 alongside §0; reframed alongside §6A.) Phase A ships the minimum slice of the §6A API + §6 schema needed to validate the API against **one real consumer use case**. The four consumer classes in §0.2 do not "ship as classes" — they ship as Cypher compositions against the same 11-verb API, with the schema expanding as more compositions need more facts. The question is *which use case stress-tests the API hardest and earliest*, so we get feedback before the schema is locked in.

   Two leading candidates:

   - **(a) Grounding query for `/prescribe`** — Cheapest schema slice (close to v0: `:Item`, `:Concept`, `CANONICAL_FOR`, `LABELED_AS`). Reuses the existing `/prescribe` skill as the integration point with no new skill to build. High touch frequency (every issue, every refactor session). Directly addresses the "Claude invents parallel abstractions" failure mode. Stress-tests `query()` and the HTTP wire form (§6A.2). Lower per-use leverage but higher total volume. Smallest Phase A.

   - **(b) Plan-validation queries for `/gate-raid-plan`** — Narrowest scope per use (one bounded context at a time), highest leverage per use (a successful raid pays back the build cost in one move). Forces quality signals (`unwrap_count`, `test_coverage`, `dup_cluster_id`, `cyclomatic`) onto `:Item` nodes from day one — closes the parallel-report trap before it opens. Maps onto the existing `/port-epic` archaeology methodology. Stress-tests `query_with_input()` (the external-sets verb) and forces the API's hardest verb to ship in Phase A. Requires building the `/gate-raid-plan` skill itself alongside the substrate. Larger Phase A but exercises more of the API up front.

   **Recommendation:** depends on what's *most painful right now*. If the daily friction is "Claude keeps inventing parallel things and `/prescribe` can't see them," ship (a) grounding first — fastest path to a felt win. If there's a known-dirty bounded context that needs a raid this quarter, ship (b) plan-validation first — the API gets stress-tested harder and the schema's quality-signal commitment is locked in early. Both are valid first picks; audit-time detection and refactor-glue use cases are easier downstream of either.

   This decision determines:
   - Which schema attributes are mandatory in Phase A vs deferrable to Phase B/C
   - Which §6A verbs are exercised in Phase A (`query` only vs `query` + `query_with_input`)
   - Which wire form (§6A.2) gets battle-tested first (HTTP for grounding, HTTP + external-sets for plan validation)
   - Which §8 example compositions ship with Phase A's bundled query library
   - Which existing or new skill gets the first integration

   What this decision does **not** determine: the API verb count (always 11, regardless), the wire-form list (always CLI + HTTP + Rust lib), the determinism guarantees (always G1–G5), or the schema's long-term shape (always converges to §6 as use cases come online).

---

## 13. Out of scope (explicitly)

- Multi-language support. Rust-only for v1. Python (`a0/`, `qbot-dashboard`) is plausible later but not in this plan.
- IDE integration. Not an LSP server. Not a VS Code extension. CLI + HTTP API only.
- Visualization. Beyond `dot` output for one-off graphs. No web UI, no dashboard.
- Real-time / incremental updates. Re-extraction is the model. If incremental is needed later, reconsider with `ra` as the substrate.
- CI auto-fix. The tool surfaces findings; humans (or `/prescribe`) act on them.
- Replacing `cargo`, `clippy`, `rust-analyzer`, or any existing tool. v1 is *additive*.
- Embedding signal in Phase B. Defer to Phase C — deterministic signals alone should produce a usable HSB result for Phase B's acceptance gate.

---

## 14. References

- `.concept-graph/README.md` — v0 documentation, retained as historical record
- `.concept-graph/extract.py` — v0 LLM extractor (will be renamed `extract_llm_v0.py` in Phase D)
- `.concept-graph/query.py` — v0 query CLI (extended for v1 with `--graph` flag and new subcommands)
- `.concept-graph/phase3-audit.md` — v0 first-pass audit, validates the *concept of an indexed surface map*
- `CLAUDE.md` §7 — Param-Effect Canary rule (the runtime version of what VSB detects statically)
- `CLAUDE.md` §7 — MCP Boundary Fix AC Template (ditto, applied at AC review time per-handler)
- `CLAUDE.md` §6 hand-forged baseline scar — the failure mode that motivates Phase A's determinism gate
- Glean: https://github.com/facebookincubator/Glean — reference architecture
- CodeQL: https://github.com/github/codeql — reference architecture
- rust-analyzer `ra-ap-*` crates: https://github.com/rust-lang/rust-analyzer — fallback extractor frontend

---

**End of plan.** Council convene at will; this is the substrate for an RFC, not the RFC itself.
