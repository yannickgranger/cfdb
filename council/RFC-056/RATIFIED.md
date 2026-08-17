# RFC-056 council record — RATIFIED 4/4

RFC: `docs/RFC-056-enrich-port-split.md` — `GraphBackend`: split `cfdb-petgraph`'s enrichment passes behind a port (strangler-fig).

Mechanism: 4-lens agent-team council (`clean-arch`, `ddd-specialist`, `solid-architect`, `rust-systems`), each spawned as a named, backgrounded teammate reviewing the draft independently against source (not against each other's output), verdicts collected by the lead over the mailbox, findings folded, R2 re-confirmation requested from each lens against the revised text.

## Round 1 — 4× REQUEST CHANGES

| Lens | Verdict | Core finding |
|---|---|---|
| clean-arch | REQUEST CHANGES | **Blocking**: §2/§3.4 disagreed on when `cfdb-petgraph`'s old `EnrichBackend` arms are deleted vs. when `cfdb-cli`'s composition root switches — as drafted, slices 056-A–F delete the old arm per-slice while the CLI keeps calling the (now-stubbed) old implementor until 056-G, a live regression window on the shipped `cfdb enrich-<verb>` binary. Non-blocking: dep-rules tripwire completeness; citation line-number drift. |
| ddd-specialist | REQUEST CHANGES | `Direction` reuse decision correct but wrong module (`query::ast`'s doc scopes it to the query grammar only — move to `cfdb_core::schema`, peer of `Label`/`EdgeLabel`); `GraphPort`/`GraphPortStore` introduced a third naming convention alongside the existing `StoreBackend`/`EnrichBackend` suffix — rename. |
| solid-architect | REQUEST CHANGES | Independently verified the §1 git-history claim (true, `comm -12` empty) and the eval/-identical-coupling claim (true). **Blocking**: independently converged on the same composition-root sequencing bug as clean-arch. Also: port-module placement left ambiguous (mandate `cfdb_core::graph`); cfdb-core's pre-existing CRP softness should be named as a deferred non-goal; `cfdb-concepts` missing from the dependency cleanup list. |
| rust-systems | REQUEST CHANGES | **Blocking (compile failure)**: `GraphPortStore` (as drafted) was missing a `Send + Sync` supertrait — `EnrichEngine<'s, S>: Send + Sync` (required by `EnrichBackend: Send + Sync`) is unprovable for unconstrained `S` without it. Also: `metrics/mod.rs:126`'s `FnItem.node_idx` is a second, previously uncited petgraph-coupling site; `cfdb-enrich`'s moved test suites need a concrete `GraphBackend` to run against (no dev-dep named); `attr_call_resolution.rs`'s 5 inline unit tests bypass `PetgraphStore` and need an actual rewrite, not a verbatim move; 056-F's BFS is a genuine allocation-class perf regression (not the same cost class as the other 5 passes) and needs a measured gate; `cfdb-cli`'s feature-forwarding Cargo.toml edits were left to an undifferentiated final cutover instead of named per-slice.

Two lenses (clean-arch, solid-architect) independently found and converged on the same root defect (composition-root cutover sequencing) by reading different evidence (clean-arch: default-stub fallback + CLI dispatcher; solid: `tools/dogfood-enrich` driving the real binary + self-dogfood test files) — treated as one finding, one fix.

## Author fold

All R1 items folded into the RFC in one revision:

- **Composition-root cutover moved per-slice** (§2, §3.4): each of 056-A–F now flips its own verb's `cfdb-cli/src/enrich.rs` dispatch arm to `EnrichEngine` in the same PR that deletes the old `PetgraphStore` arm; 056-G is pure deletion/cleanup with no remaining behavioral cutover.
- **`Direction` relocated** `cfdb_core::query::ast` → `cfdb_core::schema` (peer of `Label`/`EdgeLabel`), re-exported from `query::ast` for zero-diff compat in `eval/pattern/path.rs` (§3.3).
- **Renamed** `GraphPort`/`GraphPortStore` → `GraphView`/`GraphBackend`, matching the `StoreBackend`/`EnrichBackend` suffix convention (§2, §3.1).
- **`GraphBackend: Send + Sync`** added (§3.1) — compile-blocking fix, zero cost at the one real call site.
- **`metrics/mod.rs:126`'s `FnItem.node_idx`** + `clustering.rs:83-93`'s test fixtures named explicitly in 056-E's scope (§1, §3.4, §7).
- **`cfdb-enrich` gains `[dev-dependencies] cfdb-petgraph`** for the moved test suites — precedented (`cfdb-petgraph`'s own `[dev-dependencies] cfdb-query`), exempt from the CLEAN-3 dependency-rule gate (§2, §3.2).
- **`attr_call_resolution.rs`'s inline unit tests** called out as needing an actual rewrite against `&dyn GraphView` in 056-F, not a verbatim move (§3.2, §3.4, §7).
- **056-F gains a named perf exception**: a measured self-dogfood wall-clock/allocation comparison as an explicit acceptance-gate item (not blanket perf deferral, not a new no-ratchet threshold file) (§3.4, §6, §7).
- **`cfdb-cli` feature-forwarding** named explicitly per-slice for 056-D (`git-enrich`) and 056-E (`quality-metrics`/`llvm-cov`), including the stated mid-migration divergence (§2, §3.4).
- **Dep-rules tripwire completeness**: `[cfdb-core]`/`[cfdb-petgraph]` forbidden lists gain `cfdb-enrich`; `cfdb-concepts` added to the cleanup list, dropping from `cfdb-petgraph` once 056-B/C land rather than held to 056-G (§2).
- **cfdb-core's pre-existing CRP softness** (2/12 intra-workspace consumers use any port trait) named as a deferred non-goal, not silently extended (§6).
- **Port-trait module** mandated as `cfdb_core::graph`, not left ambiguous with `enrich.rs` (§3.1) — later corrected in R2 from an initial `graph_port` to `graph` (ddd R2, non-blocking: name the module for the domain noun like its `store`/`enrich` siblings, not for the trait suffix that was itself renamed away from "Port").

## Round 2 — 4/4 RATIFY

| Lens | Verdict | Notes |
|---|---|---|
| clean-arch | RATIFY | Composition-root fix confirmed sound (exactly the requested fix). Sanity-checked all other lenses' folds — none reopen a clean-arch concern; `Direction` relocation actually improves layering. |
| rust-systems | RATIFY | All 5 items confirmed correctly folded. Independently re-read `crates/cfdb-cli/src/enrich.rs` to confirm the per-slice cutover doesn't reopen any dyn-safety/Send+Sync concern — the composition root is a plain `match` over a concrete `PetgraphStore`, never boxed/dyn-dispatched; the bound is checked once, statically, at the `EnrichEngine` impl site. |
| solid-architect | RATIFY | All 4 items confirmed correctly folded, each verified against the exact revised line. Checked other lenses' folds for reopened component-principles concerns — none found; `Direction` relocation improves CCP (changes for the same reason as its new siblings, not query-grammar reasons). |
| ddd-specialist | RATIFY | Both R1 items confirmed correctly folded. **One new, non-blocking R2 finding, folded anyway**: the port-trait module was still named `graph_port` (residual "Port" in the module name after the trait rename removed it from the trait names) — corrected to `cfdb_core::graph`, parallel to `store`/`enrich`. Soft dissent noted, not acted on: "View" conventionally connotes read-only in DB/DDD vocabulary while `GraphView` is read+write; the RFC's own doc comment already self-corrects this. |

**Final: RATIFIED 4/4.** RFC-056 status flipped; §7's 8-slice decomposition (`056-0` through `056-G`) is the concrete backlog per CLAUDE.md §2.4.
