# Verdict — ddd-specialist

## Read log

- [x] docs/RFC-034-query-dsl.md
- [x] council/49/BRIEF.md
- [x] council/RATIFIED.md
- [x] crates/cfdb-core/src/query/ast.rs
- [x] crates/cfdb-cli/src/check.rs
- [x] .cfdb/concepts/cfdb.toml
- [x] crates/cfdb-concepts/src/lib.rs

---

## Reframe

**VERDICT: RATIFY**

The RFC's reframe — "no new DSL grammar, no new crate, extend Cypher + param resolver + predicate library" — is the correct DDD move. A new `cfdb-query-dsl` crate would create a second in-process language boundary for expressing graph predicates inside the same bounded context (`cfdb`), splitting the linguistic authority for "how queries are expressed" across two crates with no bounded-context justification. The ubiquitous language of the `cfdb` context already has the term "query" (for any Cypher expression against the graph) and the reframe correctly identifies that the predicate library is a storage concern (named templates on disk), not a new grammar concern. That observation is a clean DDD separation and the reframe honours it. RFC §2.2 "No new DSL grammar" and §2.2 "No new crate" (docs/RFC-034-query-dsl.md:48–53) are both load-bearing DDD decisions I support.

---

## Per-lens questions

### Q-DDD-1: `Predicate` (AST node) vs `predicate` (file) homonym

**Finding: ACCEPTABLE HOMONYM — same context, different layers, requires explicit documentation. No rename needed.**

The two usages are:

1. `cfdb_core::query::Predicate` — the AST enum for `WHERE` clause nodes (crates/cfdb-core/src/query/ast.rs:120–154). It is an in-memory structural concept: a tree of `Compare`, `And`, `Or`, `Not`, `NotExists`, etc. nodes. This is the **query-AST layer** meaning of "predicate".

2. `.cfdb/predicates/<name>.cypher` — a named file on disk holding a parameterised Cypher template that, when evaluated, checks a proposition about the graph. This is the **predicate-library layer** meaning of "predicate".

The homonym risk assessment depends on whether these two usages live in the same bounded context or different ones. Both are unambiguously inside the `cfdb` bounded context (`.cfdb/concepts/cfdb.toml:12–30` declares all `cfdb-*` crates + `cfdb-query` + `cfdb-cli` as a single context). There is no cross-context linguistic contamination here.

Within the same bounded context, the standard DDD resolution for layer-differentiated homonyms is explicit naming convention, not renaming the concept entirely. The two usages occupy distinct layers: AST node (in-memory, structural) vs predicate template (on-disk, storage artefact). A reader who understands the layering will not confuse them because the AST node is a Rust type and the template is a file. The RFC already acknowledges this in §5.2 (docs/RFC-034-query-dsl.md:255–257) and mandates documentation in `docs/query-dsl.md` and the homonym note in Slice 5 (docs/RFC-034-query-dsl.md:369).

**Why a rename would be worse than the documentation path:**

Renaming `.cfdb/predicates/` to `.cfdb/rules/` conflates the concept with `.cfdb/queries/` (self-hosted ban rules run by `cfdb violations`) and with future logical inference rules — both of which are different concepts. "Rules" is overloaded more broadly in the domain expert's vocabulary than "predicate". `.cfdb/checks/` collides with the existing `cfdb check --trigger` verb root. `.cfdb/asserts/` introduces a new term not present in any existing cfdb vocabulary.

**Condition on ratification:** the homonym note prescribed in Slice 5 (docs/RFC-034-query-dsl.md:369) is non-negotiable. The documentation in `docs/query-dsl.md` MUST contain a section titled "Predicate (file) vs Predicate (AST node)" that explains the layering explicitly before the first code example. Slice 5 already carries this mandate; the issue body filed from this RFC must reproduce it verbatim in the `Tests:` block.

### Q-DDD-2: `cfdb check-predicate` vs `cfdb check --trigger T1`

**Finding: DISTINCT BOUNDED CONTEXTS WITHIN cfdb — different verb roots are correct, merging would be harmful.**

The two verbs:

1. `cfdb check --trigger T1` (crates/cfdb-cli/src/check.rs:1–40) — editorial-drift detection. Operand is a `TriggerId` (capital-T, T1/T3 variants). Answers "Is the TOML concept registry drifting from the code?" The trigger drives Rust-side anti-join logic over a closed, versioned registry of Cypher rules (check.rs:54–73). Output shape: structured JSON payload with verdict/correlation columns.

2. `cfdb check-predicate --name X` (RFC-034 §3.4) — named predicate execution. Operand is a file basename from `.cfdb/predicates/`. Answers "Does the graph satisfy this named proposition?" The predicate is a user-authored Cypher template with `$param` bindings. Output shape: three-column violation format (qname, line, reason) — same as `cfdb violations`.

These are **operationally distinct** and their overlap is surface-level (both run Cypher). The DDD analysis:

- `check --trigger` belongs to the "concept-registry health" sub-context: it detects drift between the declared concept map and the actual code graph. It is closed (only T-series triggers), versioned (TriggerId variants are enumerated and tested for stable ordering — check.rs:629–638), and its output schema is deliberate (verdict/correlation columns chosen for `/operate-module` consumption per RATIFIED.md §A.15).

- `check-predicate` belongs to the "Non-negotiable predicate library" sub-context: it executes open-ended propositions authored by downstream consumers (qbot-core's `check-prelude-consistency` skill). It is open (any `.cfdb/predicates/*.cypher` file), user-extensible, and its output schema is the standard violation three-column format.

Merging them into one verb would force a single output schema on two things that must evolve independently: the closed trigger registry (which must stay backward-compatible with `/operate-module` input) and the open predicate library (which must stay backward-compatible with `check-prelude-consistency` skill). Different change vectors, different consumers, different verb roots — the RFC's default of keeping them separate is correct DDD.

The verb-root question: `check-predicate` vs `violations` vs something else. The RFC acknowledges the surface overlap with `cfdb violations --rule <path>` in §9 Q-DDD-2/Q-CA-2 (docs/RFC-034-query-dsl.md:407). My position: the `check-predicate` root is load-bearing because it signals "I am asking a boolean question about a named proposition" rather than "I am enforcing a ban rule". The predicate library is semantically assertional (a predicate either holds or does not), whereas ban rules are prohibitive (violations are things that must not exist). This semantic distinction is worth preserving in the verb name. Merging the verbs would erase it.

---

## Cross-lens binding

### Issue decomposition (§7)

The five slices are well-sequenced and correctly sized. Three specific observations:

1. **Slice 1 `Tests:` — Self-dogfood assertion is correct but should be tightened.** The prescribed test `resolve_params(workspace_root=cfdb_root, ["--param", "ctx:context:cfdb"])` asserts "exact sorted crate list" against `.cfdb/concepts/cfdb.toml`. This is the right canonical-authority canary. It will catch any divergence between `cfdb_concepts::load_concept_overrides` and the param resolver (invariant 4.6). Canonical-bypass risk is LOW because the RFC explicitly requires delegation to `load_concept_overrides` (docs/RFC-034-query-dsl.md:231). The test is the enforcement mechanism — it must assert exact crate membership, not just non-empty.

2. **Slice 4 is the critical DDD gate.** The predicate-library dogfood + determinism check (Slice 4) is the operational proof that the predicate library belongs to the `cfdb` bounded context: it runs cfdb-owned predicates against cfdb's own keyspace. The "byte-identical stdout" invariant is DDD-significant because it proves the predicate-library layer does not introduce non-determinism into the context's outputs. Slice 4 should not be deferred.

3. **Slice 2 `Tests:` — parse test is necessary but not sufficient.** The self-dogfood in Slice 2 asserts every seed predicate parses with zero `ParseError`. This is correct. However, the parse test does not verify that the seed predicates actually express valid Cypher against the v0.2 schema (e.g. that `RE_EXPORTS`, `IMPLEMENTS_FOR`, `DECLARED_IN` are real edge labels). A schema-reference test (asserts that every edge label and node label used in seed files appears in `cfdb_core::schema`) would close this gap. This is a REQUEST CHANGES candidate — I am flagging it but not blocking ratification on it, because the Slice 4 dogfood run will catch schema-reference errors at execution time. The slice 2 schema-reference test is a nice-to-have, not a blocker.

### Non-goals (§6)

All non-goals are sound from a DDD perspective:

- "Not a DSL grammar" correctly preserves the linguistic unity of the `cfdb-query` bounded context. Introducing a separate grammar would bifurcate the language.
- "Not a template composition system" (no `INCLUDE`/`MACRO`) is correct. Template composition is a future RFC concern that may require a new bounded context if it introduces its own language. Deferring it avoids premature context boundary creation.
- "Not `.cfdb/queries/` extension" is a critical non-goal. `.cfdb/queries/` (self-hosted ban rules, consumed by `cfdb violations`) and `.cfdb/predicates/` (Non-negotiable predicates, consumed by `cfdb check-predicate`) have different consumers and different change vectors. Merging the directories would collapse two distinct sub-contexts into one. The RFC's separation is correct.
- "Not a Shell-grep escape hatch via shell-out" — the RFC re-frames #49's shell-grep requirement as `MATCH (f:File) WHERE f.path =~ $pat`. This is the right DDD decision: it keeps all predicate evaluation inside the `cfdb` bounded context rather than escaping to a shell subprocess that has no cfdb semantics. I confirm this re-framing is acceptable.

### Invariants (§4)

- **4.5 Param-resolver hermeticity** is the DDD load-bearing invariant. The param resolver must not become a second context-authority. The signature `resolve_param(workspace_root: &Path, cli_arg: &str) -> Result<(String, Param), ParamResolveError>` with delegation to `cfdb_concepts::load_concept_overrides` (docs/RFC-034-query-dsl.md:86–88, 231) is the correct mechanism. Any deviation — inline TOML parsing, environment-variable-based override, alternative file path — is a canonical-bypass and a DDD violation.
- **4.6 Canonical-bypass non-regression** is correctly framed. `cfdb_concepts::load_concept_overrides` (crates/cfdb-concepts/src/lib.rs:125–139) is the single canonical resolver, identified in RATIFIED.md §A.17 as the authority. The RFC forbids a second inline TOML parser in `cfdb-query`. This is enforced by the Slice 1 self-dogfood test — the canary.
- **4.4 Keyspace backward-compat** — the RFC correctly notes that future schema-bumping RFCs must audit `.cfdb/predicates/*.cypher` for breaking references. This is a process invariant, not a code invariant. The first-line comment convention (§3.5, docs/RFC-034-query-dsl.md:176–193) citing minimum `SchemaVersion` is the right mechanism.

### DebtClass taxonomy

The RFC does not propose a new DebtClass for predicate violations, and this is correct. A predicate returning rows is not a debt class — it is a policy violation in a consumer-defined proposition. The six-class taxonomy (`duplicated_feature`, `context_homonym`, `unfinished_refactor`, `random_scattering`, `canonical_bypass`, `unwired`) covers structural facts about the code graph; predicate violations are semantic assertions by downstream consumers (qbot-core, graph-specs-rust) against that graph. No 7th class is needed. A separate taxonomy at the consumer level (if needed) is qbot-core's concern, not cfdb's.

---

## Overall

**VERDICT: RATIFY**

The RFC is coherent from a DDD perspective. The reframe is correct. The two specific DDD concerns are resolved satisfactorily:

1. The `Predicate`/`predicate` homonym is an acceptable intra-context, inter-layer naming situation — documentation (Slice 5) is the correct resolution, not rename.
2. `cfdb check-predicate` and `cfdb check --trigger T1` belong to distinct sub-contexts (open predicate library vs closed trigger registry) with different consumers and different change vectors — separate verb roots are correct.

One non-blocking request:

1. The issue body for Slice 2 SHOULD include a schema-reference assertion (every edge/node label in seed `.cypher` files appears in `cfdb_core::schema`). This closes the gap between "file parses" and "file references valid schema elements". The gap is partially covered by Slice 4's dogfood run, but an earlier static check would catch schema references on day one.

The canonical-authority invariant (4.5 + 4.6) is the single highest-priority DDD protection in this RFC. The param resolver must delegate to `cfdb_concepts::load_concept_overrides` without exception. The Slice 1 self-dogfood test is the enforcement mechanism; it must assert exact crate membership, not just successful execution.

## Peer concerns

**For clean-arch:** The `cfdb-query` crate's module `param_resolver` will import `cfdb_concepts::load_concept_overrides`. Verify the Cargo.toml dependency direction at implementation time: `cfdb-query` → `cfdb-concepts` must be explicit and not transitively introduced through `cfdb-core`. If `cfdb-concepts` is only reachable transitively through `cfdb-core` today, the explicit dep must be added in Slice 1. A transitive-only path is an implicit dependency that would break if `cfdb-core` ever drops its `cfdb-concepts` dep.

**For rust-systems:** The three seed predicate files use `IMPLEMENTS_FOR`, `RE_EXPORTS`, and `DECLARED_IN` edge labels (docs/RFC-034-query-dsl.md:187–191). Confirm all three are defined in `cfdb_core::schema::EdgeLabel` before Slice 2 merges. If any are absent, the seed files will parse (they are string literals in `.cypher` files) but fail at evaluation time.
