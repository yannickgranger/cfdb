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

The 6 roles cover product strategy, two architectural lenses (Clean and SOLID — intentionally adversarial to each other), implementation-language fluency, the LLM-consumer angle (this is a tool LLM-driven skills will use, so an Anthropic-side specialist is load-bearing), and end-to-end testability. **Domain-specific target-workspace knowledge stays with the user**; it does not need its own council seat — the user is in the loop for any backlog-grounded question.

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

**Multi-project from day one.** cfdb indexes any Rust workspace, not just the target workspace. A project registry (`.cfdb/projects.toml`) lists the workspaces to ingest; each gets its own keyspace. Cross-project queries are post-v0.1.

**No Python carries forward.** `extract.py`, `query.py`, `weekly-audit.py` are archived as v0 reference. cfdb starts fresh in Rust.

---

## 3. Problems cfdb solves (with backlog evidence)

Earlier drafts of this plan listed 4 problems (HSB, VSB, raid, Kalman/Ledger questions). The the motivating P0/P1 backlog reveals **9 distinct patterns**. Each pattern below is a class of bug that recurs across issues; cfdb expresses each as a Cypher composition over the §7 schema, replacing what is currently handwritten Rust architecture tests, manual grep audits, or "found this in code review" one-offs.

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

### 3.11 What changes in the consuming workspace development loop

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
- **Not tied to the target workspace.** Multi-workspace from day one.
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
  to find where a concept is duplicated across workspace-A / workspace-B / workspace-C.
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
| **Rust lib** (`use cfdb::query;`) | tests, in-process composition, architecture tests in a consuming workspace | function call |

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

cfdb lives under `<consuming-project>/.concept-graph/cfdb/` as a sub-Cargo-workspace (separate `Cargo.toml`, not part of the main target workspace). Per Q3 user resolution: in-tree now, extract to a stand-alone `yg/cfdb` repo when a second consumer project arrives.

```
<consuming-project>/.concept-graph/cfdb/
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
name = "alpha"
path = "/path/to/alpha"
concept_rules = ".cfdb/concepts/alpha.toml"
arch_rules_dir = ".cfdb/rules/alpha/"

[[project]]
name = "beta"
path = "/path/to/beta"
concept_rules = ".cfdb/concepts/beta.toml"
arch_rules_dir = ".cfdb/rules/beta/"
```

**Per-project state:**

- **Keyspace naming:** `cfdb_<project>_<sha12>_<schema_major>_<schema_minor>`. Each project's snapshots are independent.
- **Concept rules** (`concept_rules`): per-project because vocabularies differ (one project may have `Ledger`/`Strategy`/`Position`; another has `Workflow`/`Runner`/`Step`).
- **Architecture rules** (`arch_rules_dir`): per-project because RFCs are per-project. Each project carries its own ban rules (e.g. `arch-ban-utc-now.cypher`).

**Cross-project queries are post-v0.1.** They require either a join layer (federated query) or a merged keyspace. Both are tractable but out of scope for v1.

> **Clean architect + QA tester council lens:** is the registry too rigid? Clean architect: is the project the right boundary, or should the boundary be the workspace (a project may have multiple workspaces)? QA tester: what happens when the same project lives in two checkouts on the same machine? When two projects share a concept name? When a project moves on disk between extractions? Each is a corner case that needs a defined behavior or an explicit "undefined" disclaimer.

---

## 10. Technical stack

| Concern | Choice | Rationale |
|---|---|---|
| Language | Rust 1.93 stable | User-mandated; native to analyzed workspaces; best AST tooling via `syn` |
| Workspace discovery | `cargo_metadata` crate | Authoritative source for Cargo workspaces |
| AST parsing | `syn` (full feature) | Industry standard; fast; full grammar; unambiguous parse |
| Cross-crate resolution | `syn` symbol table + `use` resolution for **Q1=(b) Pattern D only**; **`ra-ap-hir` is a Phase B *blocker*** for Patterns B/E/G/H/I, not a fallback | `syn` ceiling per Rust guru: ~70–80% item recall, ~40–60% call-edge recall. Enough to ship arch-ban-utc-now; insufficient for anything that needs method dispatch, macro expansion, or re-export chains |
| **Graph store** | **LadybugDB** (`lbug` crate, embedded, openCypher, cxx FFI) — *recommended* | **Kuzu was archived 2025-10-13** after Apple acquired Kùzu Inc.; the `kuzu` crate is frozen at v0.11.3. LadybugDB is the credible successor (fork by Kuzu co-founder Arun Sharma), active weekly–biweekly cadence Jan–Apr 2026. See §10.1. |
| **Canonical fact format** | **JSONL** (blake3-keyed, sorted by `(node_label, qname)` then `(edge_label, src_qname, dst_qname)`) | The graph store is a *cache*, not a fixture. Determinism is asserted on the JSONL dump, not the backend file. JSONL is portable, diffable, and streamable. |
| Query language | openCypher (subset both LadybugDB and DuckDB+DuckPGQ accept) | Standard; expressive enough for all 9 patterns in §3 |
| HTTP serving | `axum` + `tokio` | Standard async HTTP in Rust; minimal deps |
| CLI | `clap` v4 (derive macro) | Standard |
| Config | `toml` + `serde` | Project registry, concept rules |
| Logging / observability | `tracing` with stable target strings (NO Prometheus, NO OpenTelemetry) | Project convention |
| Error handling | `thiserror` for library, `anyhow` for binary | Standard |
| Testing | `cargo test` + integration tests against a temp LadybugDB file *or* the JSONL canonical dump | No external services needed for tests; fixtures are JSONL (portable, diffable) not backend files |
| Content addressing | `blake3` for deterministic hashing | Fast, modern, keyed |
| CI | Self-hosted Gitea Actions | Matches existing personal projects |
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
| Rust unit tests in the target workspace | Rust lib | `query` | Replace handwritten architecture tests with declarative queries (Patterns D, E) |
| Weekly audit cron | CLI | `query`, `diff` | Batch markdown reports over HEAD snapshot |
| Drift gate at PR time | CLI | `diff` | PR comment listing new drift introduced by this branch |
| **`check-prelude-consistency` skill (qbot-core)** | CLI | `check-predicate` | Non-negotiable predicate library at `.cfdb/predicates/*.cypher` — one file per Non-negotiable, deterministic binary exit per predicate. Added by RFC-034 (Slices 1–5, issues #145–#149). See [`docs/query-dsl.md`](./query-dsl.md) for the user guide. |

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
6. **~~`ledger-split-brain.cypher` returns the actual #3525 finding~~** — **REMOVED pending Q7 resolution.** The smoke test depends on `CANONICAL_FOR` edges which are Layer 2 enrichment output, explicitly out-of-scope for v0.1. Q7 in §14 documents the SOLID vs QA disagreement on whether to (a) add a `cfdb-concepts-manual` sub-crate to v0.1 to load `.cfdb/concepts/<project>.toml` rules at extract time (SOLID Option 3), or (b) drop the Pattern C smoke test from v0.1 and defer to v0.2 with proper enrichment (QA position). **User must resolve Q7 before v0.1 starts.**
7. **QA-5 macro-spike contingency on Item 1.** Before v0.1 starts, a pre-flight spike must classify every `Utc::now()` call site in the target workspace into (a) direct call visible to syn, (b) inside macro body, (c) test-only. If category (a) does not cover ≥95% of total, then Risk 1/2 mitigations (`ra-ap-hir` escalation) must land **inside v0.1**, not v0.2 — see §10.1 syn ceiling discussion.
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

**QA-imposed contingency:** vote (b) is contingent on the **QA-5 macro spike** showing ≥95% of `Utc::now()` call sites in the target workspace are direct-syn-visible. If the spike fails, the (b) vote stands but Risk 1/2 mitigations (`ra-ap-hir`) must move from v0.2 into v0.1 — significantly raising Phase A cost. Run the spike before committing.

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

- **(a) Stand-alone repo** (e.g. `yg/cfdb`) — independent versioning, reusable across projects, can be forked by other users. **Council vote (Clean architect): this option.**
- **(b) In-tree under `<consuming-project>/.concept-graph/cfdb/`** as a sub-Cargo-workspace — proximity to first consumer, no separate CI, no separate Cargo workspace setup, lowest friction for v0.1. Extract to `yg/cfdb` cleanly via `git filter-repo` or `git mv` when a second consumer project actually arrives.
- **(c) In-tree forever** — locks cfdb to the target workspace's release cycle. Rejected — incompatible with the multi-project capability requirement.

**Resolution (user, post-vote): (b) in-tree now, extract later.** The "must work on multiple Rust projects" requirement is a *capability* (cfdb knows how to index any workspace), not a *repo-layout* requirement at v0.1. v0.1 only consumes the target workspace. Repo extraction is a tax to pay when the second consumer arrives, not on day one. Cargo workspace structure makes future extraction trivial.

**Trigger to revisit (= when to extract to `yg/cfdb`):**
- A second consumer project (orchestrator, qbot-dashboard, quant-core, ...) needs to depend on cfdb, OR
- The cfdb crate is published to crates.io, OR
- An external user wants to fork it.

Until then, cfdb lives in `<consuming-project>/.concept-graph/cfdb/` as a sub-Cargo-workspace.

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
- **v0 audit:** `.concept-graph/phase3-audit.md` — first-pass findings on the target workspace
- **CLAUDE.md §7** — Param-Effect Canary rule (runtime version of Pattern B)
- **CLAUDE.md observability rules** — no Prometheus, no OpenTelemetry, `tracing` events with stable target strings
- **Reference architectures:** Glean (Meta, open source), CodeQL (GitHub) — same shape, different scope
- **Storage primary:** LadybugDB (`lbug` crate on crates.io) — embedded graph database with openCypher support, fork by Kuzu co-founder Arun Sharma. Active weekly–biweekly cadence Jan–Apr 2026.
- **Storage plan B:** DuckDB + DuckPGQ extension — SQL/PGQ (SQL:2023 graph query standard) with the 2025 SIGMOD `USING KEY` optimization that fixes recursive-CTE memory blowup.
- **Storage rejected:** Kuzu — archived 2025-10-13 after Apple acquired Kùzu Inc.; the `kuzu` crate is frozen at v0.11.3 (July 2025). Verified by Rust guru via web research during council review on 2026-04-13.
- **AST primary:** `syn` (full feature) — sufficient for Pattern D arch-ban-utc-now (Q1=(b)) only.
- **AST Phase B blocker:** rust-analyzer `ra-ap-*` crates — `syn`-only ceiling per council analysis is ~70–80% item recall, ~40–60% call-edge recall. Patterns B/E/G/H/I require `ra-ap-hir` and it ships in v0.2 as a hard dependency, not a fallback.
- **Council review record:** see `~/.claude/teams/cfdb-council/config.json` and `~/.claude/tasks/cfdb-council/` for the 6 specialist reviews that drove the §10.1 / §12.1 / §13 / §14 revisions.

**Backlog issue numbers referenced in §3 (internal tracker of the target workspace):**

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
2. **`Item.bounded_context: String`** — stamped at extraction time (syn-level, **NOT** post-extraction enrichment) via `cfdb_concepts::compute_bounded_context` (shared `cfdb-concepts` crate, Issue #3 extraction), with override support from `.cfdb/concepts/<name>.toml` files under the workspace root.
3. **`:Context {name, canonical_crate?, owning_rfc?}` node label** — new 11th well-known label added as `pub const CONTEXT` on the `Label` newtype (open-newtype encoding per §7.1).
4. **`(:Crate)-[:BELONGS_TO]->(:Context)` edge label** — new structural edge added as `pub const BELONGS_TO` on the `EdgeLabel` newtype.

**Additive-only guarantee.** No existing fields, labels, or edges are removed or renamed. The `SchemaVersion` constant stays at `V0_1_0`. The G1 two-run determinism invariant (§12.1) is preserved: the new resolution uses `BTreeMap` + sorted directory iteration; the architecture test `cfdb-extractor/tests/architecture_determinism.rs` stays green; the `self_workspace.rs` regression passes byte-identically across two consecutive runs; the `wire_form_15_verbs` test and the `cfdb-recall` rustdoc-recall ≥95% gate remain green. All parity tests (`schema_describe_covers_all_node_labels`, `schema_describe_covers_all_edge_labels`) have been updated to enumerate the new label + edge exactly once each.

---

## Addendum B — v0.2 scope expansion + Rescue Mission Protocol (issue #51, merged 2026-04-23)

**Status:** Council-approved 2026-04-14 (SECOND-PASS GREEN), merged into parent per issue #51 on 2026-04-23. Formerly lived at `docs/RFC-cfdb.md`.
**Council verdict:** YELLOW → GREEN after second pass. 4 reviewers (clean-arch, ddd-specialist, rust-systems, solid-architect); 7 fixes applied in revision 1.

**Change log — revision 1 (pre-merge):**

| Fix | Section | Council source | Summary |
|---|---|---|---|
| #1 | §A2.2, §A2.3 | clean-arch WARN-2, solid-architect BLOCK-1 | Strip `fix_skill` from `:Finding` schema; move to external `SkillRoutingTable` (DIP-clean) |
| #2 | §A2.2 | clean-arch BLOCK-1 | Recast classifier from "a `.cypher` rule" to enrichment passes + Cypher query joining on enriched graph |
| #3 | §A3.3 | clean-arch BLOCK-2, solid-architect BLOCK-2 | cfdb returns structured inventory only; `/operate-module` formats raid plan markdown (parent RFC §4 invariant) |
| #4 | §A3.4 | solid-architect BLOCK-2 | `/operate-module` SRP-split from 4 to 2 responsibilities (threshold + raid plan), other concerns route externally |
| #5 | §A2.1, §A2.2, §A3.2 | ddd-specialist BLOCK-1 | Added 6th class "Context Homonym"; Context Mapping fix strategy, not mechanical sweep |
| #6 | §A3.2, §A3.3 | ddd-specialist BLOCK-2 | Scope `/operate-module` + raid plan to bounded contexts, not crates; context identification via crate-prefix heuristic + `.cfdb/concepts/*.toml` override |
| #7 | §A1.2, §A1.5, §A6 | rust-systems B1/B2/B3, clean-arch WARN-1 | Reframe `ra-ap-hir` as new parallel `cfdb-hir-extractor` crate (not upgrade); split compile/runtime costs; weekly upgrade tax acknowledged as budgeted cost; boundary architecture test; rust-version floor pin |

---


### A0. Why this addendum exists

v0.1 of cfdb as locked in RFC-029 §13 ships **one** Pattern D rule + **one** CI gate. The triple objective the user holds for the tool is larger than that:

1. **Horizontal split-brain** — same concept duplicated across crates (covered partially in v0.1 as `hsb-by-name.cypher` example, not as a gate item)
2. **Vertical split-brain** — trace from user-facing entry point DOWN through the call chain to identify unwired logic, duplicate implementations, and unclear paths (Pattern B in §3.2, **structurally blocked** in v0.1 per §13 because `:EntryPoint` + `CALLS` edges + `ra-ap-hir` are explicitly out of scope)
3. **Debt-causation taxonomy** — classify findings into root causes (duplicated feature / unfinished refactoring / random scattering / canonical bypass / unwired logic) so they can be acted on with different strategies (`/sweep-epic` vs `/port-epic` vs `/boy-scout`). This is **absent** from RFC-029 entirely — the RFC taxonomizes by *query shape*, not by *root cause*.

Beyond the triple objective, the user has framed a **project rescue mission** dimension:

> "If a module / crate is too infected, we will operate, slice it and make a drop-in replacement by portage of code in a clean prepared architecture → architects define RFC, coders transfert viable code, remove the cancer code … what we usually did: decide one head of hydra to keep and chop the others / rewire but we were blind, there are monsters all accross the board, we need systematical eradication but not stupid moves."

This addendum scopes cfdb v0.2 + a new "operate-module" protocol to close these gaps.

**Empirical justification** (from a 90-day scar log of 50 classified fixes):

| Root cause | Share |
|---|---|
| Duplicated feature | 34% |
| Canonical bypass | 22% |
| Random scattering | 22% |
| Unfinished refactoring | 14% |
| Unwired logic | 4% |
| Meta | 4% |

56% of recent fixes are duplicated-feature + canonical-bypass — the classes where a taxonomy-driven skill router has the highest leverage. Tool-found fixes in the last 10 days (post-`audit-split-brain` zero-tolerance) cluster into cheap sweeps; hand-found fixes in the preceding months trickled for weeks and surfaced as production incidents. The signal is clear: **trustworthy tooling + doughnut-scoped CI turns accumulated debt into mechanical sweeps**.

---

### A1. v0.2 scope expansion

#### A1.1 Schema extensions

Extend the §7 fact schema with three first-class node kinds and two edge kinds:

| Addition | Shape | Source | Purpose |
|---|---|---|---|
| `:EntryPoint` | `{name, kind, crate, qname, file, line, visibility}` | extractor — attribute on annotated functions OR heuristic on `main`, `#[tokio::main]`, MCP handler registration sites, CLI command definitions, cron job registrations, HTTP route definitions | Catalogs every way the system is invoked from outside. Required for Pattern B reachability analysis. |
| `:CallSite` | `{id, file, line, col, receiver_type, callee_type, callee_resolved}` | `ra-ap-hir` after method dispatch resolution | Every concrete call the extractor can resolve, including method dispatch through traits and macro expansion sites. `syn` alone delivers ~40–60% recall per §10.1 — insufficient. |
| `CALLS` edge | `(:CallSite)-[:CALLS]->(:Item)` | derived from `:CallSite.callee_resolved` | Materializes call graph for BFS queries. |
| `INVOKES_AT` edge | `(:Item)-[:INVOKES_AT]->(:CallSite)` | structural — the containing function → its call sites | Lets a query walk from `:Item` → its outgoing calls. |
| `EXPOSES` edge | `(:EntryPoint)-[:EXPOSES]->(:Item)` | derived — an entry point's handler function | Lets a query walk from user-facing surface to implementation. |

**Kind vocabulary for `:EntryPoint.kind`** (v0.2 initial set, extensible):

- `mcp_tool` — MCP protocol handler
- `cli_command` — `clap`-registered subcommand
- `http_route` — `axum`/`actix` route
- `cron_job` — `messenger`/`apalis` scheduled job
- `websocket` — WS connection handler
- `test` — `#[test]` / `#[tokio::test]` function (optional, for test-scope analysis)

Entry-point detection is **heuristic, not annotation-driven** in v0.2 — users should not need to annotate existing code. The extractor recognizes each kind by its registration call pattern (e.g., `ToolRegistry::register`, `Command::new(...).subcommand(...)`, `Router::new().route(...)`).

#### A1.2 `ra-ap-hir` adoption — new parallel extractor crate, not a dependency upgrade

**Architectural correction (council BLOCK, rust-systems B3).** Adopting `ra-ap-hir` is NOT an incremental dependency upgrade to the existing `cfdb-extractor`. `ra-ap-hir` does not parse — it type-checks. To resolve a method dispatch call, it must load a `ChangeSet` for the entire workspace, run type inference across all crates in dependency order, and materialize a `HirDatabase` (salsa database) for the type system. This is an **architectural replacement** of the extraction model (from file-by-file syn traversal to workspace-scoped type-checked HIR analysis), not an API swap.

**Resolution:** v0.2 ships a **new parallel crate `cfdb-hir-extractor`** alongside the existing syn-based `cfdb-extractor`. The new crate populates `:CallSite`, `CALLS`, `INVOKES_AT`, and `:EntryPoint` edges using HIR. The existing `cfdb-extractor` continues to populate `:Item`, `:Crate`, `:Module` using syn. v0.3 switches the Pattern B/C CI gate to consume `cfdb-hir-extractor` output once a release cycle of stability is proven.

**Boundary test (council WARN-1, clean-arch + rust-systems risk 1):** the architecture test covering parent RFC §14 Q2 (*"cfdb-core/Cargo.toml does not depend on lbug, duckdb, or syn — verified by an architecture test"*) is extended to assert: **no `ra_ap_*` type appears in any `cfdb-core` public type signature**. `ra-ap-hir` is confined to `cfdb-hir-extractor`; conversion to the graph schema happens inside the crate, never at the public boundary.

**Object safety constraint (council rust-systems risk 2):** `HirDatabase` is a salsa query database, explicitly NOT object-safe (uses associated types and generic methods). `cfdb-hir-extractor` must accept it as a monomorphic concrete type, not behind `dyn HirDatabase`. This is a design constraint the extractor must honor.

**Toolchain floor (council WARN-2, rust-systems):** `cfdb` workspace Cargo.toml must pin `rust-version = "1.85"` — `ra-ap-hir 0.0.328` uses `edition = "2024"` which stabilized in 1.85. Without this, developers on older toolchains get confusing compile errors.

**What `ra-ap-hir` gives us that `syn` cannot:**
- Method-dispatch resolution through trait impls (`self.resolve(input)` where `resolve` is a trait method with 5 impls)
- Macro expansion body analysis (procedural and declarative macros generate functions that `syn` cannot see without running the macros)
- Re-export chain resolution (`pub use foo::Bar` → knowing `Bar` refers to `foo::Bar` at the import site) — verified by council as a genuine gap that only hir fills
- Type inference for local variable receivers (`let x = foo(); x.bar()` → what type does `x.bar()` dispatch to?)

**Cost — two distinct categories, not one (council B1, rust-systems):**

| Category | Revised estimate | Basis |
|---|---|---|
| **Compile time** (cfdb-hir-extractor clean build, cold cache) | **+90–150s** | ~18–20 new `ra-ap-*` crates, salsa proc-macros, rustc-internal generics. The original "+30–60s" was sccache-warm inference; cold matters for CI. |
| **Compile time** (sccache warm, extractor-only change) | ~5–10s | Only the touched crate recompiles |
| **Runtime peak RSS** (cfdb extract on the target workspace) | 2–4 GB, **plausible but unverified** | HirDatabase for 23-crate workspace; no public benchmark found; v0.2 gate item measures this, see §A1.5 |

**Maintenance tax — upgraded from "risk" to acknowledged category concern (council B2, rust-systems):** `ra-ap-hir` releases **every 7 days without exception** (9 releases in 9 weeks verified on crates.io). All 10 `ra-ap-*` sub-crates are pinned with `=0.0.N` exact version constraints — every upgrade requires updating 10+ exact-pinned versions in Cargo.toml simultaneously, plus `ra-ap-rustc_type_ir` on independent versioning (2–3×/week). This is not a manageable risk; it is a **weekly maintenance tax** that must be budgeted.

**Protocol required (council proposal):** a dedicated `chore/ra-ap-upgrade-<version>` branch runs weekly, updates all `=0.0.N` pins, runs the cfdb determinism test suite, and either merges or files a compatibility issue. The cfdb CI gate does NOT consume a new `ra-ap-*` version until the chore branch lands. Runbook documented in `.concept-graph/cfdb/docs/ra-ap-upgrade-protocol.md` (post-v0.2 deliverable).

**Breaking changes observed:** rust-systems verified ~4 breaking API changes per year historically. These are not hypothetical — the `fall back to syn if API breaks` mitigation means `cfdb-hir-extractor` must be compilable and its outputs consumable even when the latest `ra-ap-*` release is incompatible. Practically: pin to last-known-good, upgrade deliberately, rollback freely.

**Alternatives considered and rejected (council-verified):**
- **`rust-analyzer` LSP IPC** — too slow, not deterministic, external process dependency
- **Regex-based call resolution** — fails on any trait dispatch, rejected in RFC-029 §10.1
- **`cargo-call-stack`** — verified unmaintained (last release 2024-10-28, 0.1.16), does NOT handle trait dispatch or generics. Removed from the Q9(c) fallback option.
- **`syn` + manual re-export resolver** — viable for shallow chains, degrades badly on nested re-exports (`qbot-ports` re-exporting from `qbot-domain`). Only hir fills this completely.
- **Wait for user annotations** — violates "heuristic not annotation-driven" principle; defeats the purpose of scanning legacy code

#### A1.3 Pattern B — vertical split-brain (`vertical-split-brain.cypher`)

**Informal goal:** starting from every `:EntryPoint`, trace reachable `:Item`s through `CALLS*` edges. For parameters that flow through the call chain, detect:

1. **Resolver forks** — the same param key is resolved in two distinct functions on the same reachable path, with different default values or different type targets
2. **Param drop** — a param enters at the entry point, is decoded into a domain type at layer K, but layer K+1 or beyond reads a *different* key that was never populated from the original input
3. **Divergent defaults across paths** — `PairConfig::default()` in use-case-A returns `hedge_lookback=120`, in use-case-B returns `hedge_lookback=None`, both reachable from the same MCP tool

**Output shape:**
```
(entry_point, param_key, layer_a, layer_b, divergence_kind, evidence)
```
where `divergence_kind` ∈ `{fork, drop, divergent_default}` and `evidence` is the file:line of each side.

**Known motivating bugs:** #2651 compound-stop, #3522 pair-resolution, #3545 `build_resolved_config` 3-way scatter, #3654 7 split resolution points.

#### A1.4 Pattern C — canonical bypass (`canonical-bypass.cypher`)

The `ledger-canonical-bypass.cypher` shipped in commit `349b153d6` is the prototype. v0.2 generalizes it to any resolver with a declared canonical impl.

**Informal goal:** given a marker (a comment annotation, a trait impl, or a naming convention) declaring "this function is the canonical resolver for concept X", find every call site that resolves X **without** going through the canonical impl. Emit verdict per site:

- `CANONICAL_CALLER` — uses the canonical impl (OK, no action)
- `BYPASS_REACHABLE` — bypasses the canonical impl, reachable from an `:EntryPoint` (action: rewire)
- `BYPASS_DEAD` — bypasses the canonical impl, NOT reachable from any `:EntryPoint` (action: delete)
- `CANONICAL_UNREACHABLE` — canonical impl exists but NOTHING reaches it (action: either wire bypass callers in or delete canonical)

**Known motivating bugs:** #3525 (LedgerService bypass), #3544/#3545/#3546 (parse_params / build_resolved_config scatter), #1526 (Capital.com `LiveTradingService` safety envelope bypass).

#### A1.5 v0.2 acceptance gate items

In addition to v0.1 items 1–6 (§13), v0.2 gates on:

| # | Item | Measurable |
|---|---|---|
| v0.2-1 | `:EntryPoint` catalog covers ≥95% of MCP tools + CLI commands on develop | `cfdb query` count compared against `rg`-based baseline of known entry points |
| v0.2-2 | `vertical-split-brain.cypher` reproduces #2651 finding from entry point to divergent resolver | Expected output contains `compound_stop` param + two divergent paths |
| v0.2-3 | `canonical-bypass.cypher` reproduces #3525 (LedgerService bypass) when parameterized on `append_idempotent` | Expected output ≥ the 2 bypass sites from the commit message |
| v0.2-4 | `CALLS` edge recall ≥80% against manually-curated ground truth on 3 representative crates | Ground truth crates: `domain-strategy`, `ports-trading`, `qbot-mcp`. **Instrumentation caveat (council W1):** the 80% target assumes hir resolves the dispatch cases syn cannot. v0.2 ships a pre-hir syn baseline measurement first to validate whether the 80% target is achievable and whether hir is actually required for the queries we care about, or whether a narrower resolution layer would suffice. |
| **v0.2-5a** (compile time) | `cfdb-hir-extractor` clean build (cold cache) ≤ **180s** | `cargo clean && time cargo build -p cfdb-hir-extractor`. Revised from 120s per rust-systems B1 evidence (cold build is +90–150s, not +30–60s). |
| **v0.2-5b** (runtime) | `cfdb extract --workspace .` on the target workspace completes in ≤ **N min**, peak RSS ≤ **M GB** | Measured during acceptance run with `/usr/bin/time -v`. Initial targets N=5 min, M=4 GB — calibrated after first measurement, not guessed. |
| v0.2-5c (maintenance) | `chore/ra-ap-upgrade-<version>` protocol documented and dry-run once before v0.2 ships | Exists as `.concept-graph/cfdb/docs/ra-ap-upgrade-protocol.md` + one proof upgrade recorded |
| v0.2-6 | Architecture test: no `ra_ap_*` type appears in any `cfdb-core` public signature | Extends RFC §14 Q2 test. **UDF scope clarification (second-pass clean-arch OBS-1):** if Cypher UDFs are used by the classifier (e.g., `signature_divergent`, `canonical_bypass_detected`, `confidence_score`), they MUST be registered inside `cfdb-store-lbug`, not in `cfdb-core`. The architecture test asserts UDF registration points do not cross the core boundary. |
| v0.2-7 | `cfdb` workspace pins `rust-version = "1.85"` | `grep rust-version cfdb/Cargo.toml` |
| **v0.2-8** (second-pass convergent follow-up) | `signature_divergent` UDF algorithm documented + ground-truth test | Document the field-set comparison algorithm (equal name + equal field names + field semantics discriminator) in `.concept-graph/cfdb/docs/udf-signature-divergent.md`. Add a unit test against `OrderStatus` (`domain-trading` vs `domain-portfolio`) and `PositionValuation` (`domain-trading` vs `domain-portfolio`, #3618 evidence) as known ground truth for the Shared-Kernel-vs-Homonym discriminator. This UDF is load-bearing for class 2 (Context Homonym) correctness — without a documented algorithm it could misclassify Shared Kernel candidates as homonyms or vice versa. Raised by ddd-specialist second pass. |
| **v0.2-9** (second-pass convergent follow-up) | `enrich_bounded_context` accuracy spot-check on ground-truth crates | During v0.2 acceptance run, manually verify the `bounded_context` label for every `:Item` in `domain-strategy`, `ports-trading`, `qbot-mcp` (the 3 ground-truth crates used in gate v0.2-4). Produce a one-page report showing: crate → context assignment per the heuristic, any overrides from `.cfdb/concepts/*.toml`, any items where the module-level context diverges from the crate-level context (v0.3 concern, but track at v0.2). Gate passes if ≥95% of items receive the human-expected context label. Raised by clean-arch OBS-1 second pass. |

**Instrumentation gate (pre-hir validation, council W1):** before committing to `ra-ap-hir` as a hard v0.2 dependency, v0.2 ships a preliminary `syn`-only extractor that emits `CALLS` edges with `resolved=false` tagging for unresolved dispatch sites. Measurement: what fraction of Pattern B/C findings actually depend on resolved dispatch edges? If the answer is <20%, the 80% gate (v0.2-4) may be reachable with `syn` + a targeted resolution layer, and `ra-ap-hir` adoption can defer to v0.3 without blocking the triple objective. The instrumentation gate is non-blocking — v0.2 proceeds to `cfdb-hir-extractor` regardless — but it informs whether the hir pipeline ships as the primary CI gate or remains a complementary instrument.

**Still deferred to v0.3+:** entry-point annotations, LLM enrichment, cross-project queries, embedding clustering, density-based thresholds (LoC instrumentation ships in v0.2 as §A3.2 telemetry).

#### A1.7 `cfdb extract --rev <url>@<sha>` — bilateral cross-repo drift-lock (Option W)

**Issue:** #96 (builds on #37 / PR #123).

**Problem.** qbot-core EPIC #4047 Phase 2 needs a cross-repo drift-lock against `yg/qbot-strategies`. The comparator consumes two fact sets (one per repo), extracted at specific SHAs, and asserts invariants across them. Without URL extraction, the user must maintain two local checkouts and orchestrate `cfdb extract --rev <sha>` separately against each — fragile, easy to misalign. Option W (qbot-core council-4046 tools-devops R2.2) is the chosen mechanism over Option Y (third `qbot-specs` repo).

**Scope.**

- `cfdb extract --rev <url>@<sha>` clones `<url>` once per `(url, sha)` pair into a persistent cache and extracts at `<sha>`.
- `<url>` carries one of `http://` / `https://` / `ssh://` / `file://` schemes. SSH shorthand (`git@host:path`) is NOT accepted in v1 — use the explicit `ssh://…` form. `file://` is supported both for hermetic integration tests and for the self-dogfood case `cfdb extract --rev file://$(pwd)/.git@$(git rev-parse HEAD)`.
- The cache base directory is, in precedence order:
  1. `$CFDB_CACHE_DIR` (explicit override; used by tests for hermeticity).
  2. `$XDG_CACHE_HOME/cfdb/extract`.
  3. `$HOME/.cache/cfdb/extract`.
  4. `std::env::temp_dir()/cfdb/extract` (last resort; non-persistent; emits `eprintln!` warning).
- Cache layout: `<base>/<sha256_hex_first_16(url)>/<full-sha>/`. Full SHA (not `short_rev`) so 12-char prefix collisions remain distinct on disk.
- A sentinel file `.cfdb-extract-ok` inside the cache dir signals a successful clone+checkout; second runs gate off the sentinel and skip the clone (AC-3).
- Auth (AC-2): subprocess `git clone` / `fetch` / `checkout` inherit ambient git credentials — SSH agent, `~/.config/git/credentials`, `GIT_ASKPASS`, `credential.helper`. No new plumbing.

**Design.**

- The `extract` dispatcher in `cfdb-cli/src/commands.rs` discriminates URL@SHA vs. plain SHA at a single match guard — `Some(rev) if is_url_at_sha(rev) => extract_at_url_rev(rev, …)`. The same-repo `extract_at_rev` (PR #123) and its `GitWorktree` RAII guard are UNCHANGED; URL form is a new sibling branch, not a modification.
- `parse_url_at_sha(&str) -> Option<(&str, &str)>` splits on the RIGHTMOST `@` so `ssh://user@host/r@deadbeef` parses correctly. The SHA side must be all-hex and ≥ 7 chars; the URL side must carry a recognised scheme.
- `git clone <url> <cache_dir>` fetches the default branch only; the arbitrary `<sha>` is explicitly fetched next (`git fetch --quiet origin <sha>`) then checked out. Gitea has `uploadpack.allowReachableSHA1InWant=true` by default, which makes this work; other servers may need the setting enabled. The CLI error names this config when fetch fails for that reason.

**Invariants.**

- **Subprocess contract preserved.** `git2` stays behind the `git-enrich` feature gate — default `cfdb-cli` builds still ship zero `git2` in their dep tree (issue #105). `sha2 = "0.10"` is the only new workspace dep (pure-Rust, small, for URL → cache-key hashing).
- **Single resolution point.** URL-vs-SHA discrimination lives ONLY in the `extract` match guard. `extract_at_rev` and `extract_at_url_rev` trust the dispatch — neither re-checks the form.
- **Determinism.** Same `(url, sha)` produces byte-identical canonical dumps on repeat extract (same guarantee as `extract --rev <sha>` today; the extraction pipeline is unchanged).
- **No `SchemaVersion` bump.** The emitted facts are identical to what the same-repo path emits; only the input-source path changes.

**Non-goals.**

- No new `--cache-dir` flag in this issue (env-var override is sufficient; flag may be added in a follow-up if a real need materialises).
- No `git2` library path for the URL clone — subprocess is the contract, matching §2 "Group B" convention.
- No HTTPS auth plumbing new to cfdb — ambient git credentials are the contract (AC-2). A future issue can add `--token` / `CFDB_GITEA_TOKEN` support if manual token injection becomes ergonomic.
- No `url` / `dirs` / `reqwest` crates — env-var path + scheme-prefix string split is sufficient.

**Tests (per CLAUDE.md §2.5).**

- **Unit:** `parse_url_at_sha` / `is_url_at_sha` / `url_hash_hex16` / `cache_base_dir` env-var precedence — covered in `crates/cfdb-cli/src/commands.rs` `#[cfg(test)] mod tests`.
- **Integration:** `crates/cfdb-cli/tests/extract_rev_url.rs` — 5 scenarios: URL form honours the SHA (AC-1), cache reuse (AC-3), unreachable URL surfaces git error (AC-2 shape; real Gitea auth is dogfood), malformed URL@SHA falls through to same-repo path, default keyspace = `short_rev(sha)`. All tests use `file://` URLs against local bare repos — zero network access.
- **Self-dogfood:** `cfdb extract --rev file://$(pwd)/.git@$(git rev-parse HEAD)` produces a keyspace on the cfdb tree.
- **Target-dogfood (manual, PR body):** `cfdb extract --rev https://agency.lab:3000/yg/qbot-core@<pinned-sha>` and the same for `yg/qbot-strategies` — per issue AC-4, reported as manual evidence in the ship PR body.

#### A1.8 `.cfdb/published-language-crates.toml` — Published Language marker (Issue #100)

**Issue:** #100 (feeds §A2.1 class 2 Context Homonym classifier).

**Problem.** The six-class taxonomy (§A2.1) distinguishes `Context Homonym` (same name, different bounded contexts, semantically divergent) from a **Published Language** (intentional cross-context consumer — DDD pattern). Without a marker file, the classifier cannot tell a legitimately-shared type like `qbot-prelude::Symbol` from a divergent homonym — both present as "same name across contexts" at the graph level. Issue #48 classifier consumes the marker; the marker itself is what #100 lands.

**Scope.**

- New single-file config at `.cfdb/published-language-crates.toml` (sibling of the directory-based `.cfdb/concepts/*.toml`). Optional — missing file is not an error, and the baseline behaviour is "every `:Crate` emits `published_language: false`".
- On-disk shape:

  ```toml
  [[crate]]
  name = "qbot-prelude"
  language = "prelude"
  owning_context = "core"
  consumers = ["trading", "portfolio", "strategy"]

  [[crate]]
  name = "qbot-types"
  language = "types"
  owning_context = "core"
  consumers = ["*"]
  ```

- Loader shape: `load_published_language_crates(workspace_root: &Path) -> Result<PublishedLanguageCrates, LoadError>` in `cfdb-concepts` (canonical home per PR #103 / issue #3). Reuses the existing `LoadError` enum — `Io` + `Toml` variants cover every failure mode. Duplicate `name` entries in the TOML array are rejected via `LoadError::Io { ErrorKind::InvalidData }` (silent last-wins is forbidden).
- Public API per issue AC-2: `is_published_language(&str) -> bool`, `owning_context(&str) -> Option<&str>`, `allowed_consumers(&str) -> Option<&[String]>`. The loader does NOT interpret the `"*"` wildcard — it passes consumer strings through verbatim; wildcard semantics are the classifier's job (issue #48).
- `:Crate` nodes gain a `published_language: bool` prop materialised at extraction time in `cfdb-extractor::emit_crate_and_walk_targets`. Every `:Crate` carries the prop (no `Option`); missing file ⇒ every crate emits `false`.

**Design — extract-time only, not re-enrichment.**

`enrich_bounded_context` (PR #119) earns its re-enrichment machinery because users routinely edit context-map TOMLs between extractions and want to patch `:Item.bounded_context` without re-walking the workspace. Published Language is a rarely-edited marker (DDD policy-level decision), so the simpler extract-time emission is chosen for the first landing. If the #48 classifier later needs post-extract patching, a follow-up issue can add `crates/cfdb-petgraph/src/enrich/published_language.rs` mirroring the `enrich_bounded_context` pattern; the current extract-time wiring is a clean prerequisite.

**Invariants.**

- **No `SchemaVersion` bump.** `published_language: bool` is additive on an already-declared `:Crate` node shape; `PropValue::Bool` is already in the wire format. No lockstep `graph-specs-rust` fixture bump required (RFC-033 §4 I2: adding an optional prop does not break the schema contract).
- **Single resolution point.** `load_published_language_crates` is canonical at `crates/cfdb-concepts/src/published_language.rs`; `:Crate.published_language` is written at exactly one site (`emit_crate_and_walk_targets`). A second writer would split-brain the prop.
- **`LoadError` reused.** No parallel `PublishedLanguageLoadError` — the canonical enum covers every failure mode.

**Non-goals.**

- No classifier logic in the loader (that's #48).
- No validation that declared `consumers` actually import the crate at compile time (static TOML check only).
- No wildcard expansion at the loader layer — `"*"` passes through.
- No CLI flag — loader is library-only, invoked by `extract_workspace`.
- No sample `.cfdb/published-language-crates.toml` in the cfdb repo itself — cfdb does not currently publish a language; the self-dogfood baseline is "file absent → `published_language=false` on every crate".

**Tests (per CLAUDE.md §2.5).**

- **Unit (8 tests, `crates/cfdb-concepts/src/published_language.rs::tests`):** missing file, empty file, single crate, multiple crates, wildcard consumers, malformed TOML, determinism, duplicate-name rejection.
- **Integration (3 tests, `crates/cfdb-concepts/tests/published_language.rs`):** full-pipeline 3-entry fixture exercising all three public methods; missing `.cfdb/` baseline; `.cfdb/` without PL file baseline.
- **Self-dogfood (2 tests, `crates/cfdb-extractor/tests/published_language_dogfood.rs`):** synthetic 2-crate workspace fixture — one declared, one not; asserts prop value matches per crate. No-PL-file baseline asserts `false` on every `:Crate`.
- **Cross-dogfood:** `ci/cross-dogfood.sh` unchanged (no `SchemaVersion` bump, no new ban rule).

---

### A2. Debt-cause taxonomy (new §A2 to RFC)

#### A2.1 The six classes

The existing RFC §3.1–§3.9 taxonomizes by **pattern shape** (HSB, VSB, canonical bypass, …). That answers *"what does the query look like?"*. It does not answer *"what caused this debt, and therefore what fix strategy applies?"*. The six classes below answer the second question.

**Sixth class added per council BLOCK (ddd-specialist):** "Context Homonym" — same name in different bounded contexts. Without this class, `/sweep-epic` would fire on legitimately distinct concepts that share a name across contexts, deleting bounded-context isolation. The fix strategy for a homonym is a **Context Mapping decision** (ACL / Shared Kernel / Conformist / Published Language), not a mechanical sweep — therefore a separate class is load-bearing.

Every cfdb finding (from Pattern A, B, or C rules) is labeled with exactly one class by the classifier:

| # | Class | Informal definition | Signals used | Fix strategy (abstract action — routing in §A2.3) |
|---|---|---|---|---|
| 1 | **Duplicated feature** | Two independent implementations of the same concept, **within the same bounded context**, with independent git history. Two teams or two sessions built the same thing without knowing about each other. | Same `bounded_context` on both items; independent git blame (no common commit in last N months touching both sides); `signature_hash` similarity > threshold; no cross-reference comments | **Consolidate** — pick one head (by usage count or complexity), `pub use` the other, delete the loser. |
| 2 | **Context Homonym** | Same name (or high-Jaccard structural similarity) appearing in items whose owning crates belong to **different bounded contexts**, where the semantics diverge (different domain invariants, different lifecycle, different collaborators). | `a.bounded_context <> b.bounded_context`; divergent collaborator sets in the graph (what edges go in/out differ); divergent field sets despite similar names; no shared RFC owning the concept across both contexts | **Context Mapping decision** — NOT mechanical dedup. Remedy is ACL, Shared Kernel, Conformist, or Published Language. Requires architect judgment. Routed as `council_required` → `/operate-module` with the owning-context declaration. |
| 3 | **Unfinished refactoring** | Old code and new code coexist because a planned migration was never completed. One side is actively used, the other is legacy; an RFC or issue exists that proposed the migration, and the RFC reference is scoped to the owning bounded context. | One side has recent commit cluster referencing a context-owning RFC/EPIC; the other is stale; `TODO(#issue): migrate` comment; `#[deprecated]` attribute; age_delta > 60 days | **Complete migration** — move callers, delete legacy. |
| 4 | **Random scattering** | Copy-paste drift with no refactor intent. Same function body appears N times with small variations, typically short utility helpers. | Identical or near-identical AST shape; no common refactor intent; no RFC reference; no deprecation marker; age_delta < 14 days; typically short functions | **Extract helper** — consolidate inline during adjacent feature work. |
| 5 | **Canonical bypass** | A canonical resolver exists and is correct, but some call sites go around it. This is a *wiring* bug, not a duplication bug. | `canonical-bypass.cypher` verdict = `BYPASS_REACHABLE`; canonical impl has `#[doc = "canonical …"]` marker or is the ports-layer trait impl | **Rewire** — replace direct calls with canonical calls. Delete bypass if dead. |
| 6 | **Unwired logic** | Code exists and compiles but has zero paths from any `:EntryPoint`. Either dead code (delete) or orphan code awaiting wiring (wire or delete). | BFS reachability from `:EntryPoint` set = empty; `cargo-udeps` / `cargo-machete` cross-validation | If `TODO(#issue)` attached → **wire**. Otherwise **delete**. |

**Note on the fix strategy column:** the values above are abstract action names (consolidate, complete migration, extract helper, rewire, context-mapping decision, wire/delete), NOT concrete Claude skill names. The mapping from abstract action → concrete skill (`/sweep-epic`, `/port-epic`, `/boy-scout`, `/operate-module`) is defined in the `SkillRoutingTable` (§A2.3), not in the classifier. This is the DIP-clean form (council BLOCK-1, solid-architect).

#### A2.2 Classifier — enrichment passes + query, not a single `.cypher` rule

**Architectural correction (council BLOCK-1, clean-arch).** The classifier cannot be a standalone `.cypher` rule. The signals it joins on require filesystem and subprocess I/O (git log, file reads of `.concept-graph/*.md`, deprecation attribute extraction) that Cypher traversal cannot perform atomically. The classifier is a **two-stage pipeline**: enrichment passes materialize signals into the graph as new edges/attributes, THEN a Cypher query joins on the enriched graph.

**Stage 1 — enrichment passes** (each is an extractor-layer operation that mutates the graph). The table below was revised by the #43 council round 1 synthesis (2026-04-20): six passes (not five) per DDD Q4 finding; `:Concept` node materialization is a distinct sixth pass that #101 and #102 block on. `enrich_metrics` is explicitly deferred out of this pipeline:

| # | Pass | Slice | Input | Output edges / attributes | Layer |
|---|---|---|---|---|---|
| 1 | `enrich_git_history` | 43-B (#105) | git log for each `:Item`'s defining file | `:Item.git_last_commit_unix_ts` (i64 epoch, **not** `git_age_days` — see G1 note), `:Item.git_last_author`, `:Item.git_commit_count` | extractor crate (uses `git2` crate behind `git-enrich` feature flag) |
| 2 | `enrich_rfc_docs` | 43-D (#107) | `docs/rfc/*.md` + `.concept-graph/*.md` keyword match against concept names (scope narrowed — see scope-narrowing note) | `(:Item)-[:REFERENCED_BY]->(:RfcDoc {path, title})` edges + nodes | petgraph impl (reads RFC files once at pass time via workspace path stored on `PetgraphStore`) |
| 3 | `enrich_deprecation` | 43-C (#106) | `#[deprecated]` attribute extraction from syn AST | `:Item.is_deprecated`, `:Item.deprecation_since` | **extractor (extractor-time — not a Phase D enrichment)** — the attribute is syntactic and the AST walker already visits attributes (see deprecation provenance note) |
| 4 | `enrich_bounded_context` | 43-E (#108) | crate-prefix convention + `.cfdb/concepts/*.toml` overrides | `:Item.bounded_context` (re-enrichment of extractor-time output when TOML changes) | petgraph impl (re-enrichment only — extractor already populates `bounded_context` + `BELONGS_TO`) |
| 5 | `enrich_concepts` | 43-F (#109) | `.cfdb/concepts/<name>.toml` declarations | `:Concept {name, assigned_by}` nodes + `(:Item)-[:LABELED_AS]->(:Concept)` + `(:Item)-[:CANONICAL_FOR]->(:Concept)` — **DDD Q4 sixth pass; unblocks #101 + #102** | petgraph impl (reads TOML via `cfdb-concepts::ConceptOverrides`) |
| 6 | `enrich_reachability` | 43-G (#110) | BFS from `:EntryPoint` over `CALLS*` | `:Item.reachable_from_entry = bool`, `:Item.reachable_entry_count` | petgraph impl (runs after HIR extraction; degraded path with `ran: false` + warning when no `:EntryPoint` nodes present) |

**G1 determinism note — timestamps, not ages.** `enrich_git_history` stores `git_last_commit_unix_ts` (i64 epoch seconds), not `git_age_days`. Days-since-now computed at enrichment time violates G1 byte-stability across calendar days (clean-arch verdict B2, council/43/clean-arch.md). The Stage-2 classifier Cypher below computes `age_delta = abs(a.git_last_commit_unix_ts - b.git_last_commit_unix_ts) / 86400` at query time instead of reading a pre-baked `git_age_days` value.

**`enrich_metrics` — deferred out of #43 scope.** The quality-metrics pass (`unwrap_count`, `cyclomatic`, `dup_cluster_id`, `test_coverage`) is orthogonal to the debt-cause classifier pipeline: the six classes in §A2.1 do not consume these signals. The Phase A stub is retained on `EnrichBackend` and in `Provenance::EnrichMetrics` so the surface is stable; a future RFC can resuscitate the pass without a breaking rename.

**Scope narrowing of `enrich_rfc_docs`.** Renamed from the v0.1 `enrich_docs` stub and scope-narrowed to RFC-file keyword matching only. The broader rustdoc rendering implied by the former Phase A stub doc comment is an **explicit non-goal for v0.2** — no #43 slice implements it. Full rustdoc enrichment is deferred beyond v0.2 and may land behind its own RFC and Provenance variant.

**Deprecation provenance — `Provenance::Extractor`, not an enrichment tag.** The RFC's original wording ("reuses existing AST walk") is now explicit: `#[deprecated]` is extracted at extraction time and tagged `Provenance::Extractor`. The `EnrichBackend::enrich_deprecation` method exists for surface symmetry but its `PetgraphStore` impl is a `ran: true, attrs_written: 0` no-op naming the extractor as the real source. This prevents the provenance split-brain DDD Q4 flagged.

**Invariant I6 (v0.2-9 load-bearing gate).** The Stage-2 classifier (issue #48) MUST NOT be deployed until `enrich_bounded_context` (slice 43-E) hits the v0.2-9 ≥95% accuracy gate on the ground-truth crates (`domain-strategy`, `ports-trading`, `qbot-mcp`). Below 95% accuracy the `cross_context` boolean produces enough false positives to misroute mechanical dedup into expensive council deliberations and (worse) false negatives that misroute homonyms into `/sweep-epic --consolidate` — which deletes bounded-context distinctions (DDD Q3 analysis, council/43/ddd.md).

**SchemaVersion bump policy.** Per-slice patch bumps — **not** a batched single bump. Each slice that writes new attributes or labels into the graph bumps the version (V0_2_1, V0_2_2, …) with its own lockstep `graph-specs-rust` cross-fixture PR per cfdb CLAUDE.md §3. Slice 43-A ships schema reservations (`:RfcDoc` label, `REFERENCED_BY` edge, new `Provenance` variants) without bumping the version — stubs write nothing, so no wire-format consumer sees a change. The first real bump lands with whichever of 43-B/43-D/43-G reaches ship first.

**Stage 2 — classifier Cypher query** (reads only enriched facts, no I/O):

```cypher
// classifier.cypher — joins Pattern A/B/C raw outputs with enriched attributes
MATCH (a:Item), (b:Item)
WHERE <pattern A/B/C match conditions>
  AND a.bounded_context IS NOT NULL
  AND b.bounded_context IS NOT NULL
WITH a, b,
     abs(a.git_last_commit_unix_ts - b.git_last_commit_unix_ts) / 86400 AS age_delta,
     (a.bounded_context <> b.bounded_context) AS cross_context,
     exists { (a)-[:REFERENCED_BY]->(:RfcDoc) } AS has_rfc_ref,
     a.is_deprecated OR b.is_deprecated AS has_deprecation,
     a.reachable_from_entry AS a_reachable,
     b.reachable_from_entry AS b_reachable
RETURN a, b,
       case
         when cross_context AND signature_divergent(a, b) then 'context_homonym'
         when has_deprecation then 'unfinished_refactor'
         when has_rfc_ref AND age_delta > 60 then 'unfinished_refactor'
         when NOT (a_reachable OR b_reachable) then 'unwired'
         when canonical_bypass_detected(a, b) then 'canonical_bypass'
         when age_delta < 14 AND NOT has_rfc_ref then 'random_scattering'
         else 'duplicated_feature'
       end AS class,
       confidence_score(a, b) AS confidence
```

**Classifier output schema** (new `:Finding` node type — `fix_skill` removed per council BLOCK-1):

```
:Finding {
  id, pattern, class, confidence,
  canonical_side, other_sides[],
  evidence[], age_delta_days,
  rfc_references[], bounded_contexts[],
  is_cross_context: bool
}
```

where `class ∈ {duplicated_feature, context_homonym, unfinished_refactor, random_scattering, canonical_bypass, unwired}` (6 classes — see §A2.1). **No `fix_skill` field.** Skill routing is a separate concern — see §A2.3.

**Why `fix_skill` is NOT in the schema (council BLOCK-1, solid-architect).** Embedding the skill name in the graph couples the classifier (data layer) to the orchestration policy (skill layer). A skill rename or a `/port-epic`-vs-`/sweep-epic --mode=port` decision would force a graph schema migration. The classifier emits abstract `DebtClass`; a separate `SkillRoutingTable` (see §A2.3) maps class → skill at invocation time. This preserves OCP under skill naming churn and keeps the fact base free of workflow knowledge (parent RFC §4 invariant).

#### A2.3 Skill routing — SkillRoutingTable (external to the graph)

**Architectural correction (council BLOCK-1).** The routing table lives **outside the graph schema**, in a separate `SkillRoutingTable` config (either `.cfdb/skill-routing.toml` or a const in the consumer skill). The classifier emits the abstract `class`; the orchestrator consults the routing table to pick a concrete skill. This preserves OCP under skill naming changes and keeps the fact base free of workflow policy (parent RFC §4 invariant).

**Routing table** (initial mapping; lives in `.cfdb/skill-routing.toml`):

| Class | Abstract action | Concrete skill (routing table, not graph) | Notes |
|---|---|---|---|
| `duplicated_feature` | Consolidate within context | `/sweep-epic` | Mechanical, parallelizable |
| `context_homonym` | Context Mapping decision | `/operate-module` with `council_required=true` | Remedy is architectural, not mechanical. Routes to operate-module to trigger raid plan + council. |
| `unfinished_refactor` | Complete migration | `/sweep-epic --mode=port --raid-plan=<path>` | Q14: variant flag on sweep-epic, not a new skill |
| `random_scattering` | Extract helper | `/boy-scout` | Inline fix during adjacent work |
| `canonical_bypass` | Rewire | `/sweep-epic` | Special case of consolidation — acknowledged seam per council WARN-1 |
| `unwired` (with `TODO(#issue)`) | Wire | `/boy-scout` or the issue owner's session | Wire to existing tracked work |
| `unwired` (no tracker) | Delete | `/boy-scout` delete | Clean removal |

**Note on `/port-epic`:** NOT a new skill. Council Q14 vote (clean-arch + solid-architect) = variant flag on `/sweep-epic`, invoked as `/sweep-epic --mode=port --raid-plan=<path>`. The distinguishing factor (approved architecture target + portage list) is an input protocol difference, not a code path difference. Promote to standalone skill only if the variant accumulates ≥3 unique responsibilities (deferred to v0.3 evaluation).

**Why a table, not hardcoded edges:** if `/sweep-epic` is later renamed or `/operate-module` is split into multiple skills, only the routing table changes — the graph schema and the classifier are untouched. This is the DIP-correct form: high-level policy (skill names) lives above the data, not inside it.

---

### A3. Operate-module rescue protocol

#### A3.1 Motivation — the hydra problem

Historical pattern (see scar log):

- **#3244 Venue 4-way split-brain** — one concept had 4 incompatible meanings across domain-ledger, executor, capital-adapter, reconciliation. Fix required a coordinated sweep with a rename + two new types + a legacy-string handler. This is NOT a `/boy-scout` job — it's a surgery.
- **#2651 compound-stop** — two parallel resolution paths for trailing activation, with a hardcoded constant preempting the param-driven path. Fix required Council deliberation, 3 canary rules (M1/M2/M3 in CLAUDE.md §7), and a re-architecture of the layer-dominance model. Surgery.
- **#3519 post-forgery curation** — 46 actionable violations across 19 fix clusters. Individual clusters are mechanical, but the *meta* decision (which concept is canonical, which is the head to keep) requires architectural judgment — surgery.

A single finding is a `/boy-scout` job. A **cluster of findings inside one module** is a surgery. We need a protocol for the second case.

#### A3.2 Infection threshold — scoped to bounded contexts, not crates

**Architectural correction (council BLOCK, ddd-specialist).** Thresholds operate on **bounded contexts** (groups of crates), not single crates. Crate-scoped thresholds produce the exact #3244 Venue 4-way failure mode: a raid plan reorganizes crates but does not align the bounded-context boundary, and three months later the split-brain is back.

**Bounded context identification (v0.2 initial rules, extensible):**

1. **Crate-prefix convention:** `domain-trading` + `ports-trading` + `adapters-trading` all belong to the `trading` context. Rule: strip `domain-` / `ports-` / `adapters-` / `inmemory-` / `postgres-` / `kraken-` / `mcp-` prefix suffixes, the remainder names the context.
2. **Explicit override:** `.cfdb/concepts/<context>.toml` lists crates belonging to this context, overriding the heuristic. Required for contexts that don't follow the prefix convention (e.g., cross-cutting `messenger`, `sizer`).
3. **Ownership declaration:** each context has exactly one owning RFC and one canonical domain crate declared in the `.toml`. This is load-bearing for §A2.1 class 3 "unfinished refactoring" — the RFC keyword match must be scoped to the owning context.

**A context is "too infected" when it exceeds ANY of:**

- **≥5 canonical bypasses** (Pattern C verdict = `BYPASS_REACHABLE`) counted across all crates in the context
- **≥10 duplicate types** (Pattern A findings where both sides are in crates belonging to the context)
- **≥3 context-homonym findings** (§A2.1 class 2) where this context is one side — triggers surgery regardless of other counts
- **≥3 unwired entry points** in crates belonging to the context
- **≥20% of items classified as `unfinished_refactor`** across the context
- **≥1 Pattern B fork** where divergent paths traverse resolvers in this context

**Telemetry alongside thresholds:** v0.2 ships LoC per crate per context (council WARN-6 from solid-architect) so v0.3 can flip to density-based thresholds (per-kloc) without a second extraction pass. Initial absolute counts, calibration in v0.3.

These thresholds are **initial values**, subject to calibration from v0.2 telemetry.

#### A3.3 Raid plan doc — emitted by `/operate-module`, not by cfdb

**Architectural correction (council BLOCK-2, clean-arch + solid-architect).** cfdb does NOT emit the raid plan markdown. cfdb's responsibility ends at returning a **structured infection inventory** — JSON or Cypher result rows containing the findings by class, the canonical candidates, and the reachability data. The `/operate-module` consumer skill (§A3.4) reads that structured inventory and formats it into the raid plan document.

This aligns with parent RFC §4 invariant: *"Not opinionated about workflows. Knows nothing about 'raids', '/prescribe', 'RFCs'. Those are consumer-side compositions."*

**cfdb returns (structured, layer-clean):**

```json
{
  "context": "trading",
  "thresholds_crossed": ["canonical_bypass_ge_5", "context_homonym_ge_3"],
  "crates_in_context": ["domain-trading", "ports-trading", "adapters-trading"],
  "findings_by_class": {
    "duplicated_feature": [...],
    "context_homonym": [...],
    "unfinished_refactor": [...],
    "canonical_bypass": [...],
    "unwired": [...]
  },
  "canonical_candidates": [...],
  "reachability_map": {...},
  "loc_per_crate": {...}
}
```

**`/operate-module` formats this into `raid-plan-<context>.md`** using a template owned by the consumer skill (not by cfdb):

```
# Raid plan: <context-name> (bounded context)

**Status:** draft — council required
**Triggered by:** cfdb threshold (<which ones>)
**Date:** <date>
**Context crates:** <list of crates belonging to this context>

### Current infection inventory
- <table of findings, grouped by class — populated from cfdb structured output>

### Canonical candidates
- <concepts where ≥2 impls exist — populated from cfdb `canonical_candidates` keyed by Pattern I queries from parent RFC §3.9, NOT by a divergent scan>

### Portage list (code belonging to this context)
- <items flagged as viable — move to new home within the context>

### Misplaced list (code belonging to a different context — raid target)
- <items where the classifier flagged `context_homonym` — these belong to another context and must be returned>

### Dead list (no reachable entry points)
- <items where `reachability_from_entry = false` AND no tracker attached — delete>

### Proposed new home architecture
- <empty section — council fills in based on user's strategic intent + owning-context declaration>

### Council decisions required
- Q1: Which impl of the homonym is canonical for which context? (Context Mapping decision)
- Q2: Is the proposed RFC for the new home already drafted?
- Q3: Which consumer sessions will execute the portage?

### Rollback plan
- <git tag before surgery, listing of untouched crates>
```

**Language correction (council WARN-3, ddd-specialist):** the template uses **"Dead list"** (no reachable entry points, delete) + **"Misplaced list"** (belongs to a different context, return) instead of the imprecise "Cancer list". These are two different dispositions requiring two different actions.

**Pattern I integration (council WARN-2, ddd-specialist):** the "Canonical candidates" and "Portage list" sections are populated by the 5 Pattern I Cypher queries defined in parent RFC §3.9 (completeness, dangling-drop, hidden-callers, missing-canonical, clean/dirty-mismatch), not by a separate scan. Tying §A3.3 to Pattern I avoids divergent implementations.

**The raid plan is a draft, not an executable spec.** Council turns it into a concrete RFC before any code moves.

#### A3.4 Skill wiring — SRP-compliant decomposition

**Architectural correction (council BLOCK-2, solid-architect).** The original spec bundled four responsibilities into `/operate-module` (extract / threshold-check / raid-plan-emit / boy-scout-fallback). Council SRP audit requires splitting these into distinct skills with independent change vectors.

**Decomposition:**

| Skill | Single responsibility | Inputs | Outputs |
|---|---|---|---|
| **cfdb extract** (existing verb) | Produce fact inventory for a bounded context | context name, workspace path | Structured JSON inventory (§A3.3 shape) — no interpretation |
| **`/operate-module <context>`** (new, slimmed) | Decide if surgery is needed + emit raid plan | structured inventory from cfdb | Raid plan markdown (if threshold crossed), OR "below threshold, route to /boy-scout" verdict |
| **`/boy-scout`** (existing) | Routine debt triage, no surgery | below-threshold inventory OR explicit `class ∈ {random_scattering, unwired}` findings | Inline fix commits, no council required |
| **`/sweep-epic --mode=port --raid-plan=<path>`** (existing with variant flag, Q14) | Execute approved portage per raid plan + RFC | approved RFC + raid plan | Portage commits, scar tests, rollback tag |

**`/operate-module` revised spec (two responsibilities — not four):**

1. **Evaluate threshold** against §A3.2 rules on the structured cfdb inventory
2. **Emit raid plan markdown** (§A3.3 template) if threshold crossed, with council-required marker

It does NOT run cfdb (that's a separate skill call that precedes it). It does NOT execute boy-scout fallback (the orchestrator routes to `/boy-scout` when threshold is not crossed). It does NOT execute the portage (that's `/sweep-epic --mode=port`).

**Invocation flow:**

```
/cfdb-scope <context>          → inventory.json
    → if threshold check trips → /operate-module <context> inventory.json → raid-plan.md → COUNCIL → approved RFC → /sweep-epic --mode=port --raid-plan=raid-plan.md
    → else → /boy-scout applies class-appropriate actions inline
```

**Why two skills, not four (council Q13 + SOLID SRP):** the minimum viable split keeps cfdb extraction and boy-scout as their own skills (already existing, independent change reasons) and consolidates the "go/no-go + document" decision into a single `/operate-module` responsibility. Further splitting (`/operate-threshold` + `/operate-plan`) is premature — one reason to change, no empirical evidence yet for further decomposition.

**`/port-epic` is NOT a new skill (council Q14 unanimous):** the portage workflow becomes a variant flag `--mode=port --raid-plan=<path>` on the existing `/sweep-epic` skill. The distinguishing factor (approved architecture target + portage list) is an input protocol difference, not a code path difference. Re-evaluate in v0.3 after empirical evidence.

#### A3.5 Council trigger

The protocol explicitly requires human + council approval between raid-plan emission and code execution. This is the counterweight to "systematical eradication but not stupid moves" — the tool identifies candidates, the council decides fate.

**`/operate-module` is not a one-shot surgery tool.** It is a staging tool that produces an artifact (raid plan) that then enters the normal RFC + approval flow.

---

### A4. Session bootstrap

#### A4.1 Project CLAUDE.md §12

A new section `§12 Split-brain Eradication Mission` is added to `/path/to/target-workspace/CLAUDE.md`. Contents (draft in companion edit):

- Triple objective recap (horizontal / vertical / taxonomy)
- Current phase of the roadmap (Phase 0–5 from the session roadmap)
- Zero-tolerance target
- Pointer to `.concept-graph/RESCUE-STATUS.md` (live inventory)
- Pointer to `/operate-module` skill (post-v0.2)
- Pointer to this addendum

**Why project CLAUDE.md:** it is auto-loaded at session start for any session working in this repo. No Redis, no memory system, no separate bootstrap hook required. The user has explicitly asked for a no-repeat-per-session mechanism; CLAUDE.md is the existing channel.

#### A4.2 Live inventory file

A new file `.concept-graph/RESCUE-STATUS.md` holds a refresh-able inventory:

```
# Rescue Status

**Last refreshed:** <cfdb run timestamp>
**Baseline proof:** `.proofs/baseline-<date>.txt`

### Counts
- HSB candidates (Pattern A): NN
- VSB candidates (Pattern B): NN
- Canonical bypasses (Pattern C): NN
- Total classified findings: NN

### Distribution by class
| Class | Count |
|---|---|
| Duplicated feature | NN |
| Unfinished refactor | NN |
| Random scattering | NN |
| Canonical bypass | NN |
| Unwired | NN |

### Operate-module candidates
(bounded contexts that crossed infection threshold — §A3.2 is context-scoped, not crate-scoped)
- context-name-1: <reason>, crates: [crate-a, crate-b, crate-c]
- context-name-2: <reason>, crates: [crate-d, crate-e]

### Top 5 infected crates (by total finding count)
1. ...

### Active raid plans
- <pointer to each draft raid plan>
```

**Update cadence:** refreshed by the v0.1 CI gate on every merge to develop, plus a weekly cron (RFC §11 "weekly audit cron").

**Format:** markdown for human + session-agent consumption, NOT machine-parsed. A machine-readable JSON sibling (`.concept-graph/RESCUE-STATUS.json`) may ship post-v0.2 if a consumer materializes.

#### A4.3 What is NOT in session bootstrap

- **No Redis state** — this is repo-local doctrine, not cross-session memory
- **No CLAUDE.md scars for individual findings** — the status file holds counts, scars go in code as test fixtures
- **No per-session cfdb run** — the tool runs in CI + cron, session agents read the published status

---

### A5. Decisions for council — FIRST PASS RESOLVED

All Q9–Q14 were voted by the first council pass (2026-04-14). Section retained as historical record. Original rationale below, annotated with vote outcomes.

**First-pass vote summary:**

| Q | Topic | Vote | Outcome |
|---|---|---|---|
| Q9 | `ra-ap-hir` schedule | (b) split — UNANIMOUS (clean-arch + rust-systems) + rust-systems architectural reframing | **RESOLVED:** v0.2 ships new parallel `cfdb-hir-extractor` crate; v0.3 switches CI gate |
| Q10 | Taxonomy classes | 6-class (5 + Context Homonym) — ddd-specialist | **RESOLVED:** 6 classes, §A2.1 updated |
| Q11 | Threshold metric | Absolute + LoC telemetry — solid-architect | **RESOLVED:** absolute for v0.2, density instrumentation in v0.2 for v0.3 flip |
| Q13 | `/operate-module` new vs extend | (a) new, conditional on SRP split — solid-architect | **RESOLVED:** new skill after SRP decomposition (§A3.4) |
| Q14 | `/port-epic` new skill vs variant | Variant flag — UNANIMOUS (clean-arch + solid-architect) | **RESOLVED:** `--mode=port --raid-plan=<path>` on existing `/sweep-epic` |

**Residual question from first pass (not voted, deferred to second pass or v0.2 telemetry):**

- **Q12 (RESCUE-STATUS.md live file vs generated)** — companion deliverable scope, not blocking the RFC. Defer to session-bootstrap review in second pass.

---

*Original council question rationale below (for historical traceability).*



#### Q9. `ra-ap-hir` adoption — v0.2 or split into v0.2 prepare / v0.3 ship?

*(Rust systems lens, clean-architect lens)*

Cost per A1.2: +30–60s clean build, +2–4 GB memory. Benefit: unlocks Patterns B, C, G, H, I. Risk: compile cost on every CI run of the extractor.

Options:
- (a) **v0.2 ships `ra-ap-hir` + all call-graph patterns in one release** — biggest jump, largest risk
- (b) **v0.2 ships `ra-ap-hir` extractor only (+ `:EntryPoint` catalog); v0.3 ships Pattern B/C cypher rules** — de-risks by splitting the load-bearing dependency from the rule library
- (c) **v0.2 skips `ra-ap-hir`, delivers `:EntryPoint` via heuristic + syn, v0.3 upgrades to ra-ap-hir** — lowest risk, but delivers Pattern B with the ~40–60% recall ceiling, which is below the v0.2-4 gate item's 80% target

**Recommendation to council:** (b). Rationale: the call-graph extractor is where the real risk lives (memory, compile time, hir stability). Ship the extractor first, prove it stable for a release, then layer rules on top.

#### Q10. Five classes — right cut or different taxonomy?

*(Clean-arch lens, DDD lens, product/CPO lens)*

The five classes in A2.1 map 1:1 to fix strategies. Alternatives:

- **3-class version** (duplicated / refactor / other) — simpler, but loses the canonical-bypass and unwired distinctions that map to different skills
- **7-class version** (add "test-only double that leaked into prod" and "ADR reversal not propagated") — more precise, but two of the classes cover <5% of historical findings
- **DDD-lens version** (aggregate boundary violation / ubiquitous-language drift / context-mapping inconsistency) — more conceptually clean, but harder to detect mechanically

**Recommendation to council:** 5-class as proposed. Simplicity + empirical distribution coverage + 1:1 skill mapping.

#### Q11. Infection threshold — absolute counts or per-kloc density?

*(Solid-architect lens, rust-systems lens)*

A3.2 uses absolute counts. Alternative: per-kloc density (5 bypasses per 1k LoC). Absolute counts are simpler to reason about and interpret. Per-kloc density adjusts for crate size, so a small crate with 3 bypasses is flagged while a large crate with 20 bypasses (but proportionally fewer) is not.

**Recommendation to council:** start with absolute counts (A3.2), instrument density telemetry in v0.2, flip to density in v0.3 if the absolute-count version produces false alarms on small crates.

#### Q12. RESCUE-STATUS.md — live file committed to repo, or generated artifact in CI output?

*(QA lens, product lens)*

Committing generated content to the repo is usually an anti-pattern — it creates diff noise on every refresh. BUT: session agents need to *read* this file at session start without running the tool, which means it must exist in the working copy.

- (a) **Committed to repo, refreshed by a CI job that commits back** — always available, generates diff noise
- (b) **Not committed, refreshed at session start via hook** — no noise, but adds tool dependency to every session and breaks offline work
- (c) **Committed once as scaffold, refreshed by CI into a sibling `.generated` copy, scaffold pointer updated occasionally** — compromise

**Recommendation to council:** (a). The diff noise is acceptable because the refresh is low-frequency (weekly + merge-triggered) and the content is load-bearing for session bootstrapping.

#### Q13. `/operate-module` — new skill, or extend an existing skill?

*(Solid-architect lens — SRP)*

Options:
- (a) **New skill `/operate-module`** — clean SRP, clear trigger, clear output
- (b) **Extend `/audit-split-brain`** with a threshold mode — reuses existing skill, but `/audit-split-brain` is currently read-only while `/operate-module` produces a planning artifact
- (c) **Extend `/quality-architecture`** — same concern as (b)

**Recommendation to council:** (a). The "produce a planning artifact that triggers council" workflow is novel enough to deserve its own skill. Reusing audit skills for planning violates SRP.

#### Q14. Should `/port-epic` be a new skill or a variant of `/sweep-epic`?

*(Clean-arch lens)*

`/sweep-epic` handles mechanical refactors in parallel. `/port-epic` would handle "move code carefully from cancer site to clean new home per an RFC". Overlap: both parallelize mechanical changes. Difference: `/port-epic` has an approved architecture target and a portage list from a raid plan; `/sweep-epic` has a pattern to apply and a list of sites.

**Recommendation to council:** start as a variant flag on `/sweep-epic` (`--mode=port --raid-plan=...`), promote to standalone skill if the variant accumulates ≥3 unique responsibilities.

---

### A6. Risks & known unknowns

- **`ra-ap-hir` weekly maintenance tax** (council B2, upgraded from risk to acknowledged category concern) — weekly releases (9 in 9 weeks verified) require 10+ exact-pinned Cargo.toml version updates per upgrade. Mitigation: dedicated `chore/ra-ap-upgrade-<version>` branch protocol, ~4 breaking changes per year expected, runbook at `.concept-graph/cfdb/docs/ra-ap-upgrade-protocol.md`. This is a budgeted cost, not a risk we hope to avoid.
- **`ra-ap-hir` API stability** — ~4 breaking changes per year historically. Mitigation: `cfdb-hir-extractor` isolated in its own crate; the `syn`-based `cfdb-extractor` continues to function during hir outages. Pin to last-known-good, upgrade deliberately.
- **`HirDatabase` object-safety constraint** (council risk 2, rust-systems) — not trait-object-safe, must be used as a monomorphic concrete type. cfdb-hir-extractor honors this; no `dyn HirDatabase` abstraction.
- **Classifier false positives** (council WARN-5, ddd) — the age-delta + keyword-match signals for "unfinished refactoring" could mislabel a recent bug fix as a refactor. Compounded risk: keyword-match not scoped to owning bounded context will fire "unfinished refactoring" on homonym cases. Mitigation: (a) ship confidence scores, (b) require human review above threshold, (c) scope RFC keyword match to owning bounded context via `.cfdb/concepts/*.toml`, (d) instrument misclassification telemetry.
- **Bounded context identification heuristic fails** — the crate-prefix convention works for `trading`, `portfolio`, `strategy`, `collection`, but fails for cross-cutting concerns like `messenger`, `sizer`, `executor`. Mitigation: explicit override in `.cfdb/concepts/*.toml` is load-bearing for these cases; v0.2 must ship the override mechanism, not only the heuristic.
- **Threshold calibration** — the §A3.2 numbers are initial guesses. Mitigation: v0.2 ships LoC telemetry alongside thresholds so v0.3 can flip to density-based without re-extraction.
- **Raid plan staleness** — a raid plan emitted on date D may not match repo state on date D+14 if other work lands. Mitigation: raid plans expire after N days, `/operate-module` must be re-run to refresh.
- **Skill seam between `duplicated_feature` and `canonical_bypass`** (council WARN-1, solid) — both route to `/sweep-epic` in the initial routing table but have different fix procedures (canonical-selection vs pure rewire). Known seam, acknowledged, promote to separate skill when canonical-selection workflow is enriched.
- **CI memory budget** — `cfdb extract` on the target workspace with HirDatabase loaded consumes 2–4 GB peak RSS (plausible, unverified). If the CI runner cannot accommodate, the extract either streams or extracts per-context. Mitigation: v0.2-5b measures this before committing.

---

### A7. Out of scope for this addendum

- LLM enrichment of findings (v0.3+)
- Cross-project classifier (multi-project v0.3+)
- Embedding-based concept clustering (v0.4)
- IDE integration / live hints
- Auto-fix (cfdb stays read-only; `/operate-module` produces plans, never edits)

---

### A8. Appendix — companion deliverables

This addendum is paired with:

1. **`<consuming-project>/CLAUDE.md` §12 edit** — project doctrine section, drafted in companion edit
2. **`.concept-graph/RESCUE-STATUS.md` scaffold** — empty-state live inventory file
3. **Issue drafts:**
   - New child of EPIC #3622: "Promote `hsb-by-name.cypher` to v0.1 gate with enriched collect()"
   - Standalone: "Capture true-count baseline post forged-file deletion"
   - EPIC #3622 body update: "Add Phase D — v0.2 vertical + taxonomy (blocked on this addendum)"
   - EPIC #3519 body update: "Cross-reference taxonomy classifier per this addendum §A2"

All companion deliverables are DRAFT pending council approval of this addendum.

---


**End of RFC.** Council convenes per §1; decisions are §14; convergence target is a vote on §14 plus a "must-fix before v0.1" list. On council acceptance, work begins with the cfdb workspace scaffold + the chosen Q1 use case integration.
