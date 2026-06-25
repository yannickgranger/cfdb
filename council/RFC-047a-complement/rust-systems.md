# rust-systems verdict — RFC-047a

## Verdict

REQUEST CHANGES — the B2 fix as written is correct for explicit bounds but Q1 (open-form semantics) must be resolved before this RFC is ratified; additionally, the latent B2 truncation is a **live production bug** in `.cfdb/queries/vsb-multi-resolver.cypher` that must be fixed independently or as part of 47-0 (not deferred to 47-B).

**Exact amendment required to flip to RATIFY:**
1. Add one sentence to §3.2 stating that the open-form `*N..` policy selected by Q1 must be a `const` or inline doc-comment so it is inspectable at read time (not just "council decision" — the implementer needs the literal policy written in code). Either `// open upper: unbounded (visited-set terminates)` or `// open upper: capped at DEFAULT_VAR_LENGTH_MAX` must appear at the call site in `path.rs`. This is not a structural change, just a doc-anchor requirement.
2. Confirm in §2 "Ships" that the live `.cfdb/queries/vsb-multi-resolver.cypher` truncation is fixed in 47-0 (explicit-bound clamp removal), not silently left broken.

---

## Per-blocker analysis

### B1 — open-range parse gap: CONFIRMED, fix design sound

`match_clause.rs:82-86`:

```rust
let range = just('*')
    .ignore_then(digits())
    .then_ignore(just("..").padded())
    .then(digits())          // upper bound required — `*1..` fails here
    .boxed();
```

Both bounds are required by the current parser. `*1..` yields a parse error. The fix (make upper `digits()` optional, absent ⇒ `u32::MAX` sentinel) reuses the existing AST tuple `Option<(u32, u32)>` at `ast.rs:108` — no new variant, correct YAGNI call. The grammar addition is a **query-language change**, not a keyspace schema change: it adds no node label, no edge label, no attribute, and no `SchemaVersion` bump. This is correctly classified.

One implementation note: `u32::MAX` as the open-upper sentinel interacts with the B2 fix. The evaluator must distinguish `M == u32::MAX` (open-form, apply chosen policy) from `M == u32::MAX` written explicitly (which no sane author would write, but the grammar does not prevent). This is a footgun at the sentinel boundary — the RFC should note it explicitly in §3.2 and recommend `u32::MAX` is documented as a reserved sentinel in the grammar, not a valid explicit bound.

### B2 — depth cap contradicts its own doc: CONFIRMED live bug, scope larger than stated

`eval/mod.rs:64` doc: "Maximum BFS depth when a variable-length pattern **omits its upper bound**."
`path.rs:205-208`:
```rust
let (min_depth, max_depth) = edge.var_length.unwrap_or((1, 1));
let max_depth = max_depth
    .max(min_depth)
    .min(DEFAULT_VAR_LENGTH_MAX.max(min_depth));   // clamps ALL var-length
```

For `*1..10`: `10.max(1).min(5.max(1))` = `10.min(5)` = **5**. The clamp applies unconditionally to every var-length pattern regardless of whether the upper bound is explicit or open.

**The "omitted upper bound" case was never reachable** before B1 lands — `unwrap_or((1,1))` fires only when `var_length` is `None`, but `None` means no `*` quantifier at all (single-hop, not open-form). After B1 lands, `u32::MAX` will be the new open-form representation; the clamp on `u32::MAX` is where the Q1 policy applies.

**Live production impact — `.cfdb/queries/vsb-multi-resolver.cypher:67`:**
```cypher
MATCH (h)-[:CALLS*1..10]->(f:Item {kind: 'fn'})
```
This is the self-dogfood ban rule (`violations` gate, CI-enforced). With the current clamp, this query silently traverses only 5 hops instead of 10. Call chains deeper than 5 are invisible to the split-brain detector. This is a **production correctness bug** in cfdb's own ban rules, not a theoretical concern. `examples/queries/vertical-split-brain.cypher:73` documents the history: "v0.2 `DEFAULT_VAR_LENGTH_MAX = 8`" — the default was once 8, then someone dropped it to 5 without fixing the clamp. The comment is now stale evidence of the regression.

**The fix is correct:** honour `M` for explicit finite bounds, apply the chosen open-form policy only for `M == u32::MAX`. This is a two-line conditional in `traverse_bfs`, well-scoped.

### B3 — extract_workspace has no resolved CALLS: CONFIRMED

`cfdb-extractor/src/lib.rs:18`: "Out of scope for v0.1: resolved cross-crate `CALLS` (Item → Item)." The extractor emits `INVOKES_AT` + stub nodes only. Resolved `Item→Item CALLS` lives in `cfdb-hir-extractor/src/emit.rs` / `call_site_emitter/`.

The `impact_seed_binding.rs` salvage test (`crates/cfdb-cli/tests/impact_seed_binding.rs:212`) calls `cfdb_extractor::extract_workspace` for its dogfood assertion. It finds `CALLS` edges only because cfdb-self is extracted with the HIR path in practice (the live keyspace has 2197 `CALLS` edges per RFC-047 §1). But the test currently uses the syn-only extractor, which produces zero resolved `CALLS` edges — it only produces `INVOKES_AT`. The dogfood at `impact_seed_binding.rs:210-244` likely passes structurally (union semantics hold) but against a near-empty `CALLS` graph. When 47-A re-runs against a HIR keyspace, it will be the first meaningful test of the traversal at scale.

The re-specification of 47-A to use `cfdb extract --hir` is **correct but implies a test-time CI cost**: `ra_ap_*` pulls ~90-150s cold. The RFC acknowledges this (§3.3) but the `Tests:` block for 47-A should explicitly mark the self-dogfood as `#[ignore]` / `cfg(feature = "integration-live")` or equivalent, so CI doesn't time out on every PR. This is a TEST PRESCRIPTION gap the implementer needs to know about.

### Correction of record (§1): CONFIRMED and well-argued

The false finding was "no list-binding path exists." Evidence: `Param::List` at `ast.rs:54-57`, evaluator at `predicate.rs:115-117`, CLI at `param_resolver.rs:8,90`. The council inspected only the `--input`/`--params` CLI surface, which §3.2 itself scopes out. The correction is accurate and the evidence chain is fully cited.

---

## Contested-question positions

### Q1 (CRUX, lead) — open-form `*N..` semantics: cap at DEFAULT_VAR_LENGTH_MAX or visited-set-unbounded?

**My position: HONOUR VISITED-SET (unbounded), with a mandatory code-anchor.**

The asymptotic argument in the complement is **correct**. `traverse_bfs` at `path.rs:211,230` uses a `BTreeSet<NodeIndex>` as its visited set:
```rust
let mut visited: BTreeSet<NodeIndex> = BTreeSet::new();
// ...
if visited.insert(target) {
    queue.push_back((target, next_depth));
}
```
A node is inserted into the queue at most once (the `insert` guard at :230 is the deduplication point). Therefore the frontier drains after at most `|V|` visits. Each visit examines the outgoing/incoming edges of one node once. Total work: O(V + E). A `max_depth` cap in this BFS does not change the asymptotic complexity — it only prunes the frontier earlier. The cap buys no algorithmic win.

**However**, the RFC-047 §3.2 "unbounded by default" claim was stated but not accompanied by any depth-policy doc in the evaluator. The concern is: future open-form queries by authors who don't read the RFC will not know the policy. The code anchor requirement (amendment 1 above) addresses this.

**Counter-argument to consider (safety-cap):** RFC-047 §3.2 describes `--max-depth` as an optional future CLI flag for "very dense targets." If open-form is truly unbounded, an author who forgets `*1..N` on a dense graph gets a whole-graph BFS. The visited-set guarantee means this terminates, but for a 2197-node graph with average degree k, a full BFS touches O(k*V) edge traversals in the worst case. Sub-second for cfdb-self, but potentially slow on a 10k-node graph. A safety cap of 5 is overly conservative; a cap of `u32::MAX` (truly unbounded) is accurate but silent. A middle ground: `DEFAULT_VAR_LENGTH_MAX = 5` for the cap case, with clear doc that open-form (`*N..`) uses the default only if the author omits an explicit bound — which is the policy the doc already states. RFC-047a is correct to honour the doc by applying the cap ONLY to open-form, not to explicit bounds.

**Broadcast to clean-arch and solid:** see SendMessage below.

### Q2 — explicit-bound clamp is a LIVE BUG, must fix independently or in 47-0

The `.cfdb/queries/vsb-multi-resolver.cypher:67` query uses `*1..10`, truncated to 5 today. This is a shipped query in CI's `violations` gate. The B2 fix (honour explicit finite bounds) must land as part of 47-0 or as a standalone `fix:` issue filed before 47-0, not as part of 47-B. Leaving it in 47-B means the vsb-multi-resolver ban rule silently produces false negatives during all of 47-0 and 47-A development.

**The complement §2 says B2 is in 47-0's scope ("re-cut slices" §7 aligns B1+B2 in one slice).** I read that as: explicit-bound fix lands in 47-0. That is correct. But the RFC text itself in §3.2 says "the open-form semantics is the council's central question (Q1)" — which is only the open-form half. The explicit-bound fix should be unconditional (no council Q1 dependency) and the wording should be tightened to reflect that.

**Engaged: clean-arch** (Q2 cross-challenge, per BRIEF §3).

### Q3 — 47-0/47-A boundary + HIR-dogfood cost

B1+B2 belong together in 47-0: the parser produces the form (B1) and the evaluator interprets it (B2). These have one shared invariant — a round-trip guarantee: `parse("*1..N")` yields `(1,N)`; `traverse_bfs` honours `N` as written. Splitting them is artificial.

The HIR-dogfood cost (90-150s cold) is the only substantive concern for 47-A. The `impact_seed_binding.rs` test currently calls `cfdb_extractor::extract_workspace` (syn-only, fast, but zero resolved `CALLS`). For 47-A, the prescribed test calls `cfdb extract --hir` (slow, CALLS-populated). The `Tests:` block for 47-A is correct in requiring HIR extraction — without it the dogfood is testing a near-empty graph and proving nothing about real blast radius. The cost is acceptable because 47-A is an integration test, not a unit test. It MUST be gated behind a feature flag or `#[ignore]` to keep CI under 5 minutes.

**Amendment needed in 47-A Tests row:** "Self dogfood: `cfdb extract --hir`... `#[ignore]` unless `CFDB_INTEGRATION=1`" or equivalent.

**Engaged: solid and ddd** (Q3 cross-challenge, per BRIEF §3).

### Q4 — correction of record: amend RFC-047 or let complement supersede?

Let the complement supersede. The ratified RFC-047 is the audit record of what the council believed at deliberation time. Amending it in place would destroy the paper trail of why RFC-047a was necessary and obfuscate the *council foundation claims need verification* anti-pattern. RFC-047 §3.2/§5 should carry a header note: `> Superseded by RFC-047a §1 for list-binding mechanics and §3 for query mechanics.` This is a one-line annotation, not a rewrite. The complement is the living document.

---

## Test-surface prescription notes

### 47-0 Tests block

Current `Tests:` is structurally correct. One gap:

- **Unit row** covers parse round-trips and clamp removal for explicit bounds. Add explicitly: "for `*1..N` where `N < u32::MAX`, `traverse_bfs` honours `N` without clamping to 5 (regression test against `.cfdb/queries/vsb-multi-resolver.cypher` truncation)."
- The salvage test in `crates/cfdb-cli/tests/impact_seed_binding.rs` currently uses `*1..5` hardcoded (because B1 was not yet fixed). After B1 lands, `IMPACT_QUERY` should switch to `*1..` (open form) to test the full composition. The `Tests:` block should note this transition.

### 47-A Tests block

Gap: the HIR-dogfood row lacks a feature-gate note. The test at `impact_seed_binding.rs:210-244` calls `extract_workspace` (syn-only, zero CALLS). For 47-A's stronger HIR dogfood, the test MUST be marked `#[ignore]` / gated behind `integration-live` / or run only via `CFDB_INTEGRATION=1`. Without this the test either (a) passes trivially on an empty CALLS graph, or (b) times out CI. Prescription:

```
  - Self dogfood (cfdb on cfdb): `cfdb extract --hir` cfdb-self to a temp keyspace (resolved CALLS), seed a known leaf fn in cfdb-core, assert its known callers in cfdb-petgraph/cfdb-cli appear in the affected set. Run only under `#[cfg_attr(not(feature = "integration-live"), ignore)]` or `CFDB_INTEGRATION` env gate to keep CI under 5-minute timeout budget.
```

---

## Phase B cross-challenge outcomes (post-mailbox)

### Q1 — converged: UNBOUNDED (visited-set terminates). Loop structure verified.

Challenge from solid: "Is there any path in `traverse_bfs` where `max_depth` causes an early exit BEFORE the visited-set check?"

Verified at `path.rs:216-232`. The `if depth >= max_depth { continue; }` at :220 skips expanding a node's **outgoing edges** — it does NOT gate the `visited.insert` at :230. The deduplication happens at **enqueue time**: any node already in `visited` is not re-enqueued regardless of depth. So with `max_depth = u32::MAX`: no reachable finite node ever triggers the `:220` continue; the queue drains after at most `|V|` dequeues. Confirmed O(V+E); the visited-set is the sole termination guarantee.

Outcome: solid and rust-systems **converged** — unbounded-via-visited-set is correct semantics and safe. The code-anchor amendment (policy comment at the `u32::MAX` branch post-B1) stands as a verifiable requirement.

### Q3 — prescription updated: in-process library call, not CLI shell-out

Challenge from solid: "Does cfdb-hir-extractor expose a library-callable `extract_workspace_hir(&root)` surface?"

Verified at `cfdb-hir-extractor/src/lib.rs:89-92`. There is NO single-call `extract_workspace_hir`. The HIR path is a two-step:
1. `build_hir_database(&root)` (`hir_db.rs:66`) → `(DB, Vfs)`
2. `extract_call_sites(&db, &vfs)` (`call_site_emitter.rs:87`) → `(Vec<Node>, Vec<Edge>)`

**Updated 47-A Tests prescription** (supersedes the earlier "cfdb extract --hir" row):
```
  - Self dogfood (cfdb on cfdb):
      In-process via `build_hir_database(&root)` + `extract_call_sites(&db, &vfs)`.
      Add `cfdb-hir-extractor` as `[dev-dependencies]` in `cfdb-cli/Cargo.toml`
      behind the `integration-live` feature gate (pulls ra_ap_*, ~90-150s cold).
      Test must carry `#[cfg_attr(not(feature = "integration-live"), ignore)]`.
      Seed a known leaf fn in cfdb-core; assert its known callers in
      cfdb-petgraph/cfdb-cli appear in the affected set.
      Do NOT use `cfdb_extractor::extract_workspace` (syn-only, zero CALLS).
```

Outcome: solid and rust-systems **converged** — in-process library path is correct; the 47-A Tests block needs the above prescription verbatim. The RFC §7 47-A block is a REQUEST CHANGES target (Amendment 2 in the verdict above is hereby extended to include this prescription).

### Q1+Q2 clean-arch convergence (post-mailbox)

clean-arch confirms Q1 (unbounded + code anchor) and Q2 (explicit-bound fix unconditional, must land in 47-0). Three live truncations confirmed by clean-arch: `vsb-multi-resolver.cypher:67` (`*1..10`), `vertical-split-brain.cypher:119-120` (`*1..8`), `vertical-split-brain-drop.cypher:135-136` (`*1..8`). All three are silently capped at 5 today.

clean-arch notes that the B2 fix is confined entirely to `traverse_bfs` at `path.rs:200-237` inside `cfdb-petgraph` — no `StoreBackend` port change, no interface surface touched. Verdict from clean-arch lens: RATIFY (doc/test amendments absorbed, not architecture blockers).

**All three lenses converged on Q1 and Q2.** rust-systems REQUEST CHANGES amendments remain the blocker; they are verifiable doc/test fixes.
