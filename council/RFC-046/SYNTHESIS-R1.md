# RFC-046 — Council Synthesis R1

RFC: `docs/RFC-046-runtime-trace-ingest.md` (runtime execution-trace ingest, issue #477).
Round 1: four adversarial architect-lens sub-agents, read-only, each verifying claims against `file:line`.

## Verdicts (R1)

| Lens | Verdict | Blocking findings |
|---|---|---|
| clean-arch | REQUEST CHANGES | `&Path` in cfdb-core port (layering violation) |
| ddd-specialist | REQUEST CHANGES | `resolver="runtime"` homonym + live ban-rule collision |
| solid-architect | REQUEST CHANGES | new cfdb-core trait under-justified (Zone-of-Pain); `TraceFormat` OCP |
| rust-systems | REQUEST CHANGES | hash unspecified (DefaultHasher non-portable); sort tiebreaker under-specified; `dispatch_enrich` can't carry payload |

## Blocking findings → R2 resolutions

1. **clean-arch — `&Path` leaks into the dependency-free inner ring.** `EnrichBackend` keeps the port path-free (`enrich.rs:100-191`), injecting `workspace_root` into the concrete `PetgraphStore` via `with_workspace` (`lib.rs:112-114`) at the composition root (`compose.rs:139-155`). The draft's `TraceBackend::ingest_trace(&mut self, &Keyspace, trace_file: &Path, …)` broke this.
   → **R2:** dropped the trait; `ingest_trace` is an inherent `PetgraphStore` method reading `self.trace_file` set via `with_trace_file`. No `&Path` in any signature. (§3.3)

2. **solid — new trait in cfdb-core is unjustified.** cfdb-core: I=0, Ca=11, A≈0.05 (Zone of Pain). One implementor (`PetgraphStore`), one consumer (`cfdb-cli`). Once the `&Path` leak is fixed, the segregation rationale collapses. OCP: `TraceFormat` in core edits the stable crate per format.
   → **R2:** Option C (inherent method, A/B/C table in §3.3); `TraceFormat` + `FormatParser` strategy in cfdb-petgraph; `#[non_exhaustive]`. Rule-of-three: extract a trait at the *second* backend, not speculatively.

3. **ddd — `resolver="runtime"` is a homonym.** Every existing `resolver` value names a *static-analysis producer* (`call_graph.rs:21`, `edges.rs:73-82`); `arch-ban-rfc-043-resolver-domain.cypher:11,53` closes the vocabulary as a drift gate. A profiler observes, it does not resolve.
   → **R2:** `resolver` removed from the runtime tier entirely; `OBSERVED_CALLS` label + `profiler` prop carry the distinction (§3.1/§3.2). `OBSERVED_CALLS` as a separate label (not a flag on `CALLS`) confirmed correct DDD.

4. **rust-systems — determinism & dispatch mechanics.** (a) `trace_id` hash unnamed; `DefaultHasher` seed is non-portable. (b) parallel `OBSERVED_CALLS` collide on the 3-tuple edge sort key (`canonical_dump.rs:74-92`) — tiebreaker mechanism unspecified. (c) `dispatch_enrich` bottlenecks into payload-free `EnrichVerb` (`main_dispatch.rs:226-275`) — cannot carry `trace_file`/`format`.
   → **R2:** I8 sha256; I3 names the 4-tuple `(label, src, dst, trace_id_or_empty)`; §3.5 dedicated `dispatch_trace` + `cfdb-cli/src/trace.rs`.

## Non-blocking, folded into R2
`TraceReport` is a distinct cfdb-petgraph struct (not an `EnrichReport` alias); "floats" wording removed (v1 all-Int); `From<f64>` documented as an accepted gap for 046-D; O(1) frame resolution via `id_to_idx` confirmed; `runner.rs` citation corrected to `main.rs:226-264`; signature-pin added as a 46-B condition; `:Trace`≠`:EntryPoint` and closed-world drop confirmed.

## Status
R1 blocking items resolved in the R2 draft. **Not ratified.** R2 confirmation pass requested on solid-architect (trait→inherent) and ddd-specialist (resolver removal) before RATIFY.
