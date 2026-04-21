# Verdict — solid-architect

## Read log
- [x] docs/RFC-034-query-dsl.md
- [x] council/49/BRIEF.md
- [x] crates/cfdb-query/src/lib.rs
- [x] crates/cfdb-query/Cargo.toml
- [x] crates/cfdb-query/src/skill_routing.rs
- [x] crates/cfdb-query/src/inventory.rs
- [x] crates/cfdb-query/src/list_items.rs
- [x] crates/cfdb-query/src/shape_lint.rs
- [x] crates/cfdb-concepts/Cargo.toml
- [x] crates/cfdb-core/Cargo.toml
- [x] crates/cfdb-cli/Cargo.toml
- [x] crates/cfdb-petgraph/Cargo.toml
- [x] council/verdicts/solid.md (prior council format reference)

---

## Reframe

VERDICT: RATIFY (with one condition — see Change Request 1)

The "no new crate" reframe is justified on CRP grounds: the consumer population for a hypothetical `cfdb-query-dsl` crate would be exactly one (cfdb-cli), and cfdb-cli already depends on cfdb-query. Creating a pass-through crate for a ~200 LOC module forces cfdb-cli to carry a new dep edge that adds no isolation benefit. The REP principle requires each proposed crate to be independently versionable and releaseable; a crate that has exactly one consumer and zero external dependents cannot be independently released in any meaningful sense. The reframe is SOLID-correct.

The RFC's claim that extending cfdb-query is a "natural extension" because it "already reads `.cfdb/skill-routing.toml`" (RFC §5.1) requires scrutiny — see Q-SOLID-1 below.

---

## Per-lens questions

### Q-SOLID-1: Does extending cfdb-query with a 7th responsibility cross the SRP threshold?

**Enumerated responsibilities (current, from lib.rs:1-29 and module files):**

1. `parser` — chumsky-based Cypher-subset parser (lib.rs:18, parser/mod.rs)
2. `builder` — fluent Rust API producing the same AST (lib.rs:15, builder/mod.rs)
3. `inventory` — DebtClass / Finding / ScopeInventory domain types (lib.rs:16, inventory.rs:1)
4. `shape_lint` — pre-eval shape linting pass (lib.rs:19, shape_lint.rs:1)
5. `skill_routing` — SkillRoutingTable TOML loader for `.cfdb/skill-routing.toml` (lib.rs:20, skill_routing.rs:1)
6. `list_items` — list_items_matching query composer (lib.rs:17, list_items.rs:1)

The RFC proposes adding:

7. `param_resolver` — TOML loader for `.cfdb/concepts/*.toml` → `Param::List` bindings (RFC §3.1)

**RFC §5.3's stated axis of change:** "produce `cfdb_core::Query` AST inputs from external inputs (text, fluent API, TOML, CLI)." This framing lumps all seven responsibilities under "Query production."

**SRP stress test:** does responsibility (7) share the same change vector as responsibilities (1)–(6)?

| Responsibility | Reason to change |
|---|---|
| parser (1) | Cypher subset grammar evolves |
| builder (2) | `cfdb_core::Query` AST evolves |
| inventory (3) | Debt-class taxonomy (§A2.1) evolves |
| shape_lint (4) | New footgun patterns discovered |
| skill_routing (5) | `.cfdb/skill-routing.toml` schema evolves |
| list_items (6) | `list_items_matching` verb contract evolves |
| param_resolver (7) | `.cfdb/concepts/*.toml` TOML schema evolves OR param form syntax evolves |

Responsibility (3) — `inventory` / DebtClass — already has a distinct change vector from (1) and (2): it changes when the §A2.1 debt taxonomy evolves, which is a DDD-level decision, not a Cypher-grammar decision. Responsibility (5) changes when the skill-routing policy changes. Responsibility (7) changes when the concept TOML schema changes OR when the `context:/regex:/literal:/list:` param-form syntax changes.

**SRP threshold question: is there a documented quantitative threshold?**

No documented cfdb-specific SRP threshold exists. The prior solid-architect verdict (council/verdicts/solid.md) used a qualitative "reasons-to-change" test, not a count ceiling. The relevant metric gate tools (quality-architecture) enforce god-file/fat-trait/fat-enum thresholds on individual files — not responsibility-count per crate.

**Quantitative observation:** the `cfdb-query` crate has 7 modules (6 today + 1 proposed), and the total public type surface is: parser: 2, builder: ~24, inventory: 9, shape_lint: 2, skill_routing: 7, list_items: 1, plus the proposed param_resolver types (2 fns + 1 error enum ≈ 4 items). Total public surface ≈ 49 items across 7 modules. No single module is a god-file.

**CCP analysis:** responsibilities (1), (2), (4), and (6) are tightly CCP-aligned — they all change when the Query AST or evaluation semantics change. Responsibilities (3), (5), and (7) are NOT in this cluster: they change when external policy files or taxonomy decisions change, which is a different change vector.

**Verdict on Q-SOLID-1:** The SRP threshold is NOT crossed in a disqualifying sense. However, the RFC's "natural extension" argument is WEAK: the correct justification is CCP-approximate (all TOML loaders that serve cfdb-query consumers change when `.cfdb/` schema changes), not "we already read one TOML file so reading another is the same." The current cfdb-query skill_routing module (skill_routing.rs:1-148) reads `.cfdb/skill-routing.toml` via the `toml` crate directly — it does NOT depend on cfdb-concepts. The param_resolver MUST delegate to `cfdb_concepts::load_concept_overrides` (RFC §4.6), which requires adding `cfdb-concepts` as a NEW dependency to `cfdb-query` (currently absent — cfdb-query/Cargo.toml has no cfdb-concepts entry).

This is a real stability cost: see the SDP analysis below.

---

## SDP (Stable Dependencies Principle) analysis

**Current cfdb-query stability metrics (from Cargo.toml files):**

| Crate | Ca (afferent) | Ce (efferent, cfdb-internal) | I = Ce/(Ca+Ce) | Notes |
|---|---|---|---|---|
| cfdb-core | 4+ | 0 | 0.00 | schema vocab — maximally stable |
| cfdb-concepts | 2 (cfdb-extractor, cfdb-petgraph) | 0 | 0.00 | TOML loader — stable |
| cfdb-query | 2 (cfdb-cli, cfdb-petgraph[dev]) | 1 (cfdb-core) | 0.33 | moderately stable |
| cfdb-petgraph | 1 (cfdb-cli) | 2 (cfdb-core, cfdb-concepts) | 0.67 | unstable — correct, impl layer |
| cfdb-cli | 0 | 4 (cfdb-core, cfdb-query, cfdb-petgraph, cfdb-extractor) | 1.00 | composition root |

**After RFC-034 (adding cfdb-concepts to cfdb-query):**

| Crate | Ca | Ce | I = Ce/(Ca+Ce) | Delta |
|---|---|---|---|---|
| cfdb-concepts | 3 | 0 | 0.00 | Ca+1 (cfdb-query added) |
| cfdb-query | 2 | 2 (cfdb-core + cfdb-concepts) | 0.50 | I rises from 0.33 → 0.50 |

**SDP check:** cfdb-query (I=0.50) would depend on cfdb-concepts (I=0.00). Direction: unstable → stable. SDP is SATISFIED — this is the correct direction.

**SAP / Main Sequence check for cfdb-query after change:**

- A (abstract types / total pub types) ≈ 0 (no pub traits, mostly concrete structs/enums/fns). A ≈ 0.
- I = 0.50 after change.
- D = |A + I - 1| = |0 + 0.50 - 1| = 0.50. This is in the Zone of Pain (D > 0.3, low A, moderate I).

**Zone of Pain diagnosis:** cfdb-query is neither maximally stable nor maximally abstract. The Zone of Pain signal here reflects a real structural issue: cfdb-query already mixes stable-abstract responsibilities (parser, builder) with concrete-loader responsibilities (skill_routing, and proposed param_resolver). Adding param_resolver increases I without increasing A, pushing D from ~0.4 → 0.50.

**However:** the Zone of Pain threshold (D > 0.3) is already violated BEFORE this RFC, since current I=0.33 and A≈0 gives D=0.67... wait, let me re-check. With I=0.33 and A≈0: D = |0 + 0.33 - 1| = 0.67. Current cfdb-query is already deep in the Zone of Pain.

This pre-existing D=0.67 is the correct signal: cfdb-query is concrete (A≈0) and already has moderate instability. The RFC proposal moves I from 0.33 to 0.50, which actually improves D from 0.67 to 0.50. So the RFC marginally IMPROVES the Main Sequence distance for cfdb-query, not degrades it.

**Corrected SAP verdict:** adding cfdb-concepts dependency marginally improves cfdb-query's Main Sequence distance. The concern is not SAP degradation — the concern is whether adding the cfdb-concepts dep creates a novel split-brain risk.

---

## Split-brain risk: param_resolver placement

**RFC §4.6 (Canonical-bypass non-regression):** "the param resolver MUST read `.cfdb/concepts/*.toml` via `cfdb_concepts::load_concept_overrides` — the canonical loader shipped in #3. A second inline TOML parser in `cfdb-query` is a forbidden move."

This invariant is correct and necessary. cfdb-concepts/Cargo.toml comment (line 15-17) explicitly states it was extracted from cfdb-extractor to prevent split-brain between cfdb-extractor and "the future `cfdb-query` DSL evaluator." The cfdb-concepts crate was designed for exactly this consumer.

However, the RFC places the param_resolver in `cfdb-query` rather than `cfdb-cli`. This requires cfdb-query to take a new cfdb-concepts dep. The alternative placement — putting param_resolver in `cfdb-cli` — would:
- Keep cfdb-query dep surface unchanged (Ce=1, I=0.33)
- cfdb-cli already depends on cfdb-query AND cfdb-petgraph (which already depends on cfdb-concepts)
- The verb handler (`check_predicate.rs`) is in cfdb-cli regardless; the param_resolver is a ~200 LOC helper
- Precedent: cfdb-cli/src/check.rs is the "prototype check verb" already containing similar CLI-bridging logic

**CRP analysis for the alternative placement:**

If param_resolver is in cfdb-cli:
- cfdb-query consumers (cfdb-petgraph, any future crate) do NOT get param_resolver for free — they'd have to dep on cfdb-cli or re-implement. Given that `cfdb-petgraph` is the backend and would never call a param_resolver (it executes queries, not resolves params), there are NO cfdb-query consumers that would ever use param_resolver.
- CRP: "Don't force users to depend on things they don't need." If param_resolver is in cfdb-query, every future consumer of cfdb-query (e.g., a hypothetical second backend) is forced to transitively accept cfdb-concepts for a feature they don't use.

**Conclusion:** placing param_resolver in `cfdb-cli` is the CRP-correct alternative. cfdb-query's job is "produce Query AST values"; resolving CLI string forms of params into Query.params is a CLI-layer concern. The skill_routing.rs precedent (responsibility 5) is in cfdb-query to serve cfdb-cli consumers — but its API is exposed through cfdb-query's pub surface. The param_resolver serves only cfdb-cli.

**Change Request 1 (see below).**

---

## Cross-lens binding

### Issue decomposition (§7)

The 5 slices are well-sequenced. The dependency graph (slice-1 + slice-2 → slice-3 → slice-4 → slice-5) is acyclic and correct. No ADP violation. Slices 1 and 2 having no file overlap is verified: slice-1 touches crates/cfdb-query/src/param_resolver.rs and slice-2 touches .cfdb/predicates/ only.

One issue with slice-3's `Tests: Self dogfood` block: it asserts "≥10 File rows" from `cfdb check-predicate --name path-regex --param pat:literal:'cfdb-query/.*\\.rs'`. This assertion is correct but note the path regex form `literal:` is used for what is semantically a regex param. The CLI param form spec (RFC §3.4) uses `regex:<pattern>` for `Param::Scalar(PropValue::Str(pattern))` — the test uses `literal:` for the same purpose. Both forms produce a scalar string, but the test should use `regex:` for clarity given the param is consumed by a Cypher `=~` operator. This is a minor documentation inconsistency, not a blocker.

Slice-4's `Tests: Target dogfood` row correctly says "No assertion; the bar is 'the run succeeds, producing deterministic output.'" This is the appropriate escape hatch since qbot-core's violation baseline is not owned by cfdb.

### Non-goals (§6)

The "Not a Shell-grep escape hatch" non-goal is a conscious reframe of #49's original requirement. The RFC answers #49's "must have escape hatch to shell-grep for simple file-path checks" with `MATCH (f:File) WHERE f.path =~ $pat`. This is the correct architectural answer — shell-grep is non-deterministic (depends on OS grep behavior, locale, file ordering) whereas Cypher path regex is deterministic and auditable. RATIFY this non-goal.

The "Not a UDF framework" non-goal is correct. The param_resolver is a SPECIFIC loader, not a general extension mechanism. Adding a UDF registry would introduce an OCP violation on cfdb-query's param_resolver surface — future RFC territory.

The "Not a namespacing scheme" non-goal (flat `.cfdb/predicates/`) is appropriate for this RFC's scope. However, this creates a future debt risk: if cfdb gains 50+ predicates, the flat directory will become unmanageable. A future RFC should address. No action required here.

### Invariants (§4)

§4.5 (Param-resolver hermeticity): "resolve_param(workspace_root, cli_arg) has a pure-function signature." The type signature in RFC §3.1 takes `&std::path::Path` (workspace_root) and `&str` (cli_arg), returns `Result<(String, cfdb_core::query::Param), ParamResolveError>`. This is I/O-dependent (reads TOML files) — it is NOT a pure function in the mathematical sense. The invariant that it "does NOT shell out, does NOT make network calls, does NOT consult environment variables" is correct, but calling this "pure-function" is misleading. The hermeticity invariant is correct; the "pure function" terminology is inaccurate. Recommendation: §4.5 should say "hermetic filesystem reader" not "pure-function signature."

§4.6 (Canonical-bypass non-regression): the prohibition on "a second inline TOML parser in cfdb-query" is load-bearing. Implementers must be reminded this means the cfdb-concepts dependency MUST be wired in Cargo.toml — it cannot be satisfied by re-implementing the loader inline.

---

## Overall

VERDICT: REQUEST CHANGES

The RFC is architecturally sound in its reframe decision (no new crate, no new grammar, no new evaluator primitive). The stability metrics confirm the SDP direction is correct and the Main Sequence distance is marginally improved. However, one concrete architectural change is required before ratification:

**Change Request 1 (CRP violation — param_resolver placement):**

Move `param_resolver` module from `cfdb-query` to `cfdb-cli`. Evidence:
- `cfdb-query` has ZERO consumers that need param resolution (cfdb-petgraph consumes cfdb-query as a dev-dep for tests only; cfdb-cli is the sole runtime consumer — cfdb-cli/Cargo.toml). 
- Adding cfdb-concepts to cfdb-query forces every future cfdb-query consumer to accept a TOML-loader + context-resolution dep they will never use.
- The cfdb-concepts/Cargo.toml (lines 14-17) comment says the crate was designed for "the future `cfdb-query` DSL evaluator" — but the RFC explicitly states there is NO new evaluator (RFC §2.2: "No new evaluator primitive"). The historical anticipation in the comment does not obligate this placement now.
- The skill_routing module in cfdb-query serves cfdb-cli consumers because skill routing is also useful to any future tool-side consumer of `DebtClass` data. param_resolver serves only the `cfdb check-predicate` verb.
- CRP says "don't force users to depend on things they don't need." If param_resolver is in cfdb-query, cfdb-query's I rises from 0.33 to 0.50; if it is in cfdb-cli, cfdb-query's stability is preserved.

**Mechanical resolution:** move the `param_resolver.rs` types/fns to `cfdb-cli/src/param_resolver.rs`. Update RFC §3.1 to show the module path as `crates/cfdb-cli/src/param_resolver.rs`. Update RFC §5.1 and §5.3 placement rationale accordingly. Update Slice-1 scope to read "add `crates/cfdb-cli/src/param_resolver.rs`" instead of `crates/cfdb-query/src/param_resolver.rs`. cfdb-cli already depends on cfdb-concepts transitively through cfdb-petgraph (cfdb-petgraph/Cargo.toml line 17), but should add a direct dep for clarity.

If the author has a specific reason why param_resolver needs to be in cfdb-query (e.g., a planned second consumer), document it in RFC §5.3 with the consumer name. Absent that evidence, the CRP-correct placement is cfdb-cli.

---

## Peer concerns

**For clean-arch lens:** Q-CA-1 asks "is `cfdb-cli → cfdb-query → cfdb-concepts` acceptable?" The correct question is whether cfdb-concepts should enter cfdb-query at all. clean-arch should confirm whether the composition root (cfdb-cli) is the correct placement for the param_resolver, consistent with the principle that filesystem-reading "TOML loaders are composition-layer concerns" (RFC §5.1 already states this). The current RFC then contradicts itself by proposing to put the TOML loader in cfdb-query instead of the composition layer (cfdb-cli).

**For ddd-specialist lens:** the `cfdb-concepts/Cargo.toml` comment (lines 14-17) states the crate was designed for `cfdb-query` consumers. DDD should confirm whether that historical anticipation remains architecturally correct now that the RFC explicitly rejects a DSL evaluator — or whether the "future evaluator" scenario has been superseded.
