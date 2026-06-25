# RFC-046 — RATIFIED

RFC: `docs/RFC-046-runtime-trace-ingest.md` — runtime execution-trace ingest (`:Trace` + `OBSERVED_CALLS`), issue #477.

## Council outcome

Two rounds of adversarial architect-lens review (read-only sub-agents, all claims verified against `file:line`).

| Lens | R1 | R2 |
|---|---|---|
| clean-arch | REQUEST CHANGES (`&Path` in core port) | **RATIFY** |
| ddd-specialist | REQUEST CHANGES (`resolver="runtime"` homonym) | **RATIFY** |
| solid-architect | REQUEST CHANGES (core trait; `TraceFormat` OCP) | **RATIFY** |
| rust-systems | REQUEST CHANGES (hash; sort key; dispatch payload) | **RATIFY** — conditioned on the I3 reword (reviewer-prescribed), applied |

R1 detail: `council/RFC-046/SYNTHESIS-R1.md`.

## Redesign delta R1 → R2 (what the council changed)

1. **No new cfdb-core trait.** `TraceBackend` dropped; `ingest_trace` is an inherent `PetgraphStore` method (Option C), trace path injected via `with_trace_file` (mirrors `with_workspace` / `execute_explained`). Zero cfdb-core blast-radius; no `&Path` in any port.
2. **No `resolver` overload.** The runtime tier carries no `resolver` prop (it would homonym the static-producer discriminator guarded by `arch-ban-rfc-043-resolver-domain.cypher`). Distinction = the `OBSERVED_CALLS` label + a `profiler` prop.
3. **`TraceFormat` + `FormatParser` in cfdb-petgraph** (`#[non_exhaustive]`) → OCP-clean format extension for RFC-046-D.
4. **Determinism mechanics pinned:** `trace_id` = sha256 (I8); canonical_dump edge key → 4-tuple with `trace_id` tiebreaker (I3, a 46-B deliverable); numeric metrics never in the key; v1 metrics all integer.
5. **Dedicated `dispatch_trace` + `cfdb-cli/src/trace.rs`** (the enrich dispatch is payload-free and cannot carry `trace_file`/`format`).

## Ratification conditions carried into implementation
- 46-A: update the `Provenance::Reserved` test assertion (`crates/cfdb-core/src/schema/describe/tests.rs:248-254`); add `OBSERVED_CALLS` to `ci/edge-liveness.sh` `EDGE_LABELS`; companion `.cfdb/cross-fixture.toml` bump PR for the `V0_6_0` lockstep.
- 46-B: the I3 canonical_dump 4-tuple change + its stable-sort unit test; signature-pin the new public surface; `debug_assert_eq!` edge `profiler` == `:Trace.profiler`.

## Next step
Ratified RFC → file the §7 decomposition (46-A, 46-B, 46-C) as issues linked `Refs: docs/RFC-046-runtime-trace-ingest.md`, carrying the prescribed `Tests:` blocks verbatim (CLAUDE.md §2.4). Not yet filed.
