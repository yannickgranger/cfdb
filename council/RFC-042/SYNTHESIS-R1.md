# RFC-042 council synthesis — Round 1

**Date:** 2026-05-17
**Convener:** captain (a0 session)
**Status:** REQUEST CHANGES (4/4 lenses) — no REJECTs.
**Action required:** RFC author applies the consolidated edits below to `docs/RFC-042-test-bench-entry-points.md`, then convener re-invokes Round 2 against the affected lenses for re-review.

---

## 1. Verdict roll-up

| Lens | Verdict | Blocking findings | Non-blocking findings |
|---|---|---|---|
| `clean-arch` | REQUEST CHANGES | 2 (port purity, composition root) | 0 |
| `ddd-specialist` | REQUEST CHANGES | 2 (homonym disambiguation, new-attr descriptor) | 0 |
| `solid-architect` | REQUEST CHANGES | 2 (SRP/CCP probe location, LSP/ISP trait surface) | 1 (CRP query duplication note) |
| `rust-systems` | REQUEST CHANGES | 1 (RS-1: trait surface ambiguity) | 3 (RS-3: `cfg(test)` clarification, RS-5: feature-flag scope, RS-6: third BFS option) |

**Convergent finding (3+ lenses):** the trait-surface impact of `entry_kind_filter` is the central blocker. Flagged by clean-arch (Finding 1+2), solid-architect (CR2), and rust-systems (RS-1).

---

## 2. The one cross-lens disagreement — resolved

**Question:** where does dual-BFS orchestration live?

| Position | Holder | Argument |
|---|---|---|
| **A.** CLI is orchestrator; `EnrichBackend::enrich_reachability` gains an `EntryPointKindFilter` enum parameter; CLI calls twice. | clean-arch (Finding 1) | "Port purity. 'Production kinds' is a CLI concern; the port should reflect what the orchestrator wants to do. Burying dual-BFS in the impl hides a CLI concern in the graph layer." |
| **B.** PetgraphStore is orchestrator; trait signature unchanged; `PetgraphStore::enrich_reachability` invokes `reachability::run` twice internally. | solid-architect (CR2 preferred), rust-systems (RS-1 second option) | "Stable abstractions for cfdb-core (no breaking trait change). Dual-BFS is a single semantic enrichment — both attributes always produced together because both derive from the same graph state. CLI doesn't need to know about two passes." |

**Synthesis verdict: adopt Position B (PetgraphStore-internal dual-BFS, trait unchanged).**

**Rationale:**

1. **Concrete dependency cost.** Position A requires editing `cfdb-core/src/enrich.rs:177` (the `EnrichBackend` trait), every `impl EnrichBackend` (including the `TestBackend` stub at line 267), and ripples to any downstream crate consuming `EnrichBackend`. Position B touches only `cfdb-petgraph/src/enrich_backend.rs:151-163` and `cfdb-petgraph/src/enrich/reachability.rs:75`. Smaller blast radius.

2. **Clean-arch's port-purity concern is preserved under Position B.** clean-arch's actual objection is that `BTreeSet<&str>` is a stringly-typed CLI concept leaking into the port. Under Position B, the filter is a module-private detail of `cfdb-petgraph::enrich::reachability::run` — it never crosses the `EnrichBackend` boundary. The `BTreeSet<&str>` stays inside `cfdb-petgraph`, where it is an implementation detail of how the BFS seeds are constructed. clean-arch's concern is fully resolved without breaking the trait.

3. **Composition root unambiguous.** clean-arch's Finding 2 (composition root underspecified) becomes moot — there is only one composition root: `PetgraphStore::enrich_reachability`. The CLI continues to make a single `store.enrich_reachability(&ks)` call; the dual-BFS happens behind that call.

4. **EnrichReport reporting (solid-architect D3 concern).** Under Position B, the impl naturally returns one `EnrichReport` that sums both passes' `attrs_written`. The arithmetic is encapsulated in the impl, not split across two CLI-level calls.

5. **Optional follow-up if Position A is ever needed.** If a future operator requirement demands per-pass control from the CLI (e.g. `--skip-test-pass`), Position A can be re-introduced as an additive trait method. Position B does not foreclose Position A. The reverse is also true, so the choice is reversible — pick the smaller-blast-radius option now.

**clean-arch lens, please re-verify against Position B in Round 2.** The stringly-typed concern is structurally addressed; the question is whether you agree the resolution is sufficient.

---

## 3. Consolidated edit list — RFC author applies these to `docs/RFC-042-test-bench-entry-points.md`

Each edit cites its source lens(es) so the Round 2 review can verify.

### EDIT 1 — §3.3 trait-surface clarification (BLOCKING)
**Source:** clean-arch Finding 1+2, solid-architect CR2, rust-systems RS-1.

Replace the §3.3 paragraph "Implementation choice — option (A) over option (B)" with text that:

a. Names the composition root explicitly: `PetgraphStore::enrich_reachability` (in `cfdb-petgraph/src/enrich_backend.rs:151-163`), NOT `cfdb-cli`.

b. States the trait surface is unchanged: `EnrichBackend::enrich_reachability` in `cfdb-core/src/enrich.rs:177` keeps its current signature `(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError>`. No `cfdb-core` change.

c. Specifies that `cfdb-petgraph::enrich::reachability::run` (the module-private helper at `reachability.rs:75`) gains a `filter: ReachabilityFilter` parameter (NOT `Option<&BTreeSet<&str>>`). The `ReachabilityFilter` is a `cfdb-petgraph`-private enum:
   ```rust
   enum ReachabilityFilter { All, ProductionOnly }
   ```
   Internal mapping `ProductionOnly → exclude {test, bench}` from the kind set lives inside `reachability.rs`, never crossing the crate boundary.

d. Specifies `PetgraphStore::enrich_reachability` calls `reachability::run(state, ReachabilityFilter::All)` followed by `reachability::run(state, ReachabilityFilter::ProductionOnly)`, writing both attribute pairs (`reachable_from_entry` / `reachable_entry_count` from pass 1; `reachable_from_production_entry` / `reachable_production_entry_count` from pass 2). The returned `EnrichReport.attrs_written` is the SUM of both passes' writes.

e. Adds a "Trait surface impact" subsection (per rust-systems RS-1 proposed edit shape) stating explicitly: "RFC-042 does NOT change the `EnrichBackend` trait. No downstream impl is affected. No `cfdb-core` API change."

### EDIT 2 — §3.2 schema-descriptor additions (BLOCKING)
**Source:** ddd-specialist Finding 1 + Finding 4.

The §3.2 scope currently only edits the `:EntryPoint.kind` descriptor at `nodes.rs:296`. Extend scope to ALSO add:

a. A homonym-disambiguation sentence on the `:EntryPoint.kind` descriptor: *"Note: `kind=\"test\"` on `:EntryPoint` is orthogonal to `:Item.is_test`. The former classifies the **entry surface** (this fn is an invocation root for the test runner). The latter classifies the **item's compile scope** (this item lives under `#[cfg(test)]`). A query that needs items reachable only from test entry points should match on `:EntryPoint{kind:\"test\"}`-reachability, NOT on `:Item.is_test=true`."*

b. Two new `:Item` attribute entries in `nodes.rs` (paralleling the existing `reachable_from_entry` / `reachable_entry_count` if they are described there, or adding both pairs if they are not):
   - `reachable_from_production_entry: bool` — true iff item is reachable via `CALLS*` from at least one `:EntryPoint` whose `kind ∉ {test, bench}`. Provenance: `EnrichReachability` (dual-BFS pass 2).
   - `reachable_production_entry_count: i64` — count of distinct production `:EntryPoint` nodes that reach the item via `CALLS*`. Provenance: `EnrichReachability` (dual-BFS pass 2).

### EDIT 3 — §3.1 SRP probe location (BLOCKING)
**Source:** solid-architect CR1.

Move `has_test_attr` and `has_bench_attr` from `entry_point_emitter/registers_param.rs` into a new sibling `entry_point_emitter/test_bench.rs`. The RFC §3.1 "Probe semantics" paragraph currently directs the probes into `registers_param.rs` — change the destination to `test_bench.rs`. Module-doc on the new file: *"Test and bench attribute classification probes used by `scan_file`'s `SyntaxKind::FN` dispatch. These probes have no REGISTERS_PARAM counterpart — they are pure classification (kept separate from `registers_param.rs` because they change for a different reason: vocabulary evolution vs param-edge wiring evolution)."*

### EDIT 4 — §3.1 `#[cfg(test)]` exclusion clarification (NON-BLOCKING but recommended)
**Source:** rust-systems RS-3.

Add one sentence to §3.1 "Probe semantics" after the existing `cfg(test)` parenthetical: *"The probe reads `attr.meta().path()`, which for `#[cfg(test)]` yields path segment `cfg` (not `test`). The `test` inside `cfg(...)` is a token-tree argument, not a path segment, so the textual probe correctly does not fire on `#[cfg(test)]` fns. A fn carrying BOTH `#[cfg(test)] #[test]` triggers the probe on the `test` attribute and is classified `kind=test`, which is the intended behavior."*

### EDIT 5 — §2 / §3.1 feature-flag scope (NON-BLOCKING but recommended)
**Source:** rust-systems RS-5.

Add one sentence (either to §2 "Ships" or §3.1 opening): *"All new emission (`kind=test`, `kind=bench` via either attribute or file-location detection) requires `--features hir` on extraction, exactly as `kind=mcp_tool` does today. There is no syn-only partial path — `cfdb-hir-extractor` is the sole producer."*

### EDIT 6 — §3.3 third BFS option (NON-BLOCKING, documentation-only)
**Source:** rust-systems RS-6.

In §3.3 "Implementation choice", add a brief note acknowledging the third option (single multi-source BFS with per-visit kind-mask producing both attribute sets in one traversal). One sentence is sufficient: *"A third option — a single multi-source BFS with per-visit kind-mask that accumulates both attribute sets in one traversal — is also viable and would halve the per-item visit cost. Option A is ratified for symmetry with the existing single-filter `enrich_reachability::run` signature (one filter, one pass, one report); the third option can be introduced as a perf optimization later without changing the public attribute schema."*

### EDIT 7 — §3.3 EnrichReport reporting (NON-BLOCKING, correctness)
**Source:** solid-architect D3 dual-dogfood discipline note.

After EDIT 1d above (which already states "The returned `EnrichReport.attrs_written` is the SUM of both passes' writes"), no additional edit is needed. EDIT 1 covers this.

### EDIT 8 — §3.3 CRP comment on classifier query duplication (NON-BLOCKING, hygiene)
**Source:** solid-architect CRP observation.

When implementing slice 042-B, add a one-line header comment in BOTH `classifier-unwired.cypher` and `classifier-unwired-production.cypher` naming the sibling and the single point of divergence:
```
// SIBLING: classifier-unwired-production.cypher — differs only on attribute read (reachable_from_entry vs reachable_from_production_entry).
// SIBLING: classifier-unwired.cypher — differs only on attribute read (reachable_from_production_entry vs reachable_from_entry).
```
This is implementation guidance (042-B PR), not RFC text edit. RFC §3.3 last paragraph may add: *"Implementer: header comments in both .cypher files name the sibling and the single point of divergence — both files must be edited together if the WHERE clause changes."*

---

## 4. Tests prescription — synthesized from all four lenses

Each slice's 4-row block, synthesized to the strongest convergent form. The RFC author applies these verbatim to §7 once Round 2 ratifies the consolidated text.

### Slice 042-A — extractor `:EntryPoint{kind=test|bench}` emission + fixture

**Unit** *(rust-systems RS-D2 prescription, all four lenses agree)*:
- Synthetic `ast::Fn` inputs constructed via `ra_ap_syntax::SourceFile::parse(src, Edition::Edition2021)`.
- Ten cases covering: `#[test]`, `#[tokio::test]`, `#[async_std::test]`, `#[given]`, `#[when]`, `#[then]`, `#[bench]`, `#[tool]` (precedence non-interference), `#[cfg(test)]` (must NOT trigger), bare fn no attr.
- File: `crates/cfdb-hir-extractor/src/entry_point_emitter/test_bench.rs` `#[cfg(test)] mod tests`.
- Plus the §3.4 synthetic-workspace fixture asserting `(kind, EXPOSES.target.qname)` per row.

**Self dogfood (cfdb on cfdb)** *(ddd-specialist tightening + solid-architect grep guidance)*:
```bash
GREP_COUNT=$(rg -c '#\[test\]|#\[tokio::test\]|#\[given\]|#\[when\]|#\[then\]' \
  --include='*.rs' crates/ | awk -F: '{s+=$2} END {print s}')
cfdb extract --workspace . --features hir --db .cfdb/db --keyspace cfdb-self
QUERY_COUNT=$(cfdb query 'MATCH (e:EntryPoint{kind:"test"}) RETURN count(e)' ...)
[ "$QUERY_COUNT" -ge "$GREP_COUNT" ]
```
Assertion: emitted count ≥ grep count (file-location detection may emit MORE, never less).

**Cross dogfood (cfdb on graph-specs-rust at pinned SHA `913f06f`)** *(all four lenses verified)*:
Run `ci/cross-dogfood.sh`. Zero new rows on any of the four existing `.cfdb/queries/*.cypher` rules (`arch-ban-unwrap-domain-ports`, `arch-context-no-application-in-domain`, `arch-context-no-cross-layer-unwrap`, `arch-context-no-syn-in-domain`). All four match on `cs.callee_path` / `caller.crate` patterns — none reads `:EntryPoint.kind` or `reachable_from_*` attrs. Cross-dogfood is a verified no-op regression per RFC §4 SchemaVersion stability invariant.

**Target dogfood (qbot-core at pinned SHA)** *(consensus + ddd-specialist's spot-audit addition)*:
Report in PR body:
- `MATCH (e:EntryPoint) WHERE e.kind IN ["test","bench"] RETURN e.kind, count(e)` total.
- First 10 emitted `kind=test` qnames as a sample.
- Spot-audit confirmation that `JupiterCryptoBroker::new` (RFC §1 canonical example) is now reached by at least one `:EntryPoint{kind:"test"}`.

### Slice 042-B — scope `--production-only` + dual-BFS + classifier rule

**Unit** *(rust-systems + solid-architect agreement)*:
- Test `reachability::run` with a synthetic `KeyspaceState` containing two `:EntryPoint` nodes (one `kind=mcp_tool`, one `kind=test`), each EXPOSES-targeting a distinct `:Item`.
- Call with `ReachabilityFilter::All` → both items have `reachable_from_entry=true`.
- Call with `ReachabilityFilter::ProductionOnly` → only the mcp_tool-exposed item has `reachable_from_production_entry=true`; the test-exposed item has `reachable_from_production_entry=false`.
- Assert determinism: two sequential runs produce byte-identical attribute writes.

**Self dogfood (cfdb on cfdb)** *(consensus)*:
```bash
cfdb scope --context cfdb-extract --db .cfdb/db --keyspace cfdb-self --format json > default.json
cfdb scope --context cfdb-extract --db .cfdb/db --keyspace cfdb-self --production-only --format json > prod.json
default_unwired=$(jq '.findings_by_class.unwired | length' default.json)
prod_unwired=$(jq '.findings_by_class.unwired | length' prod.json)
[ "$prod_unwired" -gt "$default_unwired" ]
```
Plus: assert that at least one `:Item` in cfdb-self keyspace has `reachable_from_production_entry` as a populated attribute (proves the dual-BFS actually ran).

**Cross dogfood (cfdb on graph-specs-rust at pinned SHA)** *(all four lenses verified)*:
Run `cfdb scope --production-only` on graph-specs-rust keyspace. Zero exit code; zero row change on existing four queries (`--production-only` is opt-in and no existing query reads the new attribute). Default-mode `unwired` count on graph-specs-rust unchanged from pre-RFC-042 baseline (determinism of all-kinds BFS).

**Target dogfood (qbot-core at pinned SHA)** *(consensus + ddd-specialist spot-audit + solid-architect target metric)*:
Report in PR body:
- `cfdb scope --context trading` `unwired` count (default mode) — expected ≥30% drop from 2057.
- `cfdb scope --context trading --production-only` `unwired` count — expected to remain near 2057.
- Diff table showing the deltas.
- Spot-audit of ≥5 items reclassified from "unwired (default)" to "reached-from-test" — operator confidence that reclassification is genuine, not file-location-helper false-positive.

### Slice 042-C — empirical close-out on qbot-core

**Tests:** `none — rationale: cross-repo empirical report, not code.` (Per RFC §7. All four lenses agree.)

---

## 5. Graph-specs-rust update against real code — synthesized D4 proposal

Four lenses proposed four overlapping rules; the natural synthesis is ONE base rule expressing the common detection logic, with documented lens-specific narrowing comments. The rule below is the merger of clean-arch's `arch-domain-only-reached-from-tests.cypher`, ddd-specialist's `vocab-domain-reachable-only-from-tests.cypher`, solid-architect's `arch-ban-test-only-trait-impls.cypher`, and rust-systems' `rust-systems-test-only-reachable-non-test-items.cypher`.

**Proposed filename:** `.cfdb/queries/arch-test-only-reachable-production-items.cypher`

```cypher
// arch-test-only-reachable-production-items.cypher
//
// Rule: production-classified items reachable only from test entry points
// indicate a layer-purity, vocabulary, or abstraction-leakage smell.
//
// Lens consensus (RFC-042 council 2026-05-17):
//
//   clean-arch:       domain items reached only from tests = misplaced
//                     layer marker (test helper masquerading as domain).
//                     Narrow with: AND i.crate =~ 'domain.*'.
//   ddd-specialist:   pub fn / method reachable only from tests = vocabulary
//                     concept with no production exerciser (anaemic).
//                     Narrow with: AND i.bounded_context = $context.
//   solid-architect:  port trait reachable only from tests = ISP violation
//                     (interface with no production user).
//                     Narrow with: AND i.kind = 'trait' AND i.crate =~ 'ports.*'.
//   rust-systems:     non-test fn/method reachable only from tests = dead
//                     production code (and a vtable cost candidate if dyn-
//                     dispatched). The base rule below.
//
// Inputs (require RFC-042 to have landed):
//   :Item.reachable_from_entry             (bool) — existing attr.
//   :Item.reachable_from_production_entry  (bool) — written by RFC-042 §3.3 dual-BFS.
//   :Item.is_test                          (bool) — existing attr.
//
// Pre-RFC-042 keyspace behavior: `reachable_from_production_entry` is absent;
// the WHERE clause produces zero rows safely (absent attr does not match `= false`).
//
// Expected on graph-specs-rust at pinned SHA: zero rows (small, well-wired tree).
// Intent: zero-violation policy from day one. Any future drift surfaces immediately.

MATCH (i:Item)
WHERE i.kind IN ['fn', 'method', 'trait']
  AND i.reachable_from_entry = true
  AND i.reachable_from_production_entry = false
  AND i.is_test = false
RETURN i.qname AS qname,
       i.kind AS kind,
       i.crate AS crate,
       i.bounded_context AS bounded_context,
       i.file AS file,
       i.line AS line
ORDER BY crate ASC, qname ASC
```

**Filed at:** `.cfdb/queries/arch-test-only-reachable-production-items.cypher` on `yg/graph-specs-rust`.

**Citation against current pinned SHA `913f06f`:** Lens citations all converge on a small set of files:
- `domain/src/diff.rs:23 pub fn diff(...)` (clean-arch + ddd-specialist).
- `domain/src/tokens.rs:25 tokenise_target`, `domain/src/context.rs:197 detect_import_cycle` (ddd-specialist).
- `ports/src/lib.rs:15 pub trait Reader`, `ports/src/lib.rs:29 pub trait ContextReader` (solid-architect + ddd-specialist).

Whether any of these fire depends on whether production wiring (`application/src/main.rs`) reaches each via a CLI entry point. Expected on a clean tree: zero rows (graph-specs-rust's domain is small and well-wired). Any non-zero finding is informative and cleanup-actionable.

**Intent:** zero-violation policy from day one. Three of four lenses (clean-arch, solid-architect, rust-systems) explicitly recommend zero-violation; ddd-specialist proposed cleanup-driving but acknowledged it could equally be policy if the initial extract is clean. Convener synthesizes on zero-violation because:
- graph-specs-rust's domain is small enough that an initial clean extract is plausible.
- If the rule fires on initial extract, the count will be small (a handful) and each finding is independently actionable.
- A zero-violation rule prevents drift; a cleanup-driving rule merely catalogs it.

**Follow-up PR plan:** after RFC-042 implementation slices 042-A and 042-B land on `yg/cfdb develop`, file a PR against `yg/graph-specs-rust` that:
1. Adds the rule file at `.cfdb/queries/arch-test-only-reachable-production-items.cypher`.
2. Re-extracts graph-specs-rust against the new cfdb (HEAD post-042-B) and confirms zero rows on the rule (OR catalogs the small initial finding set with operator-confirmed disposition per finding).
3. Wires the rule into `ci/cross-dogfood.sh` so future cfdb PRs continue to pass on graph-specs-rust.

The PR is the **target dogfood for the council itself** — the council does not just ratify cfdb's RFC, it also produces a real-code graph-specs-rust contribution that exercises the new vocabulary on the companion at the pinned SHA.

---

## 6. Round 2 plan

1. **RFC author (captain) applies EDITs 1-8** to `docs/RFC-042-test-bench-entry-points.md` and commits.
2. **Convener re-spawns the council** with a Round-2 brief pointing at the updated RFC and the §2 synthesis verdict (PetgraphStore-internal dual-BFS). Each lens reviews:
   - clean-arch — does Position B resolve the port-purity concern?
   - ddd-specialist — are EDITs 2a and 2b sufficient on the descriptor extensions?
   - solid-architect — does the §3.1 split into `test_bench.rs` resolve CR1? Does EDIT 1 resolve CR2?
   - rust-systems — does EDIT 1's "Trait surface impact" subsection resolve RS-1?
3. If all four lenses RATIFY at R2, convener writes `council/RFC-042/RATIFIED.md`, edits RFC §5.1-5.4 to record verdicts, files slice issues 042-A / 042-B / 042-C per the §7 decomposition + §4 above (Tests prescriptions), and notes the graph-specs-rust follow-up PR plan from §5 above.
4. If any lens requests further changes at R2, iterate.

---

## 7. Out-of-band acknowledgements

- **CRP query duplication (solid-architect non-blocking).** Hygiene comment in both `.cypher` files; implementation-time, not RFC-text.
- **Third BFS option (rust-systems RS-6 non-blocking).** Mentioned in EDIT 6; future-proofing.
- **`#[cfg(test)]` clarification (rust-systems RS-3 non-blocking).** Documentation polish in EDIT 4.
- **Feature-flag scope (rust-systems RS-5 non-blocking).** Documentation polish in EDIT 5.

End of synthesis.
