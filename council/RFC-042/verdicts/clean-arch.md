# RFC-042 verdict — clean-arch

**Verdict:** REQUEST CHANGES
**Author:** clean-arch sub-agent
**Date:** 2026-05-17

## D1. Verdict on the RFC as written

### Finding 1 — `entry_kind_filter: Option<&BTreeSet<&str>>` leaks a stringly-typed CLI concept into `cfdb-petgraph` (CHANGE REQUIRED)

RFC §3.3 specifies that `enrich_reachability` gains an `entry_kind_filter: Option<&BTreeSet<&str>>` parameter. The current `run(state: &mut KeyspaceState) -> EnrichReport` signature lives at `crates/cfdb-petgraph/src/enrich/reachability.rs:75` — a `pub(crate)` function inside `cfdb-petgraph`, one layer below the `cfdb-core::EnrichBackend` port.

The proposed parameter type `BTreeSet<&str>` encodes the production-kind set as a stringly-typed caller-supplied contract: the caller passes `Some({"cli_command", "mcp_tool", ...})`. This is a Clean Architecture violation of two kinds:

1. **Port purity.** The `EnrichBackend::enrich_reachability` trait method in `cfdb-core/src/enrich.rs:177` currently takes only `&mut self, keyspace: &Keyspace`. A `BTreeSet<&str>` parameter on the concrete implementation means either (a) the trait method signature changes to accept it — adding an infrastructure-vocabulary string-set into a `cfdb-core` port — or (b) the filter is applied at the `cfdb-petgraph` implementation layer, below the port boundary, meaning the orchestrator (cfdb-cli) cannot control which BFS pass writes which attribute without bypassing the port. Neither option is correct as written.

2. **Abstraction level mismatch.** "Production kinds" is a CLI concern — it names the v0.2 entry-point vocabulary that matters to an operator running `cfdb scope --production-only`. The `cfdb-petgraph` enrichment layer's correct abstraction is at the graph level: "seed from entry points matching this predicate." A `BTreeSet<&str>` is an ad-hoc encoding that embeds the CLI's domain language into the graph layer. The correct boundary type is a domain enum variant defined in `cfdb-core` — e.g. `EntryPointKindFilter::All | EntryPointKindFilter::ProductionOnly` — which `cfdb-core` owns and which `cfdb-cli` maps from the `--production-only` bool.

**Proposed change:** RFC §3.3 last paragraph should be updated to: define an `EntryPointKindFilter` enum in `cfdb-core` (alongside the existing schema vocabulary); update `EnrichBackend::enrich_reachability` to accept `filter: EntryPointKindFilter`; the `cfdb-petgraph` implementation maps `EntryPointKindFilter::ProductionOnly` to the kind exclusion set internally. The `BTreeSet<&str>` is an implementation detail of `cfdb-petgraph` that must not surface at the port boundary.

### Finding 2 — Composition root for dual-BFS is ambiguous (CLARIFICATION REQUIRED)

RFC §3.3 says `enrich_reachability` "is invoked twice from the orchestrator." The word "orchestrator" is ambiguous. Currently:

- `cfdb-cli/src/enrich.rs:59` is the CLI-level composition root that calls `store.enrich_reachability(&ks)` through the `EnrichBackend` port.
- `cfdb-petgraph/src/enrich_backend.rs:151-163` is the adapter that delegates to `crates/cfdb-petgraph/src/enrich/reachability.rs:75` (`run(state)`).

There are two architecturally sound options:

**Option A (preferred):** The CLI orchestrator (`cfdb-cli/src/enrich.rs`) calls `store.enrich_reachability(&ks, EntryPointKindFilter::All)` and `store.enrich_reachability(&ks, EntryPointKindFilter::ProductionOnly)` in sequence, through the updated port signature. The port is the composition boundary; the CLI decides the invocation pattern; `cfdb-petgraph` executes each BFS independently. This keeps dependency direction correct (CLI depends on `cfdb-core` port; `cfdb-petgraph` implements it; neither depends on the other's internals).

**Option B (violates dependency rule):** `cfdb-petgraph`'s `run_reachability` internally loops over both filter variants and writes both attribute sets in one call. This trades away the port's expressibility (one call, one effect) for implementation convenience, and buries a CLI concern (which attribute names to emit for the `--production-only` flag) inside the graph layer. REJECT this option.

The RFC text in §3.3 says "invoked twice from the orchestrator" which is consistent with Option A, but does not explicitly name `cfdb-cli/src/enrich.rs` as the locus. This should be made explicit to prevent the implementer from choosing Option B.

**Proposed change:** RFC §3.3 should add one sentence: "The composition root is `cfdb-cli/src/enrich.rs` (the existing `EnrichVerb::Reachability` dispatch site); both BFS calls go through the `EnrichBackend` port with different `EntryPointKindFilter` values, not through a loop inside `cfdb-petgraph`."

### Finding 3 — `classifier-unwired-production.cypher` duplication is architecturally acceptable

RFC §3.3 last sentence introduces `classifier-unwired-production.cypher` as a duplicate of `classifier-unwired.cypher` (`examples/queries/classifier-unwired.cypher:51-64`). From a clean-arch perspective this is correct: the two queries read different `:Item` attributes (`reachable_from_entry` vs `reachable_from_production_entry`). Each query is a self-contained fact projection — they share the same structural shape but bind different attribute names. A single parameterized query template would be cleaner, but the current Cypher subset has no `${attr_name}` interpolation at the query level, so duplication is the only option expressible in the existing DSL. This is not a Clean Architecture violation; it is a limitation of the embedded query language. No change required.

The classifier orchestrator in `cfdb-cli/src/scope/classifier.rs:18-38` currently holds a fixed array of 6 `(DebtClass, &str)` pairs. Adding the `--production-only` variant requires extending this array (or making it conditional on the flag), which changes the composition root in `cfdb-cli` — correct and expected. The RFC §3.3 statement "the orchestrator picks between them based on the `--production-only` flag" confirms this is a CLI-layer decision. No violation.

### Summary: REQUEST CHANGES

Two changes are required before ratification:
1. Replace `BTreeSet<&str>` with a `cfdb-core` domain type (`EntryPointKindFilter` enum) in the `EnrichBackend::enrich_reachability` port signature.
2. Name `cfdb-cli/src/enrich.rs` explicitly as the dual-BFS composition root in RFC §3.3.

Finding 3 (query duplication) is a non-violation given current DSL constraints — no change needed.

---

## D2. Tests prescription

### Slice 042-A — extractor `:EntryPoint{kind=test|bench}` emission + fixture

- **Unit:** Pure `has_test_attr(&ast::Fn) -> bool` and `has_bench_attr(&ast::Fn) -> bool` assertions on synthetic `ast::Fn` inputs (one per attribute variant from RFC §3.1: `#[test]`, `#[tokio::test]`, `#[async_std::test]`, `#[given]`, `#[when]`, `#[then]`, `#[bench]`, plus negative case `#[other]`). Test site: `crates/cfdb-hir-extractor/src/entry_point_emitter/registers_param.rs` (mirroring the existing `has_tool_attr` unit tests). These are pure predicates — no HIR database needed.
- **Self dogfood (cfdb on cfdb):** `cfdb extract --workspace . --features hir --db .cfdb/db --keyspace cfdb-self` then `MATCH (e:EntryPoint{kind:"test"}) RETURN count(e) AS n` must return `n >= 1`. Lower bound rationale: cfdb has `#[test]` fns in `crates/cfdb-petgraph/src/enrich/reachability/tests.rs` (confirmed: 5+ test fns at that path) so n ≥ 5 is a conservative floor. The assertion is smoke-level: "at least one test entry point was detected in cfdb's own codebase."
- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA 913f06f):** zero regression on all four existing `.cfdb/queries/*.cypher` rules. The rules at `arch-ban-unwrap-domain-ports.cypher`, `arch-context-no-application-in-domain.cypher`, `arch-context-no-cross-layer-unwrap.cypher`, `arch-context-no-syn-in-domain.cypher` all match on `INVOKES_AT` / `CallSite` patterns — none match on `:EntryPoint.kind`. New `test`/`bench` entry-point nodes do not appear in any existing rule's MATCH clause, so zero new rows are expected. Assert: `cfdb violations` returns exit 0 on all four rules against the re-extracted graph-specs-rust.
- **Target dogfood (qbot-core at pinned SHA):** report total `:EntryPoint{kind=test}` and `:EntryPoint{kind=bench}` counts in the PR body. No merge-blocking assertion; reviewer sanity-check only.

### Slice 042-B — scope `--production-only` flag + dual-BFS + classifier rule

- **Unit:** `enrich_reachability::collect_seeds` (after the `EntryPointKindFilter` parameter is added per Finding 1 above) returns the expected subset: with `Filter::All` all entry-point EXPOSES targets are seeds; with `Filter::ProductionOnly` only entry points whose `kind` attribute is NOT in `{test, bench}` are seeds. Test with a synthetic `KeyspaceState` containing two entry points — one `kind=mcp_tool`, one `kind=test` — and assert seed set cardinality 2 vs 1 under each filter.
- **Self dogfood (cfdb on cfdb):** `cfdb scope --context cfdb-extract` default vs `cfdb scope --context cfdb-extract --production-only` must produce different `unwired` counts (default count ≤ production-only count). The inequality direction is load-bearing: test entry points are seeds in default mode and reduce the unwired set; production-only mode excludes them. If cfdb's own `crates/cfdb-petgraph/tests/` or `crates/cfdb-hir-extractor/tests/` files are present in the keyspace, the diff must be ≥ 1.
- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA):** `cfdb scope --production-only` on graph-specs-rust keyspace must produce non-error output (exit 0); the new `reachable_from_production_entry` attribute must not conflict with any existing query. Confirm by running all four `.cfdb/queries/*.cypher` — zero row change expected (they do not reference the new attribute name).
- **Target dogfood (qbot-core at pinned SHA):** report before-vs-after `cfdb scope --context trading` unwired counts: default mode expected to drop from ~2057 by ≥30% (per RFC §7 slice 042-B prescription); `--production-only` mode expected to remain near 2057. Attach both JSON summaries to the PR body.

### Slice 042-C — empirical close-out on qbot-core

- **Tests:** none — rationale: cross-repo empirical report, not code. (Per RFC §7.)

---

## D3. Dual-dogfood discipline notes

**Self dogfood lower-bound fragility.** The D2 prescription for 042-A uses `n >= 1` as the self-dogfood lower bound for `:EntryPoint{kind=test}` count. This is intentionally conservative because the exact count will change as cfdb's own test suite evolves. A hard lower bound (e.g. `n >= 50`) would become brittle within a few issues. The prescription names the concrete evidence source (`crates/cfdb-petgraph/src/enrich/reachability/tests.rs`) so the implementer can derive a tighter floor from a grep count at implementation time. This is preferable to encoding a magic number in the test prescription.

**Cross dogfood attribute-name collision check (BRIEF §6 mandatory concern).** The BRIEF §6 convener note flags a specific risk: `enrich_reachability` will write two new `:Item` attributes (`reachable_from_production_entry`, `reachable_production_entry_count`) onto every keyspace, including the graph-specs-rust one. I have verified the four existing graph-specs-rust query files:
- `arch-ban-unwrap-domain-ports.cypher` — matches on `cs.callee_path`, `caller.crate`. No `:Item.reachable_*` attribute references.
- `arch-context-no-application-in-domain.cypher` — matches on `cs.callee_path`, `caller.crate`. No conflict.
- `arch-context-no-cross-layer-unwrap.cypher` — matches on `cs.callee_path`, `caller.crate`. No conflict.
- `arch-context-no-syn-in-domain.cypher` — matches on `cs.callee_path`, `caller.crate`. No conflict.

None of the four rules read `reachable_from_entry`, `reachable_from_production_entry`, or `reachable_entry_count`. The new attributes are additive and invisible to all existing queries. Cross dogfood is a no-op regression check as the RFC §4 invariant requires.

**Port signature change and cross-dogfood timing.** If Finding 1 (add `EntryPointKindFilter` to `cfdb-core`) is accepted, the `EnrichBackend` trait gains a parameter. This does not change the schema wire format (the attributes written to the keyspace are unchanged) and does not require a `SchemaVersion` bump. The cross-fixture pin does not need updating. However, if graph-specs-rust has its own `enrich_reachability` call site (it currently appears not to — it uses cfdb as a CLI tool, not as a library), the timing would need care. Verified: graph-specs-rust `Cargo.toml` does not list `cfdb-core` or `cfdb-petgraph` as dependencies; it consumes cfdb as a CLI binary via `.cfdb/cfdb.rev`. No library ABI impact on graph-specs-rust.

---

## D4. Graph-specs-rust update against real code

**Proposed Cypher (one):**

```cypher
// arch-domain-only-reached-from-tests.cypher
//
// Rule: no item in the `domain` crate is reached exclusively from test
// entry points (reachable_from_production_entry = false AND
// reachable_from_entry = true AND item.is_test = false).
//
// Rationale (clean-arch layer-purity): a domain fn reachable only from
// tests but not from any production entry point (cli_command, mcp_tool,
// http_route, cron_job, websocket) is either (a) dead production code
// that survives only because tests call it directly — a misplaced-layer
// marker (the fn belongs in a test helper, not in the domain), or (b)
// a feature gate that the production wiring forgot to connect. Both are
// violations of the clean-arch principle that domain code should be
// exercised by production flows; if the only exerciser is a test, the
// domain boundary is leaking test-internal logic.
//
// Expected: zero rows on a clean tree. Any row is a candidate for
// either promotion to a test-fixture crate or wiring into a production
// entry point.

MATCH (i:Item)
WHERE i.crate =~ 'domain.*'
  AND i.kind IN ['fn', 'method']
  AND i.is_test = false
  AND i.reachable_from_entry = true
  AND i.reachable_from_production_entry = false
RETURN i.qname AS qname,
       i.crate AS crate,
       i.file AS file,
       i.line AS line
ORDER BY qname ASC
```

**Filed at (or proposed for):** `.cfdb/queries/arch-domain-only-reached-from-tests.cypher` on `yg/graph-specs-rust`.

**Citation against current graph-specs-rust pinned SHA (913f06f):** The rule reads `i.reachable_from_production_entry`, an attribute that does not yet exist in any keyspace (it is written by RFC-042 slice 042-B). Until 042-B lands and a re-extract of graph-specs-rust is performed, the query returns zero rows because the attribute is absent (the evaluator treats absent-prop comparisons as false-matching). This means the rule can be shipped alongside the RFC-042 feature PRs as a preventative policy rule — it starts silent and becomes active on the first re-extract after 042-B lands.

Concrete graph-specs-rust file at current HEAD: the `domain/src/diff.rs:23` `pub fn diff(spec: CheckInput, code: Graph) -> Vec<Violation>` function is an example of a domain fn that SHOULD be reachable from production entry points (the CLI uses it). After RFC-042 lands, if `diff` were reachable only from tests, this rule would fire on it. At current HEAD, the rule is expected to produce zero rows on first run (attribute absent) and zero rows on re-extract (because `diff` is wired via the production CLI path). The rule is preventative.

**Intent:** zero-violation policy from day one. The rule ships clean against graph-specs-rust: the domain is small and well-wired. Any future domain fn that tests reach but production does not will surface immediately, preventing the JupiterCryptoBroker failure mode (RFC §1 canonical example) from recurring in graph-specs-rust itself.

**Rationale:** Clean architecture requires that the domain is exercised end-to-end through production entry points. A domain fn reachable only from tests signals a layer-purity regression: either the fn is test scaffolding masquerading as domain logic, or the production wiring was forgotten. RFC-042 makes this class of finding detectable for the first time. Shipping the rule at day one on graph-specs-rust is the correct clean-arch enforcement posture — zero-tolerance, not cleanup-driving, because graph-specs-rust's domain is small enough to be clean on the first extraction.
