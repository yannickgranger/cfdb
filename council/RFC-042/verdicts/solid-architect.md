# RFC-042 verdict — solid-architect

**Verdict:** REQUEST CHANGES
**Author:** solid-architect sub-agent
**Date:** 2026-05-17

---

## D1. Verdict on the RFC as written

### SRP at `registers_param.rs` — CONCERN, not a blocker

The RFC §3.1 "Probe semantics" paragraph directs `has_test_attr` and `has_bench_attr` to live
in `entry_point_emitter/registers_param.rs` alongside `has_tool_attr` (`registers_param.rs:56-71`).
The file's current responsibility, stated at its own module-doc line 1-7, is:

> "REGISTERS_PARAM edge emitters for clap / MCP :EntryPoints, plus the attribute probes the
> parent's `scan_file` dispatcher uses to branch on `#[derive(Parser|Subcommand)]` vs `#[tool]`."

The existing probes — `has_clap_derive`, `has_tool_attr`, `field_has_arg_attr` — all serve the
REGISTERS_PARAM emission responsibility: they detect the structural feature that gates param-edge
emission. `has_test_attr` and `has_bench_attr` do NOT gate REGISTERS_PARAM edges for test/bench
entry points (the RFC explicitly says "no REGISTERS_PARAM" for test/bench kinds — RFC §3.1 new
shape, items 2-5). They are pure classification probes with no param-edge emission counterpart.

Placing attribute probes whose only caller is the dispatch branch, and whose domain is
"test/bench classification", into a file whose stated cohesion is "param registration + detection
for param-emitting entry points" is a CCP violation (these probes change for a DIFFERENT reason
than `emit_mcp_registers_param` changes). The file has 175 LOC today; adding two more probes is
not a LOC budget concern, but the cohesion concern IS real.

**CHANGE REQUEST 1 (load-bearing for SRP/CCP):** Introduce a sibling file
`entry_point_emitter/test_bench.rs` containing `has_test_attr` and `has_bench_attr`, with its own
module-doc scoped to "test and bench attribute classification". The existing `registers_param.rs`
stays param-emission focused. This is a 10-LOC mechanical split, not a design revision.

---

### OCP / ISP at the FN dispatch chain — ACCEPTABLE as specified

The `scan_file` FN branch currently has one `if has_tool_attr` arm (`entry_point_emitter.rs:175`).
The RFC adds four more branches: `has_test_attr`, `has_bench_attr`, `is_under_tests_dir`,
`is_under_benches_dir`. This is the classic Open/Closed concern — the dispatch is open for
modification rather than extension.

However, the RFC §3.1 item 4 correctly observes: "benches and test attrs are mutually exclusive
in source (rustc lints reject a fn with both)." All five branches are also mutually exclusive at
the dispatch level, which is the precise condition under which a flat `if/else if` cascade IS the
correct idiom — a strategy or dispatch-table pattern would add indirection for no benefit. The
cascade correctly encodes a closed set of disjoint predicates on `ast::Fn` attributes.

At five branches the cascade remains within the cognitive-complexity budget (no nested early
returns, each branch is a one-line `emit` call). The RFC's statement that this shape is
"consistent with the existing `scan_file` body" is verified: the `STRUCT`/`ENUM`/`FN`/
`CALL_EXPR`/`METHOD_CALL_EXPR` outer `match` already contains multi-branch internal logic in the
`METHOD_CALL_EXPR` arm (two calls: `try_emit_websocket` + `classify_http_route_method_call`).

**Verdict on this concern: ACCEPTABLE as specified.** No change request.

---

### LSP / ISP on `enrich_reachability` parameter change — REQUEST CHANGES

The RFC §3.3 proposes adding `entry_kind_filter: Option<&BTreeSet<&str>>` to `enrich_reachability`
(in practice to `collect_seeds`, which would receive the filter). Current signature:

```rust
fn collect_seeds(state: &KeyspaceState, entry_points: &[NodeIndex]) -> BTreeSet<NodeIndex>
```

There are four existing call sites for `store.enrich_reachability()` (the `EnrichBackend` trait
method), all through `cfdb-petgraph/src/enrich_backend.rs:151-163` which calls
`crate::enrich::reachability::run(state)` — a single callsite with no filter parameter. The
public surface is the `EnrichBackend` trait method at `cfdb-core/src/enrich.rs:177`.

**Sub-concern A — trait method signature change is a BREAKING API change.**
`EnrichBackend::enrich_reachability` (`cfdb-core/src/enrich.rs:177`) is the public interface.
Adding `entry_kind_filter` to this method changes the trait signature, requiring updates to every
implementor, including the default stub. The RFC does NOT discuss this — it only describes the
`enrich_reachability` standalone function in `cfdb-petgraph`. If the filter is threaded through
the trait, this is not "transparent to callers" as implied by option (A)'s framing.

**Sub-concern B — `&BTreeSet<&str>` is the wrong interface for ISP.**
`BTreeSet<&str>` leaks the data shape (ordered set of string slices) into the caller contract.
`impl Fn(&str) -> bool` (as the BRIEF §1 suggests) would decouple the caller from the set choice
and allow the caller to encode more complex predicates without changing the function signature.
However, in the absence of such use cases today, `BTreeSet<&str>` is acceptable if scoped
correctly. The concrete risk is that the orchestrator (cfdb-cli) constructs a `BTreeSet<&'static str>`,
but the lifetime annotation `&BTreeSet<&str>` requires careful lifetime propagation — this is
a rust-systems concern and is flagged here for completeness.

**CHANGE REQUEST 2 (load-bearing for LSP/ISP):** The RFC MUST explicitly specify whether
`EnrichBackend::enrich_reachability` in `cfdb-core/src/enrich.rs` is modified, or whether the
filter is a module-private detail of `cfdb-petgraph::enrich::reachability::run`. Option (A) as
written implies an orchestrator invokes `run` twice — but who IS the orchestrator? If it is
`PetgraphStore::enrich_reachability` (the trait implementor in `cfdb-petgraph/src/enrich_backend.rs:151`),
then the `EnrichBackend` trait method signature can remain as `(&mut self, keyspace: &Keyspace)`
and the dual-BFS is encapsulated behind that call. If the orchestrator is cfdb-cli, the public
trait signature MUST change. The RFC is ambiguous on this; the implementation will diverge
depending on where the orchestrator layer is placed.

**Preferred resolution:** the dual-BFS orchestration lives inside `PetgraphStore::enrich_reachability`
(the trait implementor). The `EnrichBackend` trait method does NOT change. The implementor calls
`reachability::run(state, None)` and `reachability::run(state, Some(&production_kinds))` in
sequence, writing both attribute pairs. This preserves the stable `EnrichBackend` abstraction
(cfdb-core stays unchanged), keeps cfdb-petgraph as the sole orchestrator of BFS internals, and
leaves the cfdb-cli call path unchanged (one `store.enrich_reachability(&ks)` call, not two).

---

### Stable abstractions for `cfdb-core` — ACCEPTABLE

RFC §3.2 proposes editing the descriptor text at `cfdb-core/src/schema/describe/nodes.rs:296`.
Current text: `"Entry-point kind: \`mcp_tool\`, \`cli_command\`, \`http_route\`, or \`cron_job\`…"`.
The RFC adds `test` and `bench` to the documented enum list.

The BRIEF §7 (convener anti-bias note) asks lenses to check whether any query in
`yg/graph-specs-rust/.cfdb/queries/*.cypher` reads these attribute names or matches on kind
strings. Result of inspection: **none of the four graph-specs-rust queries** reads `:EntryPoint`
or `reachable_from_entry`; they all match on `CallSite.callee_path` and `Item.crate` patterns.
There is NO consumer that would break from the string-value change.

The descriptor text at `nodes.rs:296` is human-readable documentation emitted by `cfdb describe`
— it is NOT parsed by any evaluator. No `match ep.kind { … }` exhaustive arm exists in the
codebase (verified: the only references to the kind string constant in cfdb-petgraph are
attribute-write paths in `entry_point_emitter.rs:349-360`, not match-exhaustive reads). The RFC's
claim that this is "additive-doc, not a wire-contract break" is correct.

**Verdict: ACCEPTABLE. No change request.**

---

### Component-level CRP/CCP — ACCEPTABLE, with one observation

Slice 042-A touches cfdb-hir-extractor + cfdb-core (descriptor text). Slice 042-B touches
cfdb-petgraph + cfdb-cli + embedded Cypher. This is the correct component boundary:

- cfdb-hir-extractor changes for ONE reason: new attribute probes (changes when the attribute
  vocabulary changes).
- cfdb-petgraph changes for ONE reason: dual-BFS logic (changes when the BFS algorithm changes).
- cfdb-cli changes for ONE reason: flag plumbing (changes when the CLI contract changes).
- cfdb-core descriptor is documentation-only: no logic change, acceptable additive edit.

The ADP check: cfdb-cli → cfdb-petgraph → cfdb-core ← cfdb-hir-extractor. No cycles. Arrows point
from unstable (cfdb-cli, Ce high) toward stable (cfdb-core, Ca high). The SDP is satisfied.

**One CRP observation (non-blocking):** The new `classifier-unwired-production.cypher` is a
near-duplicate of `classifier-unwired.cypher` (the RFC §3.3 says it reads a different attr name).
Two files with ≥90% identical content that differ only in one attribute name reference is a
CRP violation candidate: reusers will want one but not both, and future changes to the WHERE
clause must be made in two places. This is NOT a blocker because the RFC correctly justifies the
duplication as "uniform grammar" and the alternative (query-time string substitution) has its own
complexity. But the implementer SHOULD add a comment in both files naming the sibling and the
single point of divergence, so future editors know to maintain both.

---

### Summary of change requests

| # | Severity | Target | Description |
|---|---|---|---|
| CR1 | REQUIRED | `entry_point_emitter/registers_param.rs` | Extract `has_test_attr` / `has_bench_attr` into a new `test_bench.rs` sibling |
| CR2 | REQUIRED | RFC §3.3 | Clarify whether `EnrichBackend::enrich_reachability` (cfdb-core) signature changes; specify that orchestration lives in `PetgraphStore` implementor, not in cfdb-cli |

If both CR1 and CR2 are resolved in the RFC text (§3.1 "probe semantics" paragraph + §3.3
"implementation choice" paragraph), verdict changes to RATIFY.

---

## D2. Tests prescription

### Slice 042-A — extractor `:EntryPoint{kind=test|bench}` + fixture

- **Unit:** Pure `has_test_attr(&ast::Fn) -> bool` and `has_bench_attr(&ast::Fn) -> bool`
  assertions on synthetic `ast::Fn` inputs. Cover all six attribute variants: `#[test]`,
  `#[tokio::test]`, `#[given]`, `#[when]`, `#[then]` (all → true for `has_test_attr`),
  `#[bench]` (→ true for `has_bench_attr`), `#[other]` (→ false for both). Negative cases:
  `#[cfg(test)]` must NOT match — the `cfg` token appears in the attr path, not as the last
  segment; verify this explicitly because `cfg` contains the substring `test`. Also assert
  `has_test_attr` on a `#[tool]` fn returns false (precedence non-interference). Place in
  `crates/cfdb-hir-extractor/tests/entry_point_attr_probes.rs` — pure-function unit tests
  do NOT need the HIR database; they can operate on `ast::Fn` nodes parsed from synthetic
  source strings using `ra_ap_syntax::SourceFile::parse`.

- **Self dogfood (cfdb on cfdb):**
  ```
  cfdb extract --workspace . --db .cfdb/db --keyspace cfdb --features hir
  MATCH (e:EntryPoint) WHERE e.kind = "test" RETURN count(e) AS n
  ```
  Assert `n >= N` where N is the grep lower-bound count at the time of implementation:
  `grep -rn '#\[test\]\|#\[tokio::test\]\|#\[given\]\|#\[when\]\|#\[then\]' crates/ | wc -l`.
  This is a smoke test that the extractor actually fires; the exact count can vary by a small
  margin as the codebase evolves. Do NOT hard-code N — compute it at CI time with the same grep
  and assert `n >= grep_count - 5` (5-item tolerance for helper fns the grep catches but the
  extractor skips via the `#[cfg(test)]`-module inline case).

- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA `2aedd013`):**
  Zero regression on all four existing `.cfdb/queries/*.cypher` rules. None of the four queries
  references `:EntryPoint` or `reachable_from_entry` (verified by inspection). The new `kind`
  values do not appear in any WHERE clause. Expected: `cfdb violations` exits 0 for all four
  rules against the pinned SHA. No new graph-specs-rust PR needed (SchemaVersion not bumped).

- **Target dogfood (qbot-core at pinned SHA):**
  Run `cfdb extract --workspace qbot-core --features hir` then report in the PR body:
  - Total `:EntryPoint{kind=test}` count
  - Total `:EntryPoint{kind=bench}` count
  - Representative sample: first 10 emitted test entry-point qnames

### Slice 042-B — scope `--production-only` + dual-BFS + classifier rule

- **Unit:** (defer to rust-systems for BFS parameter correctness). For SRP purposes, prescribe:
  test that `collect_seeds` with `kind_filter = Some({"cli_command"})` returns only seeds whose
  `:EntryPoint.kind` is `cli_command`, verified on a minimal synthetic `KeyspaceState` with one
  `test` and one `cli_command` entry point. Also assert that `collect_seeds` with `kind_filter = None`
  returns seeds from all kinds (unchanged behavior). This isolates the filter predicate as a pure
  function without an HTTP/DB/filesystem dependency.

- **Self dogfood (cfdb on cfdb):**
  ```
  cfdb scope --context cfdb-extract
  cfdb scope --context cfdb-extract --production-only
  ```
  Assert that the `unwired` count differs between the two outputs by at least 1. cfdb has its own
  integration tests under `crates/cfdb-*/tests/` — those test fns create `kind=test` entry points
  whose reachability contribution is present in default mode and excluded in `--production-only` mode.
  The diff count is the concrete proof the dual-BFS is wired correctly.

- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA):**
  Zero regression — the `--production-only` flag is opt-in and has no effect on the existing
  four queries, which do not read `reachable_from_production_entry`. Confirm `cfdb violations`
  exits 0 on all four rules; confirm `cfdb scope` (default mode) on graph-specs-rust produces the
  same `unwired` count as before this slice (determinism of the all-kinds BFS is unchanged).

- **Target dogfood (qbot-core at pinned SHA):**
  Report in the PR body:
  - `cfdb scope --context trading` `unwired` count (default mode) — expected drop of ≥30% from
    the 2057 baseline documented in RFC §1.
  - `cfdb scope --context trading --production-only` `unwired` count — expected to remain near
    2057 (the new flag recovers the historical view).

### Slice 042-C — empirical close-out on qbot-core

- **Tests:** none — rationale: cross-repo empirical report, not code. Per RFC §7.

---

## D3. Dual-dogfood discipline notes

**Determinism of `reachable_from_production_entry` (G1 invariant concern).**
The RFC §4 states determinism is inherited from the existing sort discipline
(`entry_point_emitter.rs:105-112`). This is CORRECT for entry-point node ordering, but the
`write_item_attrs` function in `reachability.rs:195-209` iterates `state.nodes_with_label` which
currently returns a `Vec<NodeIndex>` sorted by index. The second BFS invocation (production-only
filter) writes `reachable_from_production_entry` and `reachable_production_entry_count` on the
SAME items. The write order must be identical across two sequential runs.

The concern: if `PetgraphStore::enrich_reachability` is invoked twice (the dual-BFS orchestration
per CR2's preferred resolution), the second call modifies the same `KeyspaceState` that the first
call already wrote to. The `BTreeMap<NodeIndex, i64>` accumulation in `accumulate_reach_counts`
is pure (builds a fresh map each time), so there is no state pollution between the two runs.
`write_item_attrs` uses `node_weight_mut` keyed by `NodeIndex` — stable across both calls.
The G1 invariant is preserved as long as `nodes_with_label` iteration order is deterministic
(currently guaranteed by the `Vec<NodeIndex>` sorted-index contract in cfdb-petgraph).

**However:** the RFC does NOT specify which `EnrichReport` is returned when `enrich_reachability`
runs twice. The `EnrichReport::attrs_written` counter would be incorrect if it only counts the
first pass's writes. The RFC SHOULD specify: the returned `EnrichReport` reports the SUM of both
passes' `attrs_written` (2 passes × 2 attrs × N items = 4N writes total, vs 2N previously).
Implementers who return only one pass's report will produce misleading `cfdb enrich-reachability`
output. Add this to the invariants section (§4).

**Self dogfood lower-bound fragility note:**
The self-dogfood `MATCH (e:EntryPoint) WHERE e.kind = "test" RETURN count(e)` lower bound based
on `grep` counts is fragile because cfdb's own `#[cfg(test)]` inline modules use `#[test]`
attributes. The extractor WILL detect these (the §3.4 fixture's last row proves it). But the
grep count will include test attrs inside `#[cfg(test)]` blocks that the HIR pass sees only if
the HIR database was loaded with `cfg(test)` active. If the HIR loader does NOT activate
`cfg(test)` by default, the self-dogfood count will be lower than the grep bound. The RFC should
clarify whether the HIR loader activates `cfg(test)` and adjust the lower-bound assertion
accordingly (or use a grep that excludes `#[cfg(test)]` blocks).

---

## D4. Graph-specs-rust update against real code

**Proposed Cypher (one):**

```cypher
// arch-ban-test-only-trait-impls.cypher
//
// Rule: no pub trait in the `ports` crate should have ALL its implementors
// reachable ONLY from test entry points and NONE from production entry points.
//
// Rationale (SOLID / ISP lens): a pub trait in the ports layer that is
// exercised exclusively via test entry points — never via a production
// cli_command, mcp_tool, http_route, cron_job, or websocket — is either:
//   (a) a leaked abstraction: the trait was designed for a future adapter
//       that does not yet exist, meaning the production code does not
//       actually depend on this interface yet, or
//   (b) an ISP violation: the trait bundles behaviour only tests need, and
//       production code has already substituted a concrete type, bypassing
//       the trait entirely.
// In either case, the correct fix is either to delete the trait or to wire
// a production adapter through it.
//
// Inputs:
//   :Item.reachable_from_production_entry  (bool) — written by RFC-042 dual-BFS
//   :Item.reachable_from_entry             (bool) — existing attr
//   :Item.kind                             — "trait" / "impl_block" etc.
//   :Item.crate                            — filter to ports crate
//   :EntryPoint.kind                       — to distinguish production vs test seeds
//
// Expected: zero rows on a clean tree (preventative policy).
// If graph-specs-rust has port traits reachable only from tests, those
// are cleanup findings — the rule lands as a cleanup driver in that case.

MATCH (i:Item)
WHERE i.kind = 'trait'
  AND i.crate =~ 'ports.*'
  AND i.reachable_from_entry = true
  AND i.reachable_from_production_entry = false
  AND i.is_test = false
RETURN i.qname AS qname,
       i.name AS name,
       i.crate AS crate,
       i.file AS file,
       i.line AS line
ORDER BY qname ASC
```

**Filed at (or proposed for):** `.cfdb/queries/arch-ban-test-only-trait-impls.cypher` on
`yg/graph-specs-rust`.

**Citation against current graph-specs-rust pinned SHA `2aedd013`:**
The current graph-specs-rust `ports/src/lib.rs:15` declares `pub trait Reader` and
`ports/src/lib.rs:29` declares `pub trait ContextReader`. `ContextReader` has one implementor
in production (`adapters/markdown/src/lib.rs:89: impl ContextReader for MarkdownReader`) and one
in a test-only context (`ports/tests/context_reader.rs:12: impl ContextReader for ErrStub`
— an explicitly test-only compile-proof struct). The `ErrStub` impl IS reachable from
`#[test] fn context_reader_contract_is_implementable_and_object_safe` (the only test in
`ports/tests/context_reader.rs`). Whether the MarkdownReader impl is also reachable from
a production entry point depends on whether `application/src/main.rs` creates a
`Box<dyn ContextReader>` — the production binary IS a production entry point. If it does,
`ContextReader` itself is reachable from production and this rule fires zero rows. If it does
not, the trait may already be a finding under this rule.

Regardless of the current finding count, the rule represents the ISP invariant the port layer
SHOULD enforce: every port trait should have at least one production call path.

**Intent:** zero-violation policy from day one (the rule should be a guard that `ContextReader`
stays wired to a production entry point, not just to test drivers).

**Rationale:** From the SOLID/ISP lens, a `pub trait` in the port layer that is exercised
exclusively via test entry points (no `cli_command` / `mcp_tool` / `http_route` seed reaches an
implementor) is a classic "interface defined before any concrete user exists" — a violation of
ISP's "don't force users to depend on things they don't need" because the port layer's own
production context is not using the interface yet. RFC-042's `reachable_from_production_entry`
attribute makes this distinction mechanically checkable for the first time, and graph-specs-rust's
`ports/` layer is the natural first target because its trait surface is small and well-defined.
