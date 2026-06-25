# SOLID / Component Principles verdict — RFC-047a

## Verdict

REQUEST CHANGES — one targeted amendment required: the salvage test (`impact_seed_binding.rs:49`) currently hardcodes `*1..5`, which means it does NOT prove B1 works; the 47-0 Tests block must mandate that the test is updated to use `*1..` once B1 lands, or a separate parse-only unit test must be added to 47-0's Tests block so B1's correctness is independently verifiable before the integration composite runs.

The amendment that flips me to RATIFY: add to 47-0's Unit row (complement §7): "...plus a dedicated parser unit test that `parse("MATCH (a)<-[:CALLS*1..]-(b) RETURN a")` succeeds and `edge.var_length == Some((1, u32::MAX))`; the salvage test's IMPACT_QUERY is updated from `*1..5` to `*1..` in the same PR (see §3.4 — the test switches to the open form once B1 lands)."

Everything else in the complement is sound. Full analysis below.

---

## Per-blocker analysis

### B1 — open-range `*N..` parse gap

**Verified at:** `crates/cfdb-query/src/parser/match_clause.rs:82-86`.

```rust
let range = just('*')
    .ignore_then(digits())
    .then_ignore(just("..").padded())
    .then(digits())      // upper bound — required; open form cannot parse
    .boxed();
```

Both bounds are required. `*1..` cannot parse. Confirmed.

**AST reuse claim verified at:** `crates/cfdb-core/src/query/ast.rs:106-108`. The doc comment says `[:LABEL*1..5] → Some((1, 5))` — the tuple `Option<(u32, u32)>` is the existing type. The complement proposes `*1..` → `Some((1, u32::MAX))`. No new variant needed. YAGNI claim holds.

**SRP assessment:** B1 lives entirely in `cfdb-query` (the parser crate). The only dependency of `cfdb-query` is `cfdb-core` (`crates/cfdb-query/Cargo.toml:10`). The change touches nothing in `cfdb-petgraph` or `cfdb-cli`. One reason to change: "how the grammar encodes an open upper bound." SRP clean.

**CCP assessment:** `cfdb-query` is the parser crate. Grammar rules change together. The open-bound grammar addition belongs here and nowhere else. No domain import signature mismatch.

**Gap I'm flagging:** The salvage test `crates/cfdb-cli/tests/impact_seed_binding.rs:49` contains:
```rust
const IMPACT_QUERY: &str = "MATCH (seed:Item)<-[:CALLS*1..5]-(affected:Item) \
     WHERE seed.qname IN $seeds \
     RETURN DISTINCT affected.qname AS qname";
```
This uses `*1..5`, not `*1..`. The test comment at line 21 says "switches to the open `*1..` form once B1 lands" (RFC-047a §3.4 also says this). However, neither the re-cut 47-0 Tests block (complement §7) nor the salvage test itself names a dedicated parser unit test. The 47-0 Unit row says:

> "`parse("... *1.. ...")` is Ok and yields var_length `(1, u32::MAX)`"

This is correct in spirit but the test **file** named by 47-0 (`impact_seed_binding.rs`) does not currently exercise the open form — it uses `*1..5`. The amendment is: the 47-0 Tests block must explicitly require (a) a parser unit test for `*1..` parse success + `var_length == Some((1, u32::MAX))` and (b) update of the IMPACT_QUERY constant in `impact_seed_binding.rs` from `*1..5` to `*1..`, in the same 47-0 PR. Without this the B1 correctness claim is not exercised by any test that actually runs the open form end-to-end.

### B2 — explicit-bound clamp

**Verified at:** `crates/cfdb-petgraph/src/eval/pattern/path.rs:205-208`:
```rust
let (min_depth, max_depth) = edge.var_length.unwrap_or((1, 1));
let max_depth = max_depth
    .max(min_depth)
    .min(DEFAULT_VAR_LENGTH_MAX.max(min_depth));
```

`DEFAULT_VAR_LENGTH_MAX = 5` (`eval/mod.rs:64`). Its doc at `:62` says "when a variable-length pattern OMITS its upper bound." But the code applies it to ALL var-length patterns, including explicit `*1..10`. So `*1..10` is silently clamped to 5. Confirmed contradiction.

**SRP assessment:** The fix lives entirely in `cfdb-petgraph` (the evaluator crate). One reason to change: "when the default depth cap applies." SRP clean.

**Is B2 a separate reason-to-change from Q1 (open-form policy)?**

I must be precise here. B2 has two sub-decisions:
1. **Fix the explicit-bound bug** (stop clamping `*1..10` → 5): this is a bug fix with no policy ambiguity. The doc says explicit bounds are honoured; the code contradicts the doc. CCP verdict: this belongs in 47-0 alongside B1 because both are prerequisites for the canonical `impact` query to work correctly. A query author writing `*1..10` to bound a specific search would get wrong answers today.
2. **Set the open-form policy** (Q1): unbounded-via-visited-set vs. cap at `DEFAULT_VAR_LENGTH_MAX`. This IS a separate reason to change from (1) — it is a design decision with a contested answer, not a bug correction. However: since there are ZERO open-form queries in the tree today (`match_clause.rs:82-86` made them unparseable), B2-open-form cannot break any existing queries. The blast radius of choosing either policy is bounded to new queries only.

**CCP conclusion on B2 split:** Grouping both sub-decisions in 47-0 is justified: (a) they share the domain import signature (both touch `traverse_bfs` in `path.rs`), and (b) neither can be fully tested until B1 lands (B2-explicit needs B1 to write a non-clamped test; B2-open needs B1 to write an open-form query). They change together because they test together. **No mandate to split B2 into a standalone `fix:` issue** — but the council's Q1 decision must be explicitly recorded in the 47-0 implementation (a comment in `path.rs` citing this RFC, not just a code change).

**On latent G6 / #486:** The complement is correct that B2's explicit-bound truncation is a latent inconsistency pre-existing `impact`. However, #486 is filed as a separate latent G6 bug. The complement's choice to fix B2 in 47-0 rather than deferring to #486 is defensible precisely because 47-0 has to touch `path.rs` anyway to implement the open-form semantics Q1 decides. The boy-scout rule (`CLAUDE.md §7`) supports fixing pre-existing failures in the same PR when the fix is within scope.

### B3 — HIR dogfood requirement

**Verified at:** `crates/cfdb-extractor/src/lib.rs:18` — "Out of scope for v0.1: resolved cross-crate `CALLS` (Item → Item)".

The syn-based `extract_workspace` never resolves `Item→Item CALLS`. Resolved `CALLS` requires the HIR path (`cfdb-hir-extractor`). The complement's re-specification of 47-A's dogfood to use `cfdb extract --hir` is correct.

**SRP/ISP assessment of the 47-A test boundary:** The `cfdb-hir-extractor` crate is already a feature-gated optional dep of `cfdb-cli` (`cfdb-cli/Cargo.toml:70`, `feature = "hir"`). The 47-A test will need `#[cfg(feature = "hir")]` gating or must rely on the CLI's `--hir` flag. This is an adapter concern, not a port concern — clean.

**One concrete concern:** The 47-A Tests block says "cfdb extract --hir to a temp keyspace." This is a CLI integration test, not an in-process test. The `predicate_library_dogfood.rs` pattern calls `cfdb_extractor::extract_workspace` in-process; 47-A cannot follow that pattern. The test will need to either (a) shell out to the `cfdb` binary or (b) call `cfdb_hir_extractor`'s library surface directly. The complement does not nail down which. I recommend the 47-A Tests block specify "call `cfdb_hir_extractor::extract_workspace_hir(&root)` in-process (same tier-1 real-infra pattern as `predicate_library_dogfood.rs`, not a shell-out)" — if that entry point exists. This is a documentation precision issue, not a blocker, and I leave it for rust-systems to verify the HIR cost question.

### Correction of record (Q4)

The complement's approach — a separate superseding document, not an in-place edit of the ratified RFC-047 — is architecturally correct. RFC-047 is the record of what the original council deliberated. Amending a ratified RFC in place destroys the audit trail of why 47-0 was originally scoped as it was. The complement is the right mechanism. The `RATIFIED.md` entry for RFC-047 should be updated to note the superseding complement — this is not currently mentioned in the complement §2 scope.

**Amendment note (non-blocking):** Add to complement §2 Scope item 1: "Update `council/RATIFIED.md` entry for RFC-047 to note this complement supersedes §3.2/§5 clean-arch mechanics finding."

---

## Contested-question positions

### Q1 — open-form `*N..` policy: cap vs. unbounded (engage: rust-systems, clean-arch)

**My position:** The visited-set dedup at `path.rs:211,230` is confirmed. `if visited.insert(target)` on a `BTreeSet<NodeIndex>` means each node is enqueued at most once — the frontier is O(V) regardless of `max_depth`. So there is no asymptotic argument for capping the open form.

However, the SRP concern is NOT performance — it is **predictability of the public language contract**. `DEFAULT_VAR_LENGTH_MAX = 5` is a `pub(super)` constant that sits in the evaluator. If the open form is capped at 5, a query author writing `*1..` gets 5-hop semantics. If it is unbounded, a query author gets whole-graph reachability. These are different contracts, and the choice becomes part of the crate's public behavior.

My recommendation to rust-systems and clean-arch: **unbounded (visited-set-only bound)**. The argument for a safety cap is a usability concern ("accidental whole-graph traversal") not a correctness concern. The correct affordance for a depth limit is `*1..N` (explicit bound, B1 lands it). A safety cap on `*1..` conflates "I didn't specify a bound" with "I want at most 5 hops," which is wrong semantics. If a user wants 5 hops, they should write `*1..5`. The RFC-047 §3.2 original intent ("unbounded by default") was correct; the B2 bug just made it unreachable.

**Engagement result:** Challenging rust-systems to confirm the O(V+E) argument at `path.rs:211,230` and confirm there is no `max_depth` special path that exits early before the visited-set check. Challenging clean-arch to confirm unbounded does not violate any `StoreBackend` contract (the backend just returns rows; the BFS depth is purely evaluator-internal).

### Q3 — 47-0 / 47-A boundary + HIR dogfood cost (lead, engage rust-systems, ddd)

**My lead position on the boundary:** The 47-0 / 47-A cut is SRP-correct:
- 47-0 reason to change: "how variable-length bounds are parsed and enforced" (grammar + evaluator mechanics). Changes when the language spec changes.
- 47-A reason to change: "what the canonical reverse-reachability query is and whether it correctly traverses cfdb-self's CALLS graph." Changes when the query pattern changes or when cfdb-self's call graph structure warrants new assertions.

These are orthogonal axes. 47-A genuinely depends on 47-0 (the canonical query uses `*1..`, which needs B1). The blocking dependency is real and correctly modelled.

**On HIR cost:** `cfdb-hir-extractor` pulls `ra_ap_*`. Cold compile is 90-150 s (per memory note). This is acceptable for a CI integration test gated behind `features = ["hir"]` in the test manifest — the same pattern as other heavy feature-gated tests. It is NOT acceptable in a test that runs on every `cargo test` without feature isolation. The 47-A Tests block should specify the feature gate explicitly.

**Challenge to rust-systems:** Is there a lightweight in-process CALLS-resolution path that does not require the full `ra_ap_*` compile? If yes, 47-A should use it. If no, the feature-gated HIR path is the only option and the Tests block should say so.

**Challenge to ddd:** Does the "self-dogfood seeds a known leaf fn and asserts known callers appear" invariant belong in 47-A, or is this a cross-cutting concern that transcends the query-mechanics slice? (My view: it belongs in 47-A. The blast-radius concept is a view, not a domain concept per RFC-047 ddd RATIFY — no issue there.)

### Q4 — correction of record: complement vs. in-place edit

**My position (stated above under B3):** Complement supersedes, ratified RFC stays intact as audit record. The only gap is that `council/RATIFIED.md` should be updated to point to the complement. Adding this update to the 47-0 scope (or as a docs-only chore issue) is the tightest fix.

---

## Test-surface prescription notes

### 47-0 Tests block — required amendment

The current 47-0 Unit row:

> "`parse("... *1.. ...")` is Ok and yields var_length `(1, u32::MAX)`; `*1..3` honours 3 (no clamp); the open form follows the council-ratified policy (Q1). Plus the fixture composition test (reverse `<-[:CALLS*1..N]-` + `IN $seeds` Param::List ⇒ caller union; single-seed control proves membership filtering) — `crates/cfdb-cli/tests/impact_seed_binding.rs`."

**Amendment required:** Add explicit mandate: "The parser unit test (`parse("... *1.. ...")`) is a SEPARATE test function from the fixture composition test. The `impact_seed_binding.rs` IMPACT_QUERY constant at line 49 is updated from `*1..5` to `*1..` in the 47-0 PR (the test currently bypasses B1 by hardcoding a bound)."

Without this mandate, an implementer could ship B1 (parser change) without running the open form through the test that's supposed to validate the composition. The two-part test structure (parser unit + composition integration) must be named explicitly.

The `*1..3` explicit-bound unit test (no clamp) is correct as stated — this is the B2-explicit-bound regression test.

### 47-A Tests block — precision note (non-blocking)

The Self dogfood row says "cfdb extract --hir to a temp keyspace." Recommend clarifying: "call `cfdb_hir_extractor` library surface in-process (feature-gated `#[cfg(feature = "hir")]`) — NOT shell-out; follow the tier-1 real-infra pattern from `predicate_library_dogfood.rs`." This makes the implementer's task concrete.

The Cross dogfood row (`none — rationale: no schema/ban surface`) is correct. No companion lockstep is needed since there is no keyspace schema change.

Target dogfood row is acceptable as written.

---

## Stability metrics (component principles)

The changes are purely within two existing crates:
- `cfdb-query` (B1 parser change): Ce = 1 (`cfdb-core`). Ca = all crates that parse queries. This crate is already stable (abstract: it exposes `parse()` as its pub surface; I = low). The open-bound grammar addition does not change the `parse()` return type — it extends what successfully parses. Stability is preserved.
- `cfdb-petgraph` (B2 evaluator change): the change is internal to `traverse_bfs` in `path.rs`. The `StoreBackend` trait surface is untouched. No Ca impact.

No new crate is proposed. ADP is trivially satisfied (no new edges in the crate dependency graph). CRP / REP are untouched. The SDP direction holds: `cfdb-petgraph` depends on `cfdb-query` (stable), which depends on `cfdb-core` (most stable). All arrows point toward stability.

ISP check: `impact` (47-B, not landed yet) will consume only the `query`/`execute` surface of `StoreBackend` — the same two methods `list_callers` uses. No ISP violation introduced.

---

## Summary

The complement is architecturally sound. The B1 / B2 / B3 classification is correct. The 47-0 / 47-A slice boundary is SRP-clean. The only gap is that the 47-0 Tests block does not explicitly mandate updating the salvage test from `*1..5` to `*1..`, which means B1 could ship without the open form ever being exercised by the named test file. The amendment is a single sentence addition to the Unit row.

---

## Phase B cross-challenge convergence (rust-systems mailbox)

### Q1 — converged: UNBOUNDED (visited-set terminates)

rust-systems confirmed the visited-set bound at `path.rs:230`. Position aligns. Both lenses require a code-level policy anchor comment at the `u32::MAX` branch in `traverse_bfs` after B1 lands. The comment must state: "Open upper bound (`u32::MAX` sentinel from the `*N..` grammar form): the visited-set at line 211 bounds traversal to O(V+E); no depth cap applies. Explicit finite bounds (`*N..M`) are honoured as written — see `DEFAULT_VAR_LENGTH_MAX` above, which applies only to this open-form branch." Without this comment, `DEFAULT_VAR_LENGTH_MAX` in `eval/mod.rs:64` is a readability trap for the next author.

This comment requirement is added to my amendment as a second checkable item. The amendment that flips me to RATIFY now has two parts (original + this addition).

### Q3 — converged: split `impact_seed_binding.rs` (unit) from `impact_hir_dogfood.rs` (integration-live)

rust-systems correctly identified a CCP violation in the 47-A prescription: fast composition tests and slow HIR-extraction dogfood have different reasons to change and different run schedules. Keeping them in one file conflates two test cohesion axes.

**Agreed prescription:**
- `crates/cfdb-cli/tests/impact_seed_binding.rs` — stays as 47-0 artifact (fast, fixture-injected, no HIR dep). IMPACT_QUERY updated to `*1..` in the 47-0 PR (per the original amendment).
- `crates/cfdb-cli/tests/impact_hir_dogfood.rs` (new, 47-A artifact) — gated `#[cfg_attr(not(feature = "integration-live"), ignore)]`, calls `cfdb_hir_extractor` in-process (not shell-out), feature-gated behind `hir`, seeds a known leaf fn, asserts its known callers in cfdb-petgraph/cfdb-cli appear in the affected set.

This matches the `CLAUDE.md §5` `required-features = ["integration-live"]` pattern for live-infra tests.

**Updated 47-A Self-dogfood row:** "Separate test file `crates/cfdb-cli/tests/impact_hir_dogfood.rs`, gated `#[cfg_attr(not(feature = "integration-live"), ignore)]`, calls `cfdb_hir_extractor` library surface in-process (not shell-out), seeds a known leaf fn in cfdb-core, asserts known callers in cfdb-petgraph/cfdb-cli appear in the affected set."

### Updated amendment summary (both parts required to flip to RATIFY)

1. **47-0 Tests Unit row:** Add "A dedicated parser unit test (separate function) asserts `parse("MATCH (a)<-[:CALLS*1..]-(b) RETURN a")` succeeds and `edge.var_length == Some((1, u32::MAX))`. The `impact_seed_binding.rs` IMPACT_QUERY constant (line 49) is updated from `*1..5` to `*1..` in the 47-0 PR."

2. **47-0 implementation note (code comment):** The `traverse_bfs` implementation after B1 must include a comment at the `u32::MAX` branch explaining the open-form semantics and that `DEFAULT_VAR_LENGTH_MAX` applies only to this branch (not to explicit finite bounds). Without it the constant is a readability trap.

3. **47-A Tests Self-dogfood row:** Replace "cfdb extract --hir to a temp keyspace" with the split-file prescription above (separate `impact_hir_dogfood.rs`, `integration-live` gate, in-process library call).

---

## Phase B — R2 refinements (rust-systems response)

### Q1 — loop structure verified, unbounded confirmed

rust-systems verified `path.rs:216-232` at the loop level. The `if depth >= max_depth { continue; }` at `:220` skips expanding a node's outgoing edges but does NOT skip the `visited.insert` check at `:230` — that guard is at enqueue time, not dequeue time. So `max_depth = u32::MAX` is safe: each node is enqueued at most once regardless of `max_depth` value. A finite graph drains the queue after at most `|V|` dequeues. O(V+E) confirmed.

My Q1 concern (could `u32::MAX` defeat the visited-set?) is resolved. The structure is safe. Q1 fully converged: UNBOUNDED.

### Q3 — no single `extract_workspace_hir` surface; two-step call required

rust-systems verified `cfdb-hir-extractor/src/lib.rs:89-92`. There is NO single `extract_workspace_hir(&root) -> (Vec<Node>, Vec<Edge>)` mirror of `cfdb_extractor::extract_workspace`. The HIR path requires two calls:

```rust
let (db, vfs) = cfdb_hir_extractor::build_hir_database(&root)?;
let (nodes, edges) = cfdb_hir_extractor::extract_call_sites(&db, &vfs)?;
```

This adds `cfdb-hir-extractor` as a `[dev-dependencies]` of `cfdb-cli`, dragging `ra_ap_*` into cfdb-cli's test compile tree. This must be feature-gated.

**Updated 47-A Self-dogfood Tests row (final):**

> Separate test file `crates/cfdb-cli/tests/impact_hir_dogfood.rs`, gated `#[cfg_attr(not(feature = "integration-live"), ignore)]`. Calls `cfdb_hir_extractor::build_hir_database(&root)` then `cfdb_hir_extractor::extract_call_sites(&db, &vfs)` in-process (NOT shell-out — binary path is not guaranteed to exist in a test run). `cfdb-hir-extractor` added to `cfdb-cli`'s `[dev-dependencies]` behind `integration-live` feature gate in `cfdb-cli/Cargo.toml`. Seeds a known leaf fn in cfdb-core; asserts its known callers in cfdb-petgraph/cfdb-cli appear in the affected set.

### Updated amendment summary (final — 3 items, all checkable)

1. **47-0 Tests Unit row:** Add dedicated parser unit test (separate function from fixture composition test): `parse("MATCH (a)<-[:CALLS*1..]-(b) RETURN a")` succeeds and `edge.var_length == Some((1, u32::MAX))`. Update `impact_seed_binding.rs:49` IMPACT_QUERY from `*1..5` to `*1..` in the same PR.

2. **47-0 implementation note:** `traverse_bfs` carries a comment at the `u32::MAX` sentinel branch: "Open upper bound — visited-set at line 211 is the sole termination guarantee (O(V+E)); `DEFAULT_VAR_LENGTH_MAX` applies only to this branch, not to explicit finite bounds."

3. **47-A Tests Self-dogfood row:** Replace with the two-step in-process prescription above (split file, `integration-live` gate, `build_hir_database` + `extract_call_sites`, `cfdb-hir-extractor` dev-dep behind feature gate).
