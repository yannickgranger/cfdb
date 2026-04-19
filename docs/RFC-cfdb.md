---
title: cfdb — code facts database (v1)
status: Draft for council review
date: 2026-04-13
supersedes: .concept-graph/PLAN-v1-code-facts-database.md (kept as background)
audience: agent-teams council (see §1)
---

# RFC: cfdb — code facts database (v1)

A stand-alone Rust tool that indexes one or more Rust workspaces into a queryable graph of typed code facts, then exposes a small orthogonal API so skills, agents, and humans can ask structural questions about the codebase without re-extracting or re-reasoning from raw source.

---

## 1. Council briefing — how to use this RFC

This RFC is written for review by an **agent team** (`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`), not by a single reader. It is structured so different specialists can take different sections in parallel without reading the whole document.

**Council composition (6 teammates):**

| Role | Lens | Primary sections | Adversarial scope |
|---|---|---|---|
| **Chief Product Officer** | product strategy | §3 Problems, §13 v0.1 scope, §16 roadmap, §14 Q1/Q4/Q8 | "Are these the right 9 problems to solve? Does v0.1 produce a felt win? What's the strategic fit against the open backlog?" |
| **Clean architect** | Clean Architecture (boundaries, dependency rule, layers) | §6 API, §8 Architecture, §9 Multi-project, §14 Q3/Q5 | "Where does the API leak infrastructure into the domain? Is the project registry the right boundary? Does Kuzu live where it should?" |
| **SOLID architect** | SOLID principles (SRP, OCP, LSP, ISP, DIP) | §6 API, §7 Schema, §14 Q6/Q7 | "Where do the 11 verbs violate ISP? Is the schema Open/Closed under v1.x? Does the extractor own one responsibility or three?" |
| **Rust guru coder** | Rust ecosystem, idiomatic patterns, ownership | §10 Stack, §10.1 store, §8.1 crate layout, §14 Q2 | "Is `syn` enough for cross-crate resolution? Are the Kuzu Rust bindings production-ready? Is the 10-crate layout idiomatic or fragmented?" |
| **LLM specialist (@anthropic)** | LLM-as-consumer + LLM-as-enrichment | §3.5–§3.7, §11 ad-hoc agents row, §16 v0.3 LLM enrichment, §17 references | "Where does the LLM fit in the loop, and where doesn't it? Can an agent actually compose Cypher against this schema with typical session context? Is the prompt-construction story for /prescribe grounding spelled out?" |
| **QA tester** | testability, determinism, regression coverage | §12 Determinism, §13 acceptance gate, §15 Risks, §14 Q1/Q7 | "How do you test extractor recall? How does the determinism CI check actually run? What breaks first under schema churn? Are the v0.1 acceptance gate items 1–6 actually verifiable?" |

The 6 roles cover product strategy, two architectural lenses (Clean and SOLID — intentionally adversarial to each other), implementation-language fluency, the LLM-consumer angle (this is a tool LLM-driven skills will use, so an Anthropic-side specialist is load-bearing), and end-to-end testability. **Domain-specific qbot-core knowledge stays with the user**; it does not need its own council seat — the user is in the loop for any backlog-grounded question.

**How to spawn the team:**

```text
Create an agent team to review the RFC at .concept-graph/RFC-cfdb.md.
Spawn 6 teammates matching the §1 composition table:
  - chief-product-officer
  - clean-architect
  - solid-architect
  - rust-guru-coder
  - llm-specialist-anthropic
  - qa-tester
Each takes its primary sections from §1, posts findings to the shared
task list, and challenges other teammates' conclusions. Clean architect
and SOLID architect should disagree productively; QA tester should
challenge every claimed guarantee in §6/§12/§13; LLM specialist owns
the consumer-angle review for the ad-hoc agents row in §11. Converge on
§14 decisions when all six report. Require plan approval for any
teammate proposing structural changes to the API verb set or the fact
schema.
```

**Convergence target:** the council outputs a vote on §14's decisions plus a "must-fix before v0.1 starts" list. Lead synthesizes; user reviews the synthesis.

---

## 2. Summary

cfdb is the deterministic Rust successor to the LLM-based concept graph in `.concept-graph/extract.py`. It is **not a rewrite** — it is a ground-up Rust project with a new architecture, new API, new substrate, and multi-workspace scope from day one.

**Core claim:** *one fact base, a small composable API, many downstream consumers.* Every question an agent or skill asks — *"where are the Kalman filters used?"*, *"is the Ledger concept duplicated?"*, *"does this raid plan have hidden callers?"*, *"which adapters call `reqwest::Client::new()` in violation of RFC-027?"* — is a Cypher composition over the same substrate. The tool ships **11 API verbs, 3 wire forms, 5 determinism guarantees, and a fact schema large enough to answer all 9 problem patterns in §3**. Consumers compose; the tool serves.

**Multi-project from day one.** cfdb indexes any Rust workspace, not just qbot-core. A project registry (`.cfdb/projects.toml`) lists the workspaces to ingest; each gets its own keyspace. Cross-project queries are post-v0.1.

**No Python carries forward.** `extract.py`, `query.py`, `weekly-audit.py` are archived as v0 reference. cfdb starts fresh in Rust.

---

## 3. Problems cfdb solves (with backlog evidence)

Earlier drafts of this plan listed 4 problems (HSB, VSB, raid, Kalman/Ledger questions). The qbot-core P0/P1 backlog reveals **9 distinct patterns**. Each pattern below is a class of bug that recurs across issues; cfdb expresses each as a Cypher composition over the §7 schema, replacing what is currently handwritten Rust architecture tests, manual grep audits, or "found this in code review" one-offs.

### 3.1 Pattern A — Horizontal split-brain (HSB)

**Same business concept, parallel implementations across crates.** Often under the same name, sometimes under synonyms, sometimes structurally identical with renamed fields.

**Backlog evidence:** PR #3616 fixed `OrderStatus` (in `domain-trading` AND `domain-portfolio`), `PrunedStrategy` (in `domain-strategy` AND `domain-portfolio`), `RedistributedWeight` (in `domain-portfolio` AND `ports-strategy`).

**cfdb query:** multi-signal structural similarity over `:Item` — name match + signature hash + neighbor-set Jaccard + conversion-target sharing. Catches synonym-renamed duplicates that grep cannot see.

### 3.2 Pattern B — Vertical split-brain (VSB) / param ignored

**A parameter enters at an entry point, is re-resolved at multiple layers, and the wrong fork wins.** Or a param is accepted at the boundary and silently dropped before reaching the engine.

**Backlog evidence:**
- **#2651** `bug: compound stop stop_atr_mult param accepted but ignored — all values produce same output`. The textbook VSB case: param flows through but doesn't reach the computation.
- The CLAUDE.md §7 Param-Effect Canary rule exists specifically for this; cfdb makes it static.

**cfdb query:** for each `:EntryPoint`, BFS over `CALLS*` from the handler; find every `:Item{kind:Fn}` whose `RETURNS` matches the entry param's type. Count > 1 ⇒ multi-resolver fork. Plus: for each declared entry param, check that its name reaches the engine call site — orphaned params are dropped params.

### 3.3 Pattern C — Wiring of canonical vs ad-hoc impl (the Ledger case)

**A canonical implementation exists; some call sites use it, others use a parallel ad-hoc version.** The user's exact example. The repo has it as a literal P0.

**Backlog evidence (this is the one):**
- **#3525** `fix(ledger): LedgerService::record_trade calls append() not append_idempotent()`. There are two ledger append methods. One is the canonical idempotent version. The other is the original non-idempotent version. `record_trade` calls the wrong one. **This is exactly "incomplete wiring of multiple implems scattered built ad-hoc."**
- **#3523** `fix(ledger): liquidation_entries hardcodes long-side semantics`. Multiple liquidation paths with side-specific logic that should converge.
- **#3521** `fix(ledger): funding/margin/realized-PnL entries cancel to zero in cash projection`. Multiple entry types with subtly inconsistent sign conventions across call paths.

**cfdb query (the user's exact ask, decomposed):**

```cypher
// For concept "Ledger", find:
// 1. All implementations
// 2. Which is canonical (if any)
// 3. Which call sites bypass the canonical
// 4. Which implementations are dead
MATCH (impl:Item)-[:LABELED_AS]->(c:Concept {name:'Ledger'})
OPTIONAL MATCH (canonical:Item)-[:CANONICAL_FOR]->(c)
OPTIONAL MATCH (caller:Item)-[:CALLS]->(impl)
WITH impl, canonical, collect(DISTINCT caller) AS callers
RETURN
  impl.qname AS implementation,
  canonical.qname AS canonical_impl,
  size(callers) AS fan_in,
  CASE
    WHEN canonical IS NULL THEN 'NO_CANONICAL'
    WHEN impl <> canonical AND size(callers) > 0 THEN 'BYPASS'
    WHEN size(callers) = 0 THEN 'DEAD'
    ELSE 'OK'
  END AS verdict
```

One query, four findings. The `/prescribe` skill or any agent can issue it via `query()` — no tool feature, no new verb, no per-pattern code in cfdb.

### 3.4 Pattern D — Forbidden function / forbidden pattern in scoped crates

**A function or pattern is banned in some crates but allowed in others.** Currently enforced by handwritten Rust architecture tests that walk the AST per RFC. Every new ban requires a new architecture test.

**Backlog evidence (this is the largest category — 6+ open issues):**
- **#3577** `fix(RFC-027-S3): ban reqwest::Client::new() in adapters — require default_http_client`
- **#3574** `test(RFC-026-S3): architecture test banning Utc::now() in domain-*/ports-*`
- **#3573** `fix(RFC-026-S2): eliminate Utc::now() from domain-* and projection code`
- **#3571** `test(RFC-025-S3): architecture test banning unwrap_or(ZERO) in financial crates`
- **#3569** `fix(RFC-025-S1): sweep unwrap_or(ZERO) + unwrap_or(Null) in domain + ledger crates`
- **#3568** `test(RFC-024-S4): architecture test banning f64 in domain-*/ports-*`
- **#3567** `fix(RFC-024-S3): voter + allocator + pair-engine — eliminate f64 in signal chain`

**cfdb query (one Cypher per ban, declarative):**

```cypher
// RFC-026: ban Utc::now() in domain-* and ports-*
MATCH (caller:Item)-[:CALLS]->(target:Item)
WHERE caller.crate =~ '(domain|ports)-.*'
  AND target.qname = 'chrono::Utc::now'
RETURN caller.qname, caller.file, caller.line
```

Every ban rule becomes a `.cypher` file in the project's rules directory. The bundled `cfdb arch-check` example composition runs all rule files and outputs a violation report. **This replaces 6+ handwritten Rust tests with 6 declarative queries**, and adding a 7th ban is one new file, not a new test crate.

### 3.5 Pattern E — Required-property enforcement

**Every Item in a structural class must satisfy a property.** "Every adapter that issues an HTTP request must use `default_http_client`." "Every struct field carrying a credential must use `SecretString`." "Every order submission must include an idempotency key."

**Backlog evidence:**
- **#3576** `fix(RFC-027-S2): SecretString on all credential-carrying structs + redact Debug`
- **#3577** `fix(RFC-027-S3): ban reqwest::Client::new() in adapters — require default_http_client` (also Pattern D)
- **#3515** `fix(money-path): Bybit order submission missing orderLinkId (no idempotency key)`
- **#3514** `fix(money-path): Capital execute_with_retry can duplicate open_position on 401/4`

**cfdb query:**

```cypher
// RFC-027: every credential-bearing field must use SecretString
MATCH (item:Item {kind:'Struct'})-[:HAS_FIELD]->(f:Field)
WHERE f.name =~ '(?i).*(password|token|secret|api_key|credential).*'
  AND f.type_normalized <> 'SecretString'
RETURN item.qname, f.name, f.type_normalized
```

The schema has the required information (field types, names, structural class) so the query is one-liner. cfdb does not know what `SecretString` is — it knows what the schema says, the rule encodes the policy.

### 3.6 Pattern F — Money-path safety / concept-co-occurrence

**A function in a designated "money path" must satisfy a checklist of properties: idempotency key, signature verification, amount unit correctness, retry safety, etc.** Today these are hunted manually in code review.

**Backlog evidence (5 open P0 issues, all variants of "money-path X missing safety property Y"):**
- **#3516** `fix(money-path): Jupiter swap executor signs API response without verification`
- **#3515** `fix(money-path): Bybit order submission missing orderLinkId (no idempotency key)`
- **#3514** `fix(money-path): Capital execute_with_retry can duplicate open_position on 401/4`
- **#3513** `fix(money-path): Jupiter swap executor hardcodes 1e9 lamports for every mint`
- **#3512** `fix(money-path): Jito bundle fallback can double-submit Jupiter perps order`

**cfdb solution:** label money-path items via `LABELED_AS Concept{name:'MoneyPath'}` (set by rule: "any function reachable from an `:EntryPoint` of kind `mcp`/`http` whose handler matches `*execute*` or `*submit*` and which calls into an adapter labeled `Exchange`"). Then enforce a per-money-path-item checklist as one Cypher query joining the label with required-property attributes.

This is the same composition shape as Pattern D and E — **the schema is rich enough that a single query language covers ban rules, required properties, and money-path safety with zero new verbs**.

### 3.7 Pattern G — Concept absence / missing related wiring

**A concept exists but related concepts that should always appear together don't.** "Operator override exists, but no expiry, no audit, no revalidation."

**Backlog evidence:**
- **#3526** `fix(risk): circuit_breaker operator_override has no expiry, no audit, no revalidation`. Four related concepts (override, expiry, audit, revalidation) that should co-occur. Three are missing in the operator-override path.

**cfdb query:** for each `:Item` labeled with concept X, check whether items in its `CALLS` reach contains items labeled with concepts {Y, Z, W}. Missing co-occurrence = missing wiring.

### 3.8 Pattern H — Graceful degradation / fallback path existence

**For each entry point that touches an external dependency, a fallback path must exist.** Currently checked at runtime when the dependency is down.

**Backlog evidence:**
- **#3437** `bug: MCP server hard-fails when TimescaleDB is unreachable (no graceful degradation)`. Entry points hit Timescale; no fallback branch.

**cfdb query:** for each `:EntryPoint` whose `CALLS*` reach a `:Item` labeled `TimescaleDB`, verify the call path contains a `Result::Err` branch handler. Reachability + branch-existence query on the call graph.

### 3.9 Pattern I — Bounded-context raid (plan validation)

**Architects refactor a dirty bounded context: clean blueprint, portage clean parts, rewrite glue, drop dirty parts.** The failure mode is missing a hidden caller and shipping the raid broken.

**Backlog evidence:**
- **#3593** `refactor(RFC-028-P3b): re-split tool routers by access tier`. Live raid in progress on the MCP tool routers. Exactly the use case for `/gate-raid-plan`.
- **#3580** `EPIC: MCP Runtime Extraction (RFC-028)`. The umbrella raid this falls under.

**cfdb query:** five compositions (completeness, dangling-drop, hidden-callers, missing-canonical, clean/dirty-mismatch) consumed via `query_with_input(...)` with the raid plan's portage/rewrite/glue/drop buckets passed as external sets. See PLAN-v1 §3.3 for full breakdown.

### 3.10 Summary — the 9 patterns share a shape

| # | Pattern | Verbs used | Backlog evidence |
|---|---|---|---|
| A | Horizontal split-brain | `query` | PR #3616 |
| B | Vertical split-brain / param ignored | `query` | #2651 |
| C | Canonical bypass / ad-hoc impl wiring | `query` | #3525, #3523, #3521 |
| D | Forbidden function in scoped crates | `query` | #3577, #3574, #3573, #3571, #3569, #3568, #3567 |
| E | Required property enforcement | `query` | #3576, #3577, #3515, #3514 |
| F | Money-path safety checklist | `query` | #3516, #3515, #3514, #3513, #3512 |
| G | Concept co-occurrence / missing wiring | `query` | #3526 |
| H | Graceful degradation / fallback path | `query` | #3437 |
| I | Bounded-context raid (plan validation) | `query_with_input` | #3593, #3580 |

**All 9 patterns use the same 2 query verbs.** None requires a new tool feature. Eight use `query()`; one (raid plan validation) uses `query_with_input()` because it joins external buckets against the graph.

This is the polyvalence claim, evidenced against real backlog: **the API stays at 11 verbs while the use cases scale linearly with the number of `.cypher` files**.

### 3.11 What changes in the qbot-core development loop

Today, each new RFC ships with:
1. A handwritten Rust architecture test (e.g. `architecture_test_banning_f64_in_domain.rs`)
2. A manual sweep PR fixing existing violations (e.g. #3567 sweep for f64)
3. A code-review burden to keep new code from re-introducing the violation

With cfdb, each RFC ships with:
1. **One `.cypher` file** in the project's rules directory
2. **Drift queries** in CI catch new violations at PR time (#3578 architecture-rfc-enforcement gate)
3. **Sweep PRs** are guided by `cfdb query` listing all current violations

This is not theoretical — issue **#3578** (`feat: architecture-rfc-enforcement CI gate — block PR merge on RFC violations`) is **literally a meta-issue requesting the substrate cfdb provides**. cfdb is the answer to #3578.

---

## 4. What cfdb is not

- **Not a linter.** Finds structural drift, not style issues.
- **Not a rewriter.** Read-only over source. cfdb never modifies Rust files.
- **Not an LSP server.** No IDE integration in v1.
- **Not language-agnostic.** Rust-only in v1; the schema is general enough to accept a Python or TypeScript extractor later.
- **Not a replacement for `cargo`, `clippy`, `rust-analyzer`, or `cargo-deny`.** It is additive.
- **Not a Python project.** No code carries forward from `.concept-graph/extract.py` or `query.py`.
- **Not tied to qbot-core.** Multi-workspace from day one.
- **Not an audit reporter.** Returns JSON; consumers format.
- **Not opinionated about workflows.** Knows nothing about "raids", "/prescribe", "RFCs". Those are consumer-side compositions.
- **Not a prompt builder.** The query verbs return raw qnames + structured attributes. cfdb never returns pre-formatted prompt fragments, never embeds model-family conventions, never knows what `/prescribe` will do with the result. Prompt construction is the consumer skill's job; cfdb provides the structured facts. (LLM specialist [LLM-Q2] — preserves §4 opinion-agnosticism and G1 determinism by keeping cfdb's output decoupled from any LLM model family.)

---

## 5. User stories

```
As an architect,
  I want to ask "is the Ledger concept split-brained?" (Pattern C)
  and get a list of parallel implementations, dead ones, canonical bypasses, and wiring mismatches in one query
  so I can scope a refactor properly. — issue #3525

As an agent answering a user question,
  I want to ask "where are the Kalman filters used?"
  and get a structured list of callers, type references, and call chains
  so I can answer authoritatively without re-reading files.

As a /prescribe skill,
  I want to ask "what is the canonical implementation of <concept> in the current workspace?"
  and get a specific qname to inject into a generation prompt
  so generated code wires to the existing canonical instead of inventing a parallel.

As a /gate-raid-plan skill,
  I want to validate a raid plan against the fact base (Pattern I)
  by passing in the portage/drop/glue/rewrite buckets as query inputs
  so the architect catches dangling references before any file moves. — issue #3593

As an RFC author writing a new ban rule (Pattern D),
  I want to express the ban as a single .cypher file
  instead of writing a Rust architecture test that walks the AST manually
  so RFC enforcement scales with one file per rule. — issues #3574, #3568, #3571

As the architecture-rfc-enforcement CI gate (Pattern D + E),
  I want to run all .cypher rules against the current branch's snapshot
  and fail the build if any rule returns a non-empty result
  so RFC violations are caught at PR time, not in code review. — issue #3578

As a money-path auditor (Pattern F),
  I want to ask "list every adapter on the money path that is missing an idempotency key, signature verification, or unit-correctness check"
  and get one report joining structural facts with policy attributes
  so I never miss another #3515-class bug. — issues #3512–#3516

As a developer running a weekly audit,
  I want to diff two cfdb snapshots across commits
  and see new violations introduced by the branch
  so drift is caught at PR time, not months later.

As an agent analyzing multiple projects (post-v0.1),
  I want to query across cfdb keyspaces for different workspaces
  to find where a concept is duplicated across qbot-core / orchestrator / dashboard.
```

---

## 6. API (the contract)

Full spec is in PLAN-v1 §6A. Summary for council:

**11 core verbs + 4 typed convenience verbs (council revision):**

```
INGEST    extract(workspace, keyspace)                       -> ExtractReport
          enrich_docs(keyspace)                              -> EnrichReport
          enrich_metrics(keyspace)                           -> EnrichReport
          enrich_history(keyspace)                           -> EnrichReport      // repo lookup via project registry — Clean [CLEAN-2]
          enrich_concepts(keyspace, rules)                   -> EnrichReport

RAW QUERY query_raw(keyspace, cypher, params, sets?)         -> {rows, warnings}  // renamed from query/query_with_input — Clean [CLEAN-1] + LLM
          // `sets` is optional (collapses query_with_input — SOLID [SOLID-1])
          // Returns {rows, warnings}: warnings catch label/attr/edge typos
          // and "schema mismatch" vs "empty result" — silent-empty was the
          // #1 LLM-consumer failure mode (LLM specialist findings)

TYPED     find_canonical(keyspace, concept)                  -> {rows, warnings}  // Pattern C
          list_callers(keyspace, qname)                      -> {rows, warnings}  // Kalman-callers
          list_violations(keyspace, rule_path)               -> {rows, warnings}  // Pattern D, E, F
          list_bypasses(keyspace, concept)                   -> {rows, warnings}  // Pattern C

SNAPSHOT  list_snapshots()                                   -> [keyspace,sha,ts,schema_v]
          diff(keyspace_a, keyspace_b, kinds)                -> {added,removed,changed}
          drop(keyspace)                                     -> ()

SCHEMA    schema_version(keyspace)                           -> SemVer
          schema_describe()                                  -> JSON               // per-attribute provenance — SOLID [SOLID-5]
```

**Total: 15 verbs (11 core + 4 typed).** Typed verbs are convenience composers built on `query_raw` — they exist to keep consumers off the Cypher dialect for the common cases, addressing Clean architect's [CLEAN-1] dialect-leak finding. Internally they reduce to one `query_raw` call; the schema and substrate are unchanged. The "verb count under 20" design principle holds.

**3 wire forms** (identical verb set across all three):

| Form | Best for | Latency |
|---|---|---|
| **CLI** (`cfdb query …`) | humans, scripts, ad-hoc audits | per-invocation cold start |
| **HTTP** (`POST /v1/query`) | skills running outside cfdb, latency-sensitive consumers | warm process, sub-second |
| **Rust lib** (`use cfdb::query;`) | tests, in-process composition, architecture tests in qbot-core | function call |

**5 determinism guarantees:**

```
G1. Same workspace SHA + same schema version  ->  byte-identical graph.
G2. query() is read-only. No query mutates the graph.
G3. enrich_*() is additive. No enrichment deletes structural facts.
G4. schema_version() is monotonic within a major: v1.1 graphs are queryable by v1.0 consumers.
G5. Snapshots are immutable. Once written, never rewritten in place — only dropped or replaced wholesale.
```

**Explicitly NOT in the API:** named queries per use case, output formatting, skill adapters, refactoring actions, auto-fix, multi-language support (v1), opinionated workflows, caching.

> **SOLID architect + Clean architect council lens:** is 11 the right number? Where does the orthogonality break (ISP)? Does the API leak any infrastructure concept into its surface (Clean Architecture dependency rule)? What's the smallest realistic use case (drawn from §3) that cannot be expressed via `query` or `query_with_input` over §7's schema? If you can name one, that's a 12th verb — and the 12th verb is a smell.

---

## 7. Fact schema (the data model)

Full spec in PLAN-v1 §6. Summary for council:

**Nodes:** `:Crate`, `:Module`, `:File`, `:Item`, `:Field`, `:Variant`, `:Param`, `:CallSite`, `:EntryPoint`, `:Concept`

**Edges (structural):** `IN_CRATE`, `IN_MODULE`, `HAS_FIELD`, `HAS_VARIANT`, `HAS_PARAM`, `TYPE_OF`, `IMPLEMENTS`, `IMPLEMENTS_FOR`, `RETURNS`, `SUPERTRAIT`

**Edges (call graph):** `CALLS`, `INVOKES_AT`, `RECEIVES_ARG`

**Edges (entry points):** `EXPOSES`, `REGISTERS_PARAM`

**Edges (concept overlay):** `LABELED_AS`, `CANONICAL_FOR`, `EQUIVALENT_TO`

**Edges (history, optional via Layer 2):** `INTRODUCED_IN`, `LAST_TOUCHED_BY`

**Attributes on `:Item`:** `qname`, `name`, `kind`, `crate`, `module_qpath`, `file`, `line`, `signature_hash`, `doc_text`, plus quality signals `unwrap_count`, `test_coverage`, `dup_cluster_id`, `cyclomatic`. The quality attributes are load-bearing for Pattern I (raid plan validation) — they let one query join structural facts with quality signals.

> **SOLID architect + CPO council lens:** does the schema in §7 cover all 9 patterns in §3? Map each pattern to the nodes/edges/attributes it touches. If any pattern needs something not in §7, that's a schema gap to flag before v0.1 starts. CPO sanity-check: are any of the 9 patterns over-engineered relative to the actual backlog evidence in §3.10?

### 7.1 Rust encoding — open newtypes, not typed enums (ratified 2026-04-13)

**Amendment status:** ratified 2026-04-13 after the #3624 scaffold + #3625 `schema_describe()` review. Supersedes the earlier implicit assumption (carried over from PLAN-v1 and encoded in the original #3625 AC) that each `:Label` in §7 would map to a per-label Rust struct and each edge type to a variant of a `Node` / `Edge` enum.

**The Rust-side encoding of §7 is deliberately *open* and *string-keyed*:**

- `Label` and `EdgeLabel` are `#[serde(transparent)]` newtype wrappers around `String`. The canonical label vocabulary (`CRATE`, `MODULE`, `ITEM`, `CALL_SITE`, …) lives as `pub const` values on the newtypes, not as enum variants.
- `Node { id, label, props: BTreeMap<String, PropValue> }` and `Edge { src, dst, label, props: BTreeMap<String, PropValue> }` are generic over the label. There is no per-label `struct Crate { … }`, no `struct CallSite { … }`, and no `enum Node { Crate(…), CallSite(…), … }`.
- Consumers that need compile-time safety for a specific label access attributes via `props.get("qname").and_then(PropValue::as_str)` — string-keyed, not field-typed.
- The **only** structured self-description of the schema at compile time is `cfdb_core::schema_describe()` → `SchemaDescribe`, which returns a runtime document listing every node label, edge label, attribute, and per-attribute `Provenance` (extractor / enrich_docs / enrich_metrics / enrich_history / enrich_concepts). Consumers introspect this at runtime to discover the vocabulary.

**Why the open shape, not the typed shape:**

1. **Cypher is the primary query language.** Every consumer class — CLI, HTTP, Rust lib, LLM / skill adapters, `.cypher` rule files — operates on a string-keyed graph (`n.callee_path`). If the Rust types were enum-typed per label, the two schemas (Rust field names vs. Cypher attribute names) would have to be kept in sync forever. That is split-brain by construction, and RFC-cfdb's whole §3 problem statement is split-brain elimination.
2. **Extensibility is load-bearing, not cosmetic.** The QA-5 spike (#3623) shipped twelve hours after the scaffold merged and added two new values to `CallSite.kind` (`fn_ptr`, `serde_default`) to cover patterns the initial extractor missed. Under an enum-variant shape, those would have been breaking changes forcing every consumer with an exhaustive `match` to update. Under the open shape, the extractor just started emitting them and nothing in core had to change. This extension rate is expected to continue through v0.1 / v0.2 as Patterns B/E/G/H/I come online.
3. **ISP compliance.** A 10-variant `Node` enum forces every consumer into an exhaustive `match`, pulling a dependency on all 10 labels whether they care or not. Open newtypes let each consumer depend only on the labels it queries against. Per the SOLID architect council lens (§1 table), this is the more ISP-compliant shape.
4. **Schema-version friendliness.** Minor-version additions (new attributes, new labels) are purely additive on the wire — `BTreeMap` ignores unknown keys on read, old queries keep working, new queries reference new keys. An enum-variant shape would require `#[serde(default)]` migration dances on every read, and type changes would silently drop data.
5. **One schema, one source.** The schema lives in `schema_describe()` and on the wire. Not in Rust types, not in `.cypher` files, not in prose documentation. One place to update.

**How the typed shape's guarantees are recovered:**

The open shape loses compile-time safety on attribute access. The mitigations for the two resulting failure modes are:

| Failure mode | Mitigation | Status |
|---|---|---|
| Extractor emits an attribute key that `schema_describe()` does not document (schema drift on the writer side) | **Forward consistency guard** — `cfdb-extractor` smoke-runs on a fixture workspace and asserts `{emitted attrs} ⊆ schema_describe().attributes[*]`. Failure names the offending `(label, key)` pair. | Filed as #3668 — ships before any consumer treats `schema_describe()` as contract. |
| Consumer query references an undocumented label / edge / attribute (schema drift on the reader side) | **Query-time schema validation** — the evaluator walks each parsed `Query` and emits `Warning` rows in the `{rows, warnings}` result for every undocumented reference, before running the query. Undocumented references become visible typos instead of silent empty results. | Filed as #3669. RFC §6A.1 already specifies the `{rows, warnings}` contract for exactly this reason — this is the implementation of that contract. |
| Forward + reverse guard together | `schema_describe_covers_all_edge_labels` in `cfdb-core::schema::tests` locks the reverse direction — every `EdgeLabel::*` const must appear in `schema_describe()`. Add a new const and the test fires. | ✅ Shipped in #3625. |

Together, (schema_describe as contract) + (forward/reverse guards) + (query-time validation) give the consumer equivalent end-to-end safety to a typed-Rust shape **without** duplicating the schema across the Rust type system and the Cypher vocabulary.

**What the council got wrong in the original PLAN-v1 text:**

PLAN-v1 §6.1 described the fact schema as a table of labels with attribute lists, and the readers of that table (both the council and the #3625 issue author) reasonably translated it into "one Rust struct per row." That translation was the Rust-default idiom but was never debated as a design choice — no council member argued *for* the typed shape because no council member raised the Cypher-alignment cost as a counter. The 2026-04-13 scaffold review surfaced the cost the moment the QA-5 spike proved extensibility matters in week one, and the cfdb council's §7 intent — "a vocabulary shared between the extractor, the parser, and the evaluator" — turned out to be better served by an open newtype than an enum. This amendment blesses that outcome formally so the RFC stops drifting against the implementation.

**What does not change:**

- The §7 label vocabulary and edge vocabulary are unchanged. New labels/edges still require a schema-version bump per §12 Q2.
- `SchemaVersion` is still semver-typed with G4 monotonic compatibility. v0.1.x schemas are queryable by v0.1.y consumers where y ≥ x.
- JSONL canonical fact format (§12.1) is unchanged — the wire form was already string-keyed and unaware of the Rust-side shape.
- Enrichment verbs (`enrich_docs`, `enrich_metrics`, `enrich_history`, `enrich_concepts`) are unchanged. `Provenance` in `schema_describe()` mirrors these 1:1.
- The PLAN-v1 §6.1 attribute table stays authoritative for *what* attributes exist on each label. This amendment is about *how* those attributes are encoded in Rust, not what they are.

---

## 8. Architecture

```
                       cfdb (Rust workspace)
   ┌──────────────────────────────────────────────────────────────┐
   │                                                              │
   │   ┌──────────────┐         ┌──────────────────────┐          │
   │   │ CLI (clap)   │         │ HTTP server (axum)   │          │
   │   │  cfdb extract│         │  POST /v1/query      │          │
   │   │  cfdb query  │         │  POST /v1/extract    │          │
   │   │  cfdb diff   │         │  GET  /v1/snapshots  │          │
   │   └──────┬───────┘         └──────────┬───────────┘          │
   │          │                            │                      │
   │          └────────────┬───────────────┘                      │
   │                       │                                      │
   │          ┌────────────▼─────────────┐                        │
   │          │  cfdb-core (library)     │   the 11-verb API      │
   │          │  extract, query,         │                        │
   │          │  enrich, diff, ...       │                        │
   │          └────────────┬─────────────┘                        │
   │                       │                                      │
   │   ┌───────────────────┼───────────────────┐                  │
   │   │                   │                   │                  │
   │   ▼                   ▼                   ▼                  │
   │ ┌────────────┐  ┌──────────────┐  ┌───────────────┐          │
   │ │ extractor  │  │ store        │  │ enrichers     │          │
   │ │ (syn +     │  │ (kuzu embed) │  │ (docs,        │          │
   │ │  cargo_    │  │              │  │  metrics,     │          │
   │ │  metadata) │  │              │  │  history,     │          │
   │ └────────────┘  └──────────────┘  │  concepts)    │          │
   │                                   └───────────────┘          │
   │                                                              │
   └──────────────────────────────────────────────────────────────┘
                              │
                              ▼
                  ┌──────────────────────────┐
                  │ Project registry         │
                  │  .cfdb/projects.toml     │
                  │  .cfdb/concepts/*.toml   │
                  │  .cfdb/rules/*.cypher    │
                  └──────────────────────────┘
```

### 8.1 Crate layout

cfdb lives under `qbot-core/.concept-graph/cfdb/` as a sub-Cargo-workspace (separate `Cargo.toml`, not part of the main qbot-core workspace). Per Q3 user resolution: in-tree now, extract to a stand-alone `yg/cfdb` repo when a second consumer project arrives.

```
qbot-core/.concept-graph/cfdb/
├── Cargo.toml                 # workspace root
├── crates/
│   ├── cfdb-core/             # 11-verb API (library)
│   ├── cfdb-schema/           # shared schema types, versioning
│   ├── cfdb-extractor/        # syn-based fact extraction
│   ├── cfdb-store/            # Kuzu wrapper + schema migrations
│   ├── cfdb-enrich-docs/      # /// extraction + LLM fallback
│   ├── cfdb-enrich-metrics/   # unwrap count, complexity, coverage
│   ├── cfdb-enrich-history/   # git-derived edges
│   ├── cfdb-enrich-concepts/  # rule-based label overlay
│   ├── cfdb-cli/              # clap binary
│   └── cfdb-server/           # axum HTTP binary
├── examples/queries/          # bundled Cypher example library (the §8 catalog of PLAN-v1)
│   ├── hsb-by-name.cypher
│   ├── vsb-multi-resolver.cypher
│   ├── kalman-callers.cypher
│   ├── ledger-split-brain.cypher
│   ├── arch-ban-utc-now.cypher
│   ├── arch-ban-f64-in-domain.cypher
│   ├── moneypath-checklist.cypher
│   └── raid-validate-*.cypher
└── docs/
    ├── README.md
    ├── API.md          # synced from RFC §6
    ├── SCHEMA.md       # synced from RFC §7
    └── ARCHITECTURE.md # synced from RFC §8
```

> **Clean architect + Rust guru coder council lens:** is the crate split right? `cfdb-extractor` and `cfdb-store` are the only crates with hard external dependencies (`syn` / `kuzu`); the rest are pure logic. Clean architect: is the 10-crate layout correctly hexagonal — does the dependency rule point only inward? Rust guru: is splitting into 10 crates idiomatic for a tool of this size, or fragmented? At what crate count does build time and dev-loop friction outweigh the boundary clarity?

---

## 9. Multi-project support

cfdb indexes multiple Rust workspaces via a **project registry**:

```toml
# .cfdb/projects.toml
[[project]]
name = "qbot-core"
path = "/home/yg/workspaces/qbot-core"
concept_rules = ".cfdb/concepts/qbot-core.toml"
arch_rules_dir = ".cfdb/rules/qbot-core/"

[[project]]
name = "orchestrator"
path = "/home/yg/workspaces/orchestrator"
concept_rules = ".cfdb/concepts/orchestrator.toml"
arch_rules_dir = ".cfdb/rules/orchestrator/"
```

**Per-project state:**

- **Keyspace naming:** `cfdb_<project>_<sha12>_<schema_major>_<schema_minor>`. Each project's snapshots are independent.
- **Concept rules** (`concept_rules`): per-project because vocabularies differ (qbot-core has `Ledger`/`Strategy`/`Position`; orchestrator has `Workflow`/`Runner`/`Step`).
- **Architecture rules** (`arch_rules_dir`): per-project because RFCs are per-project. qbot-core's `arch-ban-utc-now.cypher` lives here; orchestrator has its own bans.

**Cross-project queries are post-v0.1.** They require either a join layer (federated query) or a merged keyspace. Both are tractable but out of scope for v1.

> **Clean architect + QA tester council lens:** is the registry too rigid? Clean architect: is the project the right boundary, or should the boundary be the workspace (a project may have multiple workspaces)? QA tester: what happens when the same project lives in two checkouts on the same machine? When two projects share a concept (`Ledger` could exist in both qbot-core and quant-core)? When a project moves on disk between extractions? Each is a corner case that needs a defined behavior or an explicit "undefined" disclaimer.

---

## 10. Technical stack

| Concern | Choice | Rationale |
|---|---|---|
| Language | Rust 1.93 stable | User-mandated; native to analyzed workspaces; best AST tooling via `syn` |
| Workspace discovery | `cargo_metadata` crate | Authoritative source for Cargo workspaces |
| AST parsing | `syn` (full feature) | Industry standard; fast; full grammar; unambiguous parse |
| Cross-crate resolution | `syn` symbol table + `use` resolution for **Q1=(b) Pattern D only**; **`ra-ap-hir` is a Phase B *blocker*** for Patterns B/E/G/H/I, not a fallback | `syn` ceiling per Rust guru: ~70–80% item recall, ~40–60% call-edge recall. Enough to ship arch-ban-utc-now; insufficient for anything that needs method dispatch, macro expansion, or re-export chains |
| **Graph store** | **LadybugDB** (`lbug` crate, embedded, openCypher, cxx FFI) — *recommended* | **Kuzu was archived 2025-10-13** after Apple acquired Kùzu Inc.; the `kuzu` crate is frozen at v0.11.3. LadybugDB is the credible successor (fork by Kuzu co-founder Arun Sharma), active weekly–biweekly cadence Jan–Apr 2026. See §10.1. |
| **Canonical fact format** | **JSONL** (blake3-keyed, sorted by `(node_label, qname)` then `(edge_label, src_qname, dst_qname)`) | The graph store is a *cache*, not a fixture. Determinism is asserted on the JSONL dump, not the backend file. Aligns with qbot-core's CLAUDE.md "JSONL interchange" convention. |
| Query language | openCypher (subset both LadybugDB and DuckDB+DuckPGQ accept) | Standard; expressive enough for all 9 patterns in §3 |
| HTTP serving | `axum` + `tokio` | Standard async HTTP in Rust; minimal deps |
| CLI | `clap` v4 (derive macro) | Standard |
| Config | `toml` + `serde` | Project registry, concept rules |
| Logging / observability | `tracing` with stable target strings (NO Prometheus, NO OpenTelemetry — per qbot-core CLAUDE.md) | Project convention |
| Error handling | `thiserror` for library, `anyhow` for binary | Standard |
| Testing | `cargo test` + integration tests against a temp LadybugDB file *or* the JSONL canonical dump | No external services needed for tests; fixtures are JSONL (portable, diffable) not backend files |
| Content addressing | `blake3` for deterministic hashing | Fast, modern, keyed |
| CI | Gitea Actions on `qbot.lab` | Matches existing personal projects |
| License | Apache-2.0 (recommended) | Permissive; matches Rust ecosystem default |

### 10.1 Graph store decision (the load-bearing one) — **revised after council §14 Q2 vote**

**Status:** Kuzu (the prior recommendation) was archived 2025-10-13 after Apple acquired Kùzu Inc. The `kuzu` crate on crates.io is frozen at v0.11.3 (July 2025). No maintainer. No CVE response. **The original §10.1 recommendation was dead on arrival** — verified by Rust guru via web research during council review.

PLAN-v1 inherited **FalkorDB** (Redis module) from v0 because it was already deployed on LXC 501. That was fine for a single-project tool. It is **wrong for cfdb** because:

1. **Deployment friction.** A multi-project tool cannot require each developer to run a Redis server.
2. **Portability.** Snapshots should be diffable artifacts: movable between machines, committable as test fixtures, attachable to bug reports.
3. **Test isolation.** Integration tests should create and destroy graph stores per-test without a daemon.

**Revised recommendation (council Q2 vote outcome): LadybugDB primary, DuckDB+DuckPGQ documented plan B, store-agnostic core.**

#### The three-part decision

1. **Backend: LadybugDB** (`lbug` crate, currently v0.15.3, cxx-based FFI). Forked by Kuzu co-founder Arun Sharma; active weekly-to-biweekly release cadence Jan–Apr 2026 shipping MVCC fixes, CVE sweeps, `CREATE GRAPH`, parquet memory-corruption fixes, WASM/OPFS support. Pin to `lbug = "=0.15.x"` minor. **Caveat (council-flagged):** docs ~61% coverage, extensions rework ongoing, no on-disk format stability promise yet — treat the `.ldb` file as a **rebuildable cache, not a portable fixture**.

2. **Canonical fact format: JSONL** (blake3-keyed, sorted by `(node_label, qname)` then `(edge_label, src_qname, dst_qname)`). This is the **immutable, portable, diffable artifact**. Determinism (G1) is asserted by hashing the JSONL dump, not the backend file. Test fixtures are JSONL. Snapshots committed to repo are JSONL. The backend is a query accelerator over the JSONL canonical truth.

3. **Plan B: DuckDB + DuckPGQ.** DuckDB's DuckPGQ community extension ships SQL/PGQ (the SQL:2023 graph query standard, Cypher-inspired `MATCH` syntax). The 2025 SIGMOD `USING KEY` optimization fixes the recursive-CTE memory blowup that previously ruled DuckDB out. **All 9 patterns in §3 are expressible in SQL/PGQ**, including Pattern I variable-length paths. The `duckdb-rs` Rust binding is the most mature analytical-embedded crate in the ecosystem. If LadybugDB destabilizes, swapping is a `StoreBackend` impl change, not a query rewrite.

4. **Store-agnostic `cfdb-core`:** the 11 API verbs and §7 schema are defined against a `StoreBackend` trait in `cfdb-core`. `cfdb-store-lbug` and `cfdb-store-duckpgq` are sibling impl crates. `cfdb-core`'s `Cargo.toml` does **not** depend on `lbug`, `duckdb`, or `syn` — verified by an architecture test (Clean architect must-fix item).

**Alternatives considered:**

| Option | Verdict |
|---|---|
| **Kuzu** | **Rejected — archived 2025-10-13.** Apple acquisition; no maintainer; v0.11.3 frozen; format never stabilized. |
| **FalkorDB** (v0 inheritance) | Rejected: requires Redis daemon, breaks portability and multi-project goal. |
| **Neo4j** | Rejected: requires JVM + server, same portability problem. |
| **LadybugDB** (`lbug`) | **Selected primary.** See above. |
| **DuckDB + DuckPGQ** | **Selected plan B.** SQL/PGQ via community extension; 2025 USING KEY optimization is the unblocker; expressivity sufficient for all 9 patterns. |
| **petgraph + custom Cypher interpreter** | Rejected: implementing a Cypher parser/planner is a year of work. |
| **SQLite + adjacency tables** | Rejected: no graph query language at all; expressivity loss too big. |

#### Determinism implications (load-bearing)

Because the backend file is now a *cache*, not a *fixture*, **the determinism contract (G1) is asserted on the JSONL dump, not on the backend file**. See §12.1 for the concrete CI recipe. This is a substantive change from earlier drafts.

#### `syn` cross-crate ceiling — promoted to a Phase B blocker

Council-verified ceiling on `syn`-only extraction: **~70–80% item recall, ~40–60% call-edge recall**. This is enough for **Q1=(b) Pattern D arch-ban-utc-now only** — the simplest pattern, single-name `CALLS` lookup. It is **insufficient for Patterns B/E/G/H/I**, which require method dispatch, macro expansion, and re-export chain following.

**`ra-ap-hir` is therefore promoted from "fallback if blocked" to a hard Phase B dependency.** Q1=(a) Pattern C and Q1=(c) Pattern I cannot ship without it. The original §10 wording ("escalate only if blocked") understated the cost — Patterns B/E/G/H/I land in v0.2 *with* `ra-ap-hir`, not as add-ons.

> **Rust guru coder council lens (closed):** Q2 vote = ABSTAIN both Kuzu and FalkorDB. Counter-proposal adopted above (LadybugDB primary, DuckDB+DuckPGQ plan B, JSONL canonical, store-agnostic core). Open: defer the StoreBackend trait shape to a v0.1 design note before code starts.

---

## 11. Integration points (the consumers)

cfdb is consumed, not consuming. Every integration below is a Cypher composition against the same 11-verb API. **None extends cfdb.**

| Consumer | Wire form | Verbs | What they compose |
|---|---|---|---|
| `/prescribe` skill | HTTP | `query` | Canonical lookup by concept (Pattern C) |
| `/prepare-issue` skill | HTTP | `query` | Scope neighborhood — items near issue files |
| `/quality-architecture` skill | CLI | `query`, `diff` | HSB, hex-violation, layer audit (Pattern A) |
| `/gate-raid-plan` skill (new) | HTTP | `query_with_input` | 5-query plan validation (Pattern I) |
| `/port-epic` skill | CLI | `query` | Blast-radius + surface-map for archaeology |
| `/boy-scout` skill | CLI | `query` | Local drift near touched files |
| **architecture-rfc-enforcement CI gate (#3578)** | CLI | `query` (run all rules) | One Cypher per RFC ban rule (Patterns D, E, F) |
| Ad-hoc agents | HTTP | `query` | Arbitrary compositions: "where is X used?", "is Y duplicated?", "what calls Z?" |
| Rust unit tests in qbot-core | Rust lib | `query` | Replace handwritten architecture tests with declarative queries (Patterns D, E) |
| Weekly audit cron | CLI | `query`, `diff` | Batch markdown reports over HEAD snapshot |
| Drift gate at PR time | CLI | `diff` | PR comment listing new drift introduced by this branch |

**Key insight:** the architecture-rfc-enforcement CI gate (#3578) and the in-repo Rust architecture tests are both consumers, not features. cfdb does not "ship CI integration"; cfdb ships the API, and the CI wraps it. This is the orthogonality test — if the user can wire a new consumer in their own code without touching cfdb, the API is right.

> **LLM specialist (@anthropic) council lens:** the "Ad-hoc agents" row is the load-bearing path for the LLM-consumer angle, and the most under-specified in this RFC. Open questions for the LLM specialist to answer: (1) Can a Claude session running with the project's `CLAUDE.md` and ~50k tokens of working state actually compose Cypher against the §7 schema, or does it need a "Cypher cheat sheet for agents" doc bundled in `examples/queries/`? (2) What's the prompt-construction story for `/prescribe` grounding when it lands in v0.2 — does cfdb return raw `:Item` qnames or pre-formatted prompt fragments? (3) Should the bundled example library include "agent-grade" templates (parameterized Cypher with placeholder hints) alongside the human-grade ones? (4) Where does v0.3 LLM enrichment introduce risk that the Layer 1/Layer 2 firewall (no LLM in structural truth) gets blurred?

---

## 12. Determinism model

- Extraction is keyed by `(workspace SHA, schema major.minor)`. Same key → byte-identical **JSONL canonical dump** (the backend `.ldb` file is a *cache*, not the immutable artifact).
- Re-extracting the same key against an existing keyspace is a no-op (content-hash dedup, blake3-keyed).
- Snapshots are immutable. Drift queries (`diff`) address two keyspaces at different SHAs.
- Enrichments write additional attributes/edges but never delete structural facts. Re-running `enrich_metrics` updates metrics in place via content-hash matching.

### 12.1 Determinism CI recipe (the concrete check)

The one-line "byte-diff the Kuzu file" of earlier drafts was unverifiable as written (LadybugDB has page allocation, catalog timestamps, WAL scratch — none guaranteed byte-stable). The recipe is now defined on the **JSONL canonical dump**, not the backend file:

1. **Canonical dump format.** `cfdb dump --db <path> --keyspace <name>` emits one JSON object per node and edge, sorted by `(node_label, qname)` then `(edge_label, src_qname, dst_qname)`. Numeric and string fields ordered alphabetically within each object. UTF-8, LF newlines, no trailing whitespace. Each emitted object carries a `kind: "node" | "edge"` discriminator so a stream-parser does not need positional context. (The original RFC draft named this verb `cfdb export --format=sorted-jsonl`; the shipped CLI uses `cfdb dump` — a one-name verb is cleaner than a verb+flag pair with a single legal flag value, and the format itself is unaffected. Reconciled in #3630.)
2. **Two-run harness:**
   ```
   cfdb extract --workspace <ws> --db <db-A> --keyspace ks && cfdb dump --db <db-A> --keyspace ks | sha256sum > a.sha
   cfdb extract --workspace <ws> --db <db-B> --keyspace ks && cfdb dump --db <db-B> --keyspace ks | sha256sum > b.sha
   diff a.sha b.sha   # MUST be identical
   ```
3. **Extractor invariants enforced by architecture test:**
   - `BTreeMap` (not `HashMap`) in any collection that feeds the dump
   - Single-threaded write phase (no `rayon` in extractor write path)
   - `slice::sort` (stable), never `sort_unstable`, on any collection that feeds the dump
   - No system clock reads during extraction
4. **No pinned baseline.** G1 is a **consistency check, not a conformance check.** Two consecutive runs over the same workspace must produce byte-identical dumps — that proves determinism without a stored expectation. The RFC does **not** specify a pin file, a baseline sha, or a refresh recipe, and CLAUDE.md §6 rule 8 forbids them: ratchets are self-serve escape hatches that get regenerated whenever CI complains. Drift in the extractor is caught by the two-run compare in the same PR; drift in the fixture is visible via `git diff` on the fixture itself. (Earlier drafts and the shipping session for #3630 introduced a `fixtures/determinism-workspace-sha.txt` pin file + a `make refresh-determinism-baseline` target. Both were ripped out in a follow-up cleanup — they violated doctrine the moment they were written.)
5. **Regression coverage over time:** the harness lives at `.concept-graph/cfdb/ci/determinism-check.sh` and runs on every PR that touches `cfdb-extractor/`, `cfdb-petgraph/`, `cfdb-core/`, the harness itself, or the spike workspace under `spikes/qa5-utc-now/`. Path filters live in `.gitea/workflows/ci.yml` (`test-cfdb-determinism` job). The script performs a two-run G1 consistency check only. If consumers need a point-in-time reference dump they can generate it locally and diff `cfdb dump` output directly — the reference must not live in the repo.

This is non-negotiable because:
- Without G1, **drift queries are meaningless** (every diff would surface noise).
- Without G2, **queries are unsafe to share** between consumers.
- Without G3, **re-running enrichments is destructive**.
- Without G4, **every consumer breaks on every schema change**.
- Without G5, **queries are racy under concurrent ingest**.

---

## 13. v0.1 scope (first release)

**Goal:** ship the minimum API slice to validate cfdb against one real consumer use case from §11. Which use case is §14 Q1.

**In scope for v0.1:**

- `cfdb-core` library: all 11 verbs implemented (some return "not yet" for verbs that need enrichments)
- `cfdb-extractor`: syn-based extraction targeting structural nodes/edges from §7 for a single workspace
- `cfdb-store-lbug`: LadybugDB wrapper with schema versioning. Backend file is a *cache* of the JSONL canonical dump (§12.1).
- `cfdb-cli`: all verbs accessible via CLI
- `cfdb-server`: HTTP `POST /v1/query` endpoint (warm process, axum)
- Project registry (single project supported; multi-project file structure in place)
- Determinism guarantees G1, G2, G5 active in CI
- Integration with **one** chosen consumer (council picks, §14 Q1)
- Bundled query library in `examples/queries/`: ~8 Cypher files covering Patterns A, C, D (one ban rule), the kalman-callers example, and the ledger-split-brain example

**Out of scope for v0.1:**

- Call-graph extraction (`:CallSite`, `CALLS`, `INVOKES_AT`) — *unless* the chosen v0.1 consumer needs it (Pattern B / Pattern G / Pattern H all need it)
- Entry-point cataloging (`:EntryPoint`, `EXPOSES`) — same
- Enrichments (docs, metrics, history, concepts) — Phase B
- Multi-project queries (cross-keyspace) — v0.3+
- LLM enrichment — v0.3+
- IDE integration

**Acceptance gate for v0.1** (revised after council review — items 1, 2, 5 changed; item 6 promoted to headline):

1. **[Headline] Pattern D equivalence demo.** `arch-ban-utc-now.cypher` returns a result set **equivalent to or a superset of** the existing handwritten Rust architecture test (`architecture_test_banning_utc_now.rs`). The handwritten test is replaced by the `.cypher` rule and CI stays green. **Reference test must be AST-based** (not regex) to avoid macro-body false negatives. Extra true positives count as improvements, not failures.
2. **Extraction recall ≥ 95% per crate**, measured *as a set*, against `cargo public-api` (or `rustdoc --output-format json`) ground truth — **not** `rg -c` (count-based, misses macro-generated items, doesn't handle nested `pub`, doesn't follow re-exports). Recall = `|syn_items ∩ rustdoc_items| / |rustdoc_items|`. Macro-generated items (`define_id!` etc.) handled via the special-case audit list (§15 Risk 2).
3. **Determinism CI check passes** per the §12.1 recipe: same `(workspace SHA, schema major.minor)` → byte-identical sorted-jsonl dump across two consecutive extractions. The check diffs the JSONL dump, not the LadybugDB backend file.
4. **The chosen v0.1 consumer integration works end-to-end** against a real question from §3, with a specific observable: a CLI line, a CI line, or a PR comment. "Works end-to-end" without an observable is not a verifiable acceptance criterion.
5. **`kalman-callers.cypher` returns ≥3 known callers** of `qbot_indicators::kalman::*` against `rg` ground truth (smoke test for the ad-hoc agent path; "non-empty" was unfalsifiable).
6. **~~`ledger-split-brain.cypher` returns the actual #3525 finding~~** — **REMOVED pending Q7 resolution.** The smoke test depends on `CANONICAL_FOR` edges which are Layer 2 enrichment output, explicitly out-of-scope for v0.1. Q7 in §14 documents the SOLID vs QA disagreement on whether to (a) add a `cfdb-concepts-manual` sub-crate to v0.1 to load `.cfdb/concepts/qbot-core.toml` rules at extract time (SOLID Option 3), or (b) drop the Pattern C smoke test from v0.1 and defer to v0.2 with proper enrichment (QA position). **User must resolve Q7 before v0.1 starts.**
7. **QA-5 macro-spike contingency on Item 1.** Before v0.1 starts, a pre-flight spike must classify every `Utc::now()` call site in qbot-core into (a) direct call visible to syn, (b) inside macro body, (c) test-only. If category (a) does not cover ≥95% of total, then Risk 1/2 mitigations (`ra-ap-hir` escalation) must land **inside v0.1**, not v0.2 — see §10.1 syn ceiling discussion.
8. **Enriched horizontal split-brain rule** (promoted from example → gate item via issue #3675). `hsb-by-name.cypher` returns, in a single pass, rows of shape `(name, kind, n, crates[], qnames[], files[])` over every `struct` / `enum` / `trait` whose `name` + `kind` is declared in **more than one crate** (`count(DISTINCT a.crate) > 1`), excluding `is_test = true` items. The enriched `collect()` output eliminates the 58-candidate manual-triage loop that the original count-only version imposed — a reader can classify each candidate without running a follow-up query. The rule ships with a **triage note** warning readers that cross-crate candidates may be legitimate `context_homonym`s (routed to `/operate-module`) rather than `duplicated_feature`s (routed to `/sweep-epic`); bounded-context classification is v0.2 work per the RFC-029 v0.2 addendum §A2.2. Determinism validated via §12.1 (stable `ORDER BY n DESC, name ASC`). **Out-of-scope for Item 8:** signature-hash Jaccard clustering for synonym-renamed duplicates (`OrderStatus` vs `OrderState`), classification of each candidate into `duplicated_feature` / `context_homonym`, and visibility-column output (the v0.1 extractor only emits pub-visible items, so visibility is uniform).

> **QA tester + SOLID architect council lens (closed):** the original Item 5 was unverifiable under v0.1 scope. The split is now an explicit Q7 decision for the user to resolve. SOLID's Option 3 unblocks Pattern C in v0.1 by making concept rules a hand-authored Layer 1 input rather than an LLM-derived Layer 2 enrichment; QA's counter-position is that this introduces a bespoke file format and the cleaner move is to drop the smoke test and defer Pattern C entirely to v0.2.

> **QA tester + SOLID architect council lens:** v0.1 acceptance gate item 5 — does the schema in §7 contain enough to express ledger-split-brain.cypher? QA tester: if `CANONICAL_FOR` is set by Layer 2 enrichment and Layer 2 is out of scope for v0.1, how does the smoke test actually pass? SOLID architect: does that imply the extractor (Phase A) and the enricher (Phase B) violate SRP because the smoke test forces them to ship together? Resolve before v0.1 starts — this is §14 Q7.

---

## 14. Decisions for council

Each decision below is structured for parallel council deliberation. **Suggested owner role** in italics.

### Q1. Which consumer use case ships first? *(CPO + QA tester)* — **COUNCIL VOTE: (b), 6/6**

- **(a) Pattern C — ledger-split-brain** for `/quality-architecture` or `/prescribe`. Highest stakes (P0 bugs in #3525, #3523, #3521 today). Requires call-graph extraction in Phase A AND `ra-ap-hir` escalation (raises Phase A cost). Coupled to Q7 (Layer 2 enrichment dependency).
- **(b) Pattern D — arch-ban-utc-now** for the architecture-rfc-enforcement CI gate (#3578). Lowest schema cost (only `CALLS`-by-name). Replaces a handwritten Rust test 1:1 — clearest before/after demo. Highest backlog volume (7+ open issues). **Within `syn`-only ceiling per §10.1 Rust guru analysis.**
- **(c) Pattern I — raid plan validation** for `/gate-raid-plan` against the live #3593 raid. Highest leverage per use. Largest Phase A. Parallel risk: `/gate-raid-plan` skill does not exist yet (§15 Risk 8).

**Council vote: (b)** — unanimous across CPO, QA, LLM specialist, and implicit votes from Clean/SOLID/Rust guru. Reasoning converged on:
- Lowest schema cost (no enrichments, no ra-ap-hir for v0.1)
- 1:1 replacement of an existing artifact (`architecture_test_banning_utc_now.rs` → `arch-ban-utc-now.cypher`) — the clearest before/after demo for the user
- Unblocks the largest backlog category (7 open RFC sweep issues)
- Cashes out the strategic shift from O(reviewer-hours per PR) to O(one-time per RFC)
- Sidesteps Q7 entirely

**QA-imposed contingency:** vote (b) is contingent on the **QA-5 macro spike** showing ≥95% of `Utc::now()` call sites in qbot-core are direct-syn-visible. If the spike fails, the (b) vote stands but Risk 1/2 mitigations (`ra-ap-hir`) must move from v0.2 into v0.1 — significantly raising Phase A cost. Run the spike before committing.

### Q2. Graph store: Kuzu or FalkorDB? *(Rust guru coder)* — **COUNCIL VOTE: ABSTAIN both, switch to LadybugDB primary + DuckDB+DuckPGQ plan B**

The original Q2 framing was invalidated by web research during council review:

- **Kuzu — REJECTED.** Archived 2025-10-13 after Apple acquired Kùzu Inc. Crate frozen at v0.11.3. No maintainer. No CVE response. Format never stabilized.
- **FalkorDB — REJECTED.** Requires Redis daemon, breaks portability, blocks Q5 per-developer-local recommendation.
- **LadybugDB (`lbug` crate) — SELECTED PRIMARY.** Fork by Kuzu co-founder Arun Sharma, active weekly–biweekly cadence Jan–Apr 2026, cxx FFI, openCypher support. Caveat: format stability not yet promised — backend file is a *cache*, not a fixture. Pin `lbug = "=0.15.x"`.
- **DuckDB + DuckPGQ — SELECTED PLAN B.** SQL/PGQ (SQL:2023) via community extension; SIGMOD 2025 `USING KEY` optimization fixes recursive CTE memory; expressivity sufficient for all 9 patterns; `duckdb-rs` is the most mature analytical-embedded Rust binding.
- **JSONL canonical fact format — also SELECTED.** The immutable, portable, diffable artifact. Determinism is asserted on the JSONL dump (§12.1), not the backend.
- **Store-agnostic `cfdb-core` with `StoreBackend` trait** — `cfdb-core/Cargo.toml` does not depend on `lbug`, `duckdb`, or `syn`. Architecture test enforces.

See §10.1 (revised) for the full decision and rationale.

### Q3. Repo location *(Clean architect)* — **USER OVERRIDE: in-tree now, extract later**

- **(a) Stand-alone repo on `qbot.lab`** (e.g. `yg/cfdb`) — independent versioning, reusable across projects, can be forked by other users. **Council vote (Clean architect): this option.**
- **(b) In-tree under `qbot-core/.concept-graph/cfdb/`** as a sub-Cargo-workspace — proximity to first consumer, no separate CI, no separate Cargo workspace setup, lowest friction for v0.1. Extract to `yg/cfdb` cleanly via `git filter-repo` or `git mv` when a second consumer project actually arrives.
- **(c) In-tree forever** — locks cfdb to qbot-core's release cycle. Rejected — incompatible with the multi-project capability requirement.

**Resolution (user, post-vote): (b) in-tree now, extract later.** The "must work on multiple Rust projects" requirement is a *capability* (cfdb knows how to index any workspace), not a *repo-layout* requirement at v0.1. v0.1 only consumes qbot-core. Repo extraction is a tax to pay when the second consumer arrives, not on day one. Cargo workspace structure makes future extraction trivial.

**Trigger to revisit (= when to extract to `yg/cfdb`):**
- A second consumer project (orchestrator, qbot-dashboard, quant-core, ...) needs to depend on cfdb, OR
- The cfdb crate is published to crates.io, OR
- An external user wants to fork it.

Until then, cfdb lives in `qbot-core/.concept-graph/cfdb/` as a sub-Cargo-workspace.

### Q4. Naming *(CPO; low stakes)*

- `cfdb` — placeholder, descriptive but bland
- `graphtool` — too generic
- `workspace-index` — describes function not substance
- `codefacts` — clean, evocative, available on crates.io if we ever publish

**Recommendation: `cfdb`** for v0.1 (low-cost placeholder), revisit at v1.0 if rename matters.

### Q5. Where does cfdb run? *(Clean architect)*

- **Per-developer local instance** — each developer runs `cfdb-server` on their workstation, indexes their checkouts, serves their skills.
- **Shared homelab instance** — one `cfdb-server` on LXC, indexes all workspaces nightly, serves all developers and skills.
- **Hybrid** — local for fast queries, homelab for batch audits.

**Recommendation: per-developer local for v0.1.** Embedded Kuzu makes this cheap. Revisit at v0.3 when multi-project queries land.

### Q6. Schema governance *(SOLID architect)*

- Council-reviewed `SCHEMA.md` + Rust types derived from it.
- Major version bumps require explicit council approval.
- Minor versions are additive-only, ship without ceremony.
- Every `:Item` carries `schema_version` attribute so consumers can detect mismatches.

**Recommendation: yes, all four.** No alternative is on the table.

### Q7. v0.1 acceptance gate item 5 — does the schema cover ledger-split-brain without enrichments? *(QA tester + SOLID architect)* — **🟡 SPLIT VERDICT — USER MUST RESOLVE**

The two co-owners reached opposite conclusions. Both are coherent. The user picks.

**SOLID architect verdict: Option 3 — ship `cfdb-concepts-manual` sub-crate in v0.1.**
- Add a new sub-crate that loads `.cfdb/concepts/<project>.toml` rule files at extract time
- Concept rules become hand-authored Layer 1 metadata, NOT LLM-derived Layer 2 enrichment
- Preserves the Layer 1 / Layer 2 firewall (TOML rules are human-authored and deterministic)
- Unblocks Q1=(a) Pattern C as a viable v0.1 pick — gives CPO/user freedom to pick the highest-stakes use case
- Enables the §13 Item 5 smoke test (`ledger-split-brain.cypher` against #3525) to pass on v0.1 output
- The RFC's own §6 `enrich_concepts(keyspace, rules)` signature already acknowledges concept enrichment is rule-fed; "the RFC is arguing against itself" in recommending Option 2

**QA tester verdict: drop §13 Item 5 from v0.1 gate; pick Q1=(b); defer Pattern C to v0.2.**
- Option 3 introduces a bespoke `.cfdb/concepts/*.toml` file format not in §6/§7 — adds API surface
- Acceptance tests cannot depend on out-of-scope infrastructure (Layer 2 enrichment is explicitly out of v0.1)
- Rejected Option 2 (RFC's own recommendation) as architecturally clean but strategically wrong
- The cleaner move: drop the smoke test, ship Pattern D (Q1=(b)) on its own merits, deliver Pattern C in v0.2 when enrichments land properly
- v0.1 gate now reads: §13 items 1, 2, 3, 4, 5, 7 (Item 6 became Item 1 in the headline promotion; Item 5 removed)

**Implications:**
- Picking SOLID's Option 3 unblocks Q1=(a) Pattern C as a contender for the v0.1 use case (currently §14 Q1 is still voted (b) by all six specialists, but Q7 resolution could re-open it)
- Picking QA's drop-Item-5 keeps Q1=(b) firmly as the v0.1 use case (current state) and removes the Pattern C smoke test from acceptance
- **Both options agree §13 Item 5 cannot stand as originally written.**

**The user's call. Until resolved, the RFC ships with Item 5 marked REMOVED-PENDING-Q7 in §13 and Q1=(b) standing.**

### Q8. Council composition for *future* RFCs *(CPO; meta)* — **COUNCIL VOTE: 6-role default**

**Errata:** earlier drafts said "5-role council" — drafting bug. §1 has 6 roles (CPO, Clean architect, SOLID architect, Rust guru, LLM specialist, QA tester). The "5" was a typo from before LLM specialist was added. Corrected.

**Vote:** keep the 6-role default for cfdb-related RFCs and any substrate-level RFC that touches product/architecture/Rust/testing/LLM-consumer dimensions. Per-RFC augmentation rules:
- **Add security specialist** for any auth/credential/money-path RFC
- **Add data-engineering specialist** for any RFC touching migrations or schema evolution
- **Add domain SME** (user-in-the-loop, no agent seat) for any RFC touching trading primitives
- **Add ops/homelab specialist** for any RFC touching LXC/Gitea/Redis/WireGuard layout
- **Drop roles** for docs-only or pure bug-fix RFCs (council overhead exceeds value)
- **Meta-gate:** any RFC proposing to change council composition must name the gap (what section is unreviewed?) rather than adding roles prophylactically. Council bloat is the failure mode.

---

## 15. Risks

1. **`syn` cannot fully resolve cross-crate calls.** Mitigation: emit unresolved as `:Symbol` placeholders; escalate to `ra-ap-hir` if coverage proves insufficient. (PLAN-v1 Risk 1.)
2. **Macro-defined items are invisible to `syn`.** Mitigation: special-case known internal macros (`define_id!`); audit via `rg 'macro_rules!.*pub (struct|enum)'`. (PLAN-v1 Risk 2.)
3. **~~Kuzu maturity~~ → resolved during council review: Kuzu archived 2025-10-13.** Apple acquired Kùzu Inc.; the `kuzu` crate is frozen; no maintainer. Resolution baked into §10.1: LadybugDB (`lbug`) primary, DuckDB+DuckPGQ documented plan B, JSONL canonical fact format, store-agnostic `cfdb-core` with `StoreBackend` trait. Backend file is now treated as a *cache* (rebuildable from JSONL), not a fixture. The "trait-abstract the store" mitigation that was originally a future intention is now mandatory and ships in v0.1.
4. **Schema churn.** v0.1's schema will prove wrong somewhere. Mitigation: schema versioning from day one, migration tooling in v0.2, breaking changes are major-version events.
5. **Determinism regression.** Easy to break via HashMap iteration, parallel writes, unstable sorts. Mitigation: G1 is verified by a CI determinism check.
6. **Multi-project scope creep.** v0.1 line must hold. Mitigation: cross-project queries explicitly out of v0.1 AC.
7. **Council-team coordination overhead.** Agent teams use significantly more tokens than single sessions. Mitigation: scope §1's review to specific sections per role; converge via §14 decisions, not free-form discussion.
8. **`/gate-raid-plan` skill does not exist yet.** If Q1=(c) is picked, building cfdb and the new skill in parallel doubles risk. Mitigation: ship Q1=(b) first, build `/gate-raid-plan` against a working cfdb in v0.2.

---

## 16. Post-v0.1 roadmap (council should approve direction, not specifics)

| Version | Feature | Pattern unlocked |
|---|---|---|
| **v0.2** | Call-graph + entry-point extraction | Patterns B, F, G, H |
| **v0.2** | Enrichment passes (docs, metrics, history) | Pattern I (raid plan signals) |
| **v0.2** | Drift-at-PR gate | Catches new violations of Patterns D/E at PR time |
| **v0.2** | `/gate-raid-plan` skill (consumes cfdb) | Pattern I |
| **v0.3** | Concept label overlay + synonym rules | Pattern A (synonym detection) |
| **v0.3** | Multi-project cross-keyspace queries | Cross-repo HSB |
| **v0.3** | LLM enrichment (concept description, duplicate labeling) | Layer 2 overlay only |
| **v0.4** | Embedding-based semantic clustering | Pattern A (catches what hashing misses) |
| **v0.5** | Query-recording: agents' queries → catalog examples | Auto-grow the bundled example library |
| **v1.0** | Schema v2: whatever v0.1–v0.5 revealed was missing | Major version bump |

The roadmap is **optional**; v0.1 must stand alone. Each post-v0.1 version is independently valuable.

---

## 17. References

- **Council mechanism:** Claude Code agent teams docs (`https://code.claude.com/docs/en/agent-teams`) — experimental feature, requires `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`, requires Claude Code v2.1.32+
- **Background plan:** `.concept-graph/PLAN-v1-code-facts-database.md` — full architectural plan, schema, build phases (this RFC supersedes it but keeps it as the long-form source)
- **v0 retrospective:** `.concept-graph/README.md` — LLM-based concept graph lessons
- **v0 audit:** `.concept-graph/phase3-audit.md` — first-pass findings on qbot-core
- **CLAUDE.md §7** — Param-Effect Canary rule (runtime version of Pattern B)
- **CLAUDE.md observability rules** — no Prometheus, no OpenTelemetry, `tracing` events with stable target strings
- **Reference architectures:** Glean (Meta, open source), CodeQL (GitHub) — same shape, different scope
- **Storage primary:** LadybugDB (`lbug` crate on crates.io) — embedded graph database with openCypher support, fork by Kuzu co-founder Arun Sharma. Active weekly–biweekly cadence Jan–Apr 2026.
- **Storage plan B:** DuckDB + DuckPGQ extension — SQL/PGQ (SQL:2023 graph query standard) with the 2025 SIGMOD `USING KEY` optimization that fixes recursive-CTE memory blowup.
- **Storage rejected:** Kuzu — archived 2025-10-13 after Apple acquired Kùzu Inc.; the `kuzu` crate is frozen at v0.11.3 (July 2025). Verified by Rust guru via web research during council review on 2026-04-13.
- **AST primary:** `syn` (full feature) — sufficient for Pattern D arch-ban-utc-now (Q1=(b)) only.
- **AST Phase B blocker:** rust-analyzer `ra-ap-*` crates — `syn`-only ceiling per council analysis is ~70–80% item recall, ~40–60% call-edge recall. Patterns B/E/G/H/I require `ra-ap-hir` and it ships in v0.2 as a hard dependency, not a fallback.
- **Council review record:** see `~/.claude/teams/cfdb-council/config.json` and `~/.claude/tasks/cfdb-council/` for the 6 specialist reviews that drove the §10.1 / §12.1 / §13 / §14 revisions.

**Backlog issues referenced in §3 (qbot.lab:3000/yg/qbot-core/issues):**

- **Pattern A:** PR #3616 (closed, fixed)
- **Pattern B:** #2651
- **Pattern C:** #3525, #3523, #3521
- **Pattern D:** #3577, #3574, #3573, #3571, #3569, #3568, #3567
- **Pattern E:** #3576, #3577, #3515, #3514
- **Pattern F:** #3516, #3515, #3514, #3513, #3512
- **Pattern G:** #3526
- **Pattern H:** #3437
- **Pattern I:** #3593, #3580
- **Meta-issue:** #3578 (architecture-rfc-enforcement CI gate — the meta-request cfdb answers)

---

## Addendum A — v0.1 minor schema bump (issue #3727, 2026-04-14)

Per council-cfdb-wiring `RATIFIED.md §B.1` (same-day addendum), four strictly **additive** extensions land in v0.1 without incrementing the schema version:

1. **`Item.is_test: bool`** — extended to recognise the bare `#[test]` function attribute in addition to the existing `#[cfg(test)]` module path (`attrs_contain_hash_test` helper alongside `attrs_contain_cfg_test`). The Item `NodeLabelDescriptor` now declares the attribute, closing the pre-existing asymmetry with the already-declared CallSite descriptor.
2. **`Item.bounded_context: String`** — stamped at extraction time (syn-level, **NOT** post-extraction enrichment) via `cfdb_extractor::context::compute_bounded_context`, with override support from `.cfdb/concepts/<name>.toml` files under the workspace root.
3. **`:Context {name, canonical_crate?, owning_rfc?}` node label** — new 11th well-known label added as `pub const CONTEXT` on the `Label` newtype (open-newtype encoding per §7.1).
4. **`(:Crate)-[:BELONGS_TO]->(:Context)` edge label** — new structural edge added as `pub const BELONGS_TO` on the `EdgeLabel` newtype.

**Additive-only guarantee.** No existing fields, labels, or edges are removed or renamed. The `SchemaVersion` constant stays at `V0_1_0`. The G1 two-run determinism invariant (§12.1) is preserved: the new resolution uses `BTreeMap` + sorted directory iteration; the architecture test `cfdb-extractor/tests/architecture_determinism.rs` stays green; the `self_workspace.rs` regression passes byte-identically across two consecutive runs; the `wire_form_15_verbs` test and the `cfdb-recall` rustdoc-recall ≥95% gate remain green. All parity tests (`schema_describe_covers_all_node_labels`, `schema_describe_covers_all_edge_labels`) have been updated to enumerate the new label + edge exactly once each.

---

**End of RFC.** Council convenes per §1; decisions are §14; convergence target is a vote on §14 plus a "must-fix before v0.1" list. On council acceptance, work begins with the cfdb workspace scaffold + the chosen Q1 use case integration.
