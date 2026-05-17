# RFC-042 verdict — ddd-specialist

**Verdict:** REQUEST CHANGES
**Author:** ddd-specialist sub-agent
**Date:** 2026-05-17

---

## D1. Verdict on the RFC as written

### Finding 1 — Homonym gap: `is_test` (bool attribute) vs `kind="test"` (string discriminator)

The RFC does not distinguish two overlapping but semantically distinct concepts that share the root token `test` in the cfdb vocabulary:

- **`:Item.is_test: bool`** — declared at `crates/cfdb-core/src/schema/describe/nodes.rs:109` and produced by `cfdb-extractor::item_visitor::emit::fn_is_test` (`crates/cfdb-extractor/src/item_visitor/emit/mod.rs:183-189`). Semantics: "this item was compiled under a `#[cfg(test)]` scope OR carries a bare `#[test]` attribute." It is a structural fact about where in the source tree the item lives, not about its role as an invocation surface.
- **`:EntryPoint{kind="test"}`** — proposed by RFC-042 §3.1. Semantics: "a function that Rust's test runner (or cucumber-rs) is able to invoke as the entry into a test binary." It is a *behavioral* classification — the fn is a caller-graph root from the test harness.

These are different concepts in different bounded contexts:

- `:Item.is_test` belongs to the **syn-extraction context**: it answers "what cfg scope encloses this source item?" Consumers: `classifier-unwired.cypher:14` filters `is_test = false` to exclude test-only items from the unwired classifier. The `crates/cfdb-cli/tests/pattern_c_canonical_bypass.rs:22` and `arch_ban_utc_now.rs:83` confirm that `is_test` is used as a **scope exclusion predicate** in rules.
- `:EntryPoint{kind="test"}` belongs to the **reachability-graph context**: it answers "what fns seed the BFS from the test-invocation surface?" Consumers: `enrich_reachability.rs` (all `:EntryPoint` nodes regardless of kind seed the all-kinds BFS).

The homonym risk is **query collision**: a query filtering on `ep.kind = "test"` and a query filtering on `i.is_test = true` answer different questions. If a future analyst writes `MATCH (e:EntryPoint{kind:"test"})-[:EXPOSES]->(i:Item) WHERE i.is_test = false RETURN i` they will find production-declared items that are only exercised by tests — a valid and interesting query — but the mental model requires understanding that `kind="test"` and `is_test=true` are orthogonal axes.

**This is not a blocking defect on its own**, but the RFC §3.2 schema-doc text does not acknowledge it. The schema descriptor for `kind` at `nodes.rs:296` should add one sentence: "Note: `kind=\"test\"` on `:EntryPoint` is orthogonal to `:Item.is_test` — the former classifies the *entry surface*, the latter classifies the *item's compile scope*."

**This is the change requested under Finding 1.** It is a descriptor-only edit; no code changes.

### Finding 2 — Category-axis defensibility: runtime-exposed vs build-time-invoked

The RFC §6 rejected-alternative ("`:Test`/`:Bench` separate labels") is correctly decided from the reachability BFS perspective: tests are entry points whose seeds drive the call graph, exactly as `mcp_tool` or `cli_command` do. The `kind` discriminator is the correct axis.

The DDD question is whether `{cli_command, mcp_tool, http_route, cron_job, websocket}` and `{test, bench}` represent one ubiquitous-language concept ("entry point") or two. The answer from the domain's perspective: in cfdb's model, an `:EntryPoint` is any caller-graph root — a node from which the BFS seeds. Tests and benches are exactly that. The distinction "runtime-exposed vs build-time-invoked" is an important *property* of an entry point (which `--production-only` exposes) but not a reason to split the label. The RFC's vocabulary is correct.

Vocabulary fit assessment: PASS. The ubiquitous language of cfdb's consuming operators is "what code is reachable from what kind of caller?" — and `kind` on `:EntryPoint` is precisely the discriminator for that question. `test` and `bench` are valid values in that vocabulary.

### Finding 3 — BDD step classification: `kind="test"` is correct

RFC §3.1 classifies `#[given]`/`#[when]`/`#[then]` as `kind="test"`. The RFC §3.4 calls this "load-bearing" without a DDD argument. The DDD argument: BDD step-definition functions are not a distinct domain concept from tests in cfdb's context. From the reachability graph's perspective, a step-def is a leaf that cucumber-rs calls — identical structurally to what `#[test]` functions are to libtest. There is no downstream consumer that needs to distinguish "item reachable only from a step-def" from "item reachable only from a unit test." A `kind="bdd"` or `kind="step"` would add a discriminator that no existing query or operator workflow would use.

Classification ACCEPTED. No change requested.

### Finding 4 — SchemaVersion stability claim: VALID with one caveat

RFC §4 argues no SchemaVersion bump because `kind` is an open-set string. This is structurally correct as evidenced by `cfdb-petgraph/src/graph.rs` (no exhaustive `match ep.kind { … }`). The DDD lens adds one verification: the `enrich_reachability` dual-BFS in §3.3 writes `reachable_from_production_entry` and `reachable_production_entry_count` — two new `:Item` attributes not previously defined. These ARE new facts on existing nodes. The BRIEF §7 anti-bias note correctly identifies this as the live risk: "lenses MUST think through whether ANY query in `yg/graph-specs-rust/.cfdb/queries/*.cypher` reads these attribute names."

Verified: all four `yg/graph-specs-rust` queries (at HEAD SHA `5a7cb03` — note: local clone HEAD, the pinned companion SHA in `.cfdb/cross-fixture.toml` is `913f06f`) match only on `caller.is_test`, `cs.is_test`, `caller.crate`, and `cs.callee_path`. None reads `reachable_from_entry` or `reachable_from_production_entry`. The cross-dogfood regression risk is **zero** for the new `:Item` attributes on existing queries.

However, the new `:Item.reachable_from_production_entry` attribute is **not described in the schema descriptor**. It is produced by the RFC-042 Option (A) dual-BFS but `nodes.rs` has no `attr("reachable_from_production_entry", ...)` entry for `:Item`. The existing `reachable_from_entry` and `reachable_entry_count` may also be undescribed (they are produced by the current `enrich_reachability` but not audited here). If the new attribute is undescribed, `cfdb describe` output will be incomplete and future query authors will not discover it.

**This is the change requested under Finding 4.** The RFC §3.2 scope (descriptor edits) should be extended to add `reachable_from_production_entry: bool` and `reachable_production_entry_count: i64` to the `:Item` node descriptor in `nodes.rs` (under `Provenance::EnrichReachability` or whichever provenance tag reachability uses). This is a descriptor-only edit.

### Summary of change requests

1. **RFC §3.2 — descriptor text for `:EntryPoint.kind`**: add one sentence disambiguating `kind="test"` from `:Item.is_test`.
2. **RFC §3.2 scope extension — `:Item` descriptor**: add `reachable_from_production_entry: bool` and `reachable_production_entry_count: i64` attribute entries produced by the dual-BFS.

Neither request requires code changes to §3.1 or §3.3. Both are descriptor-only. The RFC remains correct in its core design choices. The verdict is REQUEST CHANGES, not REJECT. If the RFC author adds both descriptor extensions (a one-paragraph addition to §3.2), the DDD lens would RATIFY on resubmit.

---

## D2. Tests prescription

### Slice 042-A — extractor `:EntryPoint{kind=test|bench}` emission + fixture

- **Unit:** pure `has_test_attr(&ast::Fn) -> bool` assertions on synthetic `ast::Fn` inputs covering each attribute variant (`#[test]`, `#[tokio::test]`, `#[async_std::test]`, `#[given]`, `#[when]`, `#[then]`, `#[bench]`, plus negative `#[other]` and `#[cfg(test)]` — the last must NOT trigger `has_test_attr` because `cfg` is not in the attribute path as RFC §3.1 documents). `has_bench_attr` analogously. DDD concern: the `cfg(test)` negative case is the split-brain boundary — if `has_test_attr` fires on `#[cfg(test)]`, items inside test modules would emit a spurious `:EntryPoint` node that duplicates the `:Item.is_test=true` signal.
- **Self dogfood (cfdb on cfdb):** `MATCH (e:EntryPoint{kind:"test"}) RETURN count(e) AS cnt` against cfdb's own keyspace after `cfdb extract --features hir`. The lower bound N must be derived at PR time from `grep -rn '#\[test\]' crates/*/src/ crates/*/tests/ | wc -l` (attribute-based) plus `find crates/*/tests/ -name '*.rs' | xargs grep -l '^fn ' | wc -l` (file-location-based helpers). N must be ≥ the grep count — if it is lower, the attribute probe is under-counting.
- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA):** zero regression on all four existing `.cfdb/queries/*.cypher`. Verified expectation: none of those queries contains `kind` on `:EntryPoint` or `reachable_from_production_entry` on `:Item`, so the new emissions cannot produce new rows on existing rules. The cross-dogfood run is a no-op regression check per RFC §4 SchemaVersion stability invariant.
- **Target dogfood (qbot-core at pinned SHA):** report total `:EntryPoint{kind:"test"}` and `:EntryPoint{kind:"bench"}` counts in PR body. DDD note: the reviewer should also spot-check that `JupiterCryptoBroker::new` (RFC §1 canonical example) is now classified as `reached_from_entry=true` with at least one reaching `:EntryPoint{kind:"test"}` — this validates the vocabulary unification at the level of the concrete domain object named in the problem statement.

### Slice 042-B — scope `--production-only` flag + dual-BFS + classifier rule

- **Unit:** `enrich_reachability::collect_seeds` returns the expected subset when `entry_kind_filter = Some({cli_command, mcp_tool, http_route, cron_job, websocket})` — a graph with one `kind="test"` entry point and one `kind="mcp_tool"` entry point must produce exactly one seed under the production filter, two under `None`. `accumulate_reach_counts` invariant: items only reachable from the test entry point have `reachable_from_production_entry = false` and `reachable_from_entry = true`.
- **Self dogfood (cfdb on cfdb):** run `cfdb scope --context cfdb-extract` in both default and `--production-only` modes; assert default `unwired` count < `--production-only` unwired count by at least 1. cfdb has integration tests under `crates/cfdb-*/tests/` so RFC-042's test entry points will reach items that the production-only BFS does not. If the two counts are equal on cfdb's own keyspace, something in the dual-BFS wiring is wrong.
- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA):** zero regression. The `--production-only` flag is opt-in; existing graph-specs-rust queries do not invoke `cfdb scope`. The new `reachable_from_production_entry` attribute will appear on graph-specs-rust `:Item` nodes after enrichment, but no existing companion query reads it.
- **Target dogfood (qbot-core at pinned SHA):** before-vs-after `cfdb scope --context trading` unwired count in both modes. Per RFC §7: expected ≥30% drop in default mode from 2057. DDD-specific concern: the PR body must include a spot audit of at least 5 items reclassified from "unwired" to "reached-from-test" — the operator needs confidence that the reclassified items are genuinely test-exercised (not file-location false-positives from `tests/` helper modules that don't actually exercise the reclassified item).

### Slice 042-C — empirical close-out on qbot-core

- **Tests:** none — rationale: cross-repo empirical report, not code. Per RFC §7.

---

## D3. Dual-dogfood discipline notes

**Self dogfood lower-bound fragility (Slice 042-A):** The RFC §7 self-dogfood prescription says `MATCH (e:EntryPoint{kind:"test"}) RETURN count(e)` ≥ N, where N is a grep lower bound. The fragility: cfdb's own integration tests under `crates/cfdb-cli/tests/` use `#[test]` fns, but those integration tests are in files *outside* `crates/*/src/` — they are in `crates/*/tests/`. The file-location detector (§3.1 "is_under_tests_dir") will emit `:EntryPoint` for every `fn` in those files, including helper fns that are not themselves `#[test]`-attributed. The grep count of `#[test]` lines will be LOWER than the actual emitted count because file-location detection captures test helpers too. The lower bound is sound (actual ≥ grep), but the assertion should be stated as "emitted count ≥ grep count of `#[test]` occurrences" rather than "emitted count ≈ grep count." The RFC §7 wording should be tightened to avoid this ambiguity.

**Cross dogfood attribute collision (all slices):** Verified that none of the four `yg/graph-specs-rust/.cfdb/queries/*.cypher` files (`arch-ban-unwrap-domain-ports.cypher`, `arch-context-no-application-in-domain.cypher`, `arch-context-no-cross-layer-unwrap.cypher`, `arch-context-no-syn-in-domain.cypher`) reads `reachable_from_entry`, `reachable_from_production_entry`, or any `:EntryPoint` attribute. The new attributes written by the dual-BFS do not trigger new rows on the companion's existing rules. Cross-dogfood is safe.

**`reachable_from_production_entry` schema descriptor gap:** The dual-BFS in Option (A) writes a new `:Item` attribute (`reachable_from_production_entry: bool`) that is not currently defined in the `:Item` node descriptor at `crates/cfdb-core/src/schema/describe/nodes.rs`. This is the same category of gap that motivated RFC-037 (schema-producer alignment). The descriptor must be extended before the slice lands or `cfdb describe` becomes incomplete. This is the basis of Finding 4 in D1.

---

## D4. Graph-specs-rust update against real code

**Proposed Cypher (one):**

```cypher
// vocab-domain-reachable-only-from-tests.cypher
//
// Rule: domain items reachable only from test entry points (not from any
// production entry point) are vocabulary candidates that exist outside the
// production call graph — potential signs of an anaemic aggregate or a
// concept that lives in the domain model but is never exercised by the
// system's actual production surface.
//
// DDD rationale: an aggregate-root method (or domain service fn) that
// is only reachable from test drivers is either:
//   (a) a method the aggregate has grown but the production wiring never
//       exercises — anaemic aggregate smell; the method models a concept
//       that exists in the domain vocabulary but has no live use.
//   (b) a genuinely useful method that production wiring simply hasn't
//       adopted yet — the test is ahead of production wiring (valid,
//       but informative for an inventory-driven operator).
//
// This rule fires on items with reachable_from_entry = true (they ARE
// reached by something) but reachable_from_production_entry = false
// (that something is NOT a production entry point). It is therefore
// distinct from the existing "unwired" classifier (reachable_from_entry
// = false) — it finds the middle class: test-only-wired.
//
// Expected: cleanup-driving (finds existing findings). Zero-violation
// policy would be too strict — test-only wiring is not inherently wrong.
// The rule produces an inventory for the operator to review.

MATCH (i:Item)
WHERE i.kind IN ['fn', 'method']
  AND i.reachable_from_entry = true
  AND i.reachable_from_production_entry = false
  AND i.is_test = false
  AND i.bounded_context = $context
RETURN i.qname AS qname,
       i.name AS name,
       i.kind AS kind,
       i.crate AS crate,
       i.bounded_context AS bounded_context,
       i.file AS file
ORDER BY qname ASC
```

**Filed at (or proposed for):** `.cfdb/queries/vocab-domain-reachable-only-from-tests.cypher` on `yg/graph-specs-rust`.

**Citation against current graph-specs-rust pinned SHA (`913f06f` per `.cfdb/cross-fixture.toml`):** The graph-specs-rust domain crate exposes three public functions: `domain::tokens::tokenise_target` (`domain/src/tokens.rs:25`), `domain::context::detect_import_cycle` (`domain/src/context.rs:197`), `domain::diff::diff` (`domain/src/diff.rs:23`). The ports crate exposes two traits: `Reader` and `ContextReader` (`ports/src/lib.rs:15,29`). Whether any of these are reachable only from test entry points cannot be determined without a live cfdb extract of graph-specs-rust post-RFC-042 landing. The rule is proposed as **cleanup-driving** (finds existing findings) — zero-violation policy is intentionally not required on day one, since the intent is to surface the inventory for review, not to gate on it.

**Intent:** cleanup-driving (finds existing findings that may warrant operator review).

**Rationale:** The DDD lens concern is anaemic aggregates and vocabulary leakage. An item that the domain model exposes (`pub fn`) but that only test drivers call is a vocabulary fact: the concept exists (the fn is named and typed) but the production system never invokes it. In cfdb's own vocabulary terms, this is an item with `reachable_from_entry=true, reachable_from_production_entry=false, is_test=false` — a fact class that only becomes expressible after RFC-042 lands the dual-BFS. The proposed rule makes this fact class a first-class query in graph-specs-rust's ban suite. It is intentionally non-blocking (cleanup-driving) because test-ahead-of-production wiring is a legitimate development pattern, not an invariant violation. The rule produces an operator inventory, not a gate failure.
