# RFC-043 verdict — ddd-specialist (Round 2)

**Verdict:** RATIFY
**Author:** ddd-specialist sub-agent (R2-b)
**Date:** 2026-05-18
**R1 verdict:** REQUEST CHANGES (C1, C2, C3)
**R2 disposition:** C1 addressed by §4 I6. C2 non-blocking per R1 and the VfsPath filter is real code at a named location (call_site_emitter.rs:113-114). C3 moot — `proc_macro_status` metadata was cut entirely (YAGNI), removing the discoverability gap. One new R2 concern raised (third epistemic state from silent probe fallback) but found non-blocking by design.

---

## D1. Verdict on the RFC as written

### R1 C1 — Homonym on `callee_resolved=true` (ADDRESSED)

v2 §4 I6 adds the descriptor caveat invariant: the `:CallSite.callee_resolved` descriptor at `crates/cfdb-core/src/schema/describe/nodes.rs` gains a sentence noting the epistemic-precision shift, and §2 Scope names the descriptor edit as a deliverable in 043-A.

The YAGNI cut of `proc_macro_status` changes the shape of the mitigation: the R1 C1 change request asked for a descriptor noting that `callee_resolved=true` precision co-varies with `proc_macro_status`. Since `proc_macro_status` no longer exists, the descriptor instead states "consumers wishing to disambiguate pre/post-RFC-043 keyspaces must re-extract — there is no per-keyspace status flag, by design" (§4 I6). This is the correct substitute: it makes the epistemic limitation explicit at the vocabulary level without introducing a cut concept. The homonym within a single keyspace is resolved (all nodes in one extract share the same probe outcome). Cross-keyspace federation remains a consumer discipline concern — the descriptor acknowledges it. C1 ADDRESSED.

### R1 C2 — Phantom call-site protection invariant for `#[async_trait]` (NON-BLOCKING, NOT ADDED AS INVARIANT)

The SYNTHESIS-R1 convener classified C2 as "no action needed (VfsPath::Virtual filter is protective by current code; documented in §5.2)." v2 §3.6 names `call_site_emitter.rs:113-114` as the canonicalization filter and notes the risk surface (macro-introduced attributes that bypass it). No new §4 invariant was added for phantom call-site protection.

The DDD lens accepts this disposition. C2 was marked non-blocking in R1. The filter is real code at `crates/cfdb-hir-extractor/src/call_site_emitter.rs:113-114`. The protection mechanism is now named in the RFC (§3.6), which satisfies the documentation obligation the R1 finding raised. The suggested I8 invariant (assert zero `:CallSite` nodes with non-existent `file` paths) is carried forward as a test prescription below (D2 Unit row) rather than as an RFC invariant — this is the appropriate level of enforcement: the invariant belongs in the test, not in the RFC's schema vocabulary section.

The question of whether `ra_ap_vfs` could ever change VfsPath::Virtual to a temp-file-backed path is a Rust-systems concern deferred to that lens.

### R1 C3 — `schema-describe` should expose `proc_macro_status` (MOOT)

`proc_macro_status` was cut entirely. There is no metadata to expose and no discoverability gap to close. MOOT per SYNTHESIS-R1.md.

### New R2 concern — third epistemic state from silent probe fallback

When `proc_macro_server_available()` returns false, v2 §3.3 case 1 falls back silently to `ProcMacroServerChoice::None` with a stderr warning. The keyspace produced is identical in schema to a `--no-proc-macro` keyspace or a pre-043 keyspace. There is no machine-readable signal that the probe fired false — the stderr warning is the only operator-visible indicator.

This creates a third epistemic state for `callee_resolved=true`:
- State A: proc-macros enabled, probe returned true — high-recall resolution.
- State B: `--no-proc-macro` explicitly passed — pre-043 semantics, operator-intentional.
- State C: proc-macros requested, probe returned false, silent fallback — pre-043 semantics, probe-triggered.

States B and C produce identical keyspaces but differ in intent. A consumer cannot distinguish them from the keyspace alone.

**Assessment: non-blocking.** The RFC explicitly documents this design at §3.3 case 1, §6 non-goals, and §4 I6 (the descriptor acknowledges "no per-keyspace status flag, by design"). The third state is accepted by design; the user-directed YAGNI directive makes restoring `proc_macro_status` out of scope. The stderr warning is adequate for the named consumer (CI operators who see logs). If a future RFC names a consumer that needs machine-readable probe-outcome signalling, the `proc_macro_status` concept is ready to be re-introduced with a concrete motivation.

One implementer note (not a blocking concern): the §4 I6 descriptor sentence should explicitly mention that silent probe fallback produces a keyspace indistinguishable from `--no-proc-macro`, so operators reading `cfdb schema-describe` understand why two keyspaces with identical `callee_resolved` distributions might have different recall.

---

## D2. Tests prescription for slice 043-A

```
Tests:
  - Unit:
      (a) build_hir_database with proc_macros=true and a stub sysroot that has
          rust-analyzer-proc-macro-srv present: assert ProcMacroClient is returned as
          Some and LoadCargoConfig.with_proc_macro_server == Sysroot and
          proc_macro_processes == 1.
      (b) build_hir_database with proc_macros=true and a tmpdir-stubbed sysroot
          missing the binary: assert fallback to ProcMacroServerChoice::None,
          proc_macro_processes == 0, stderr emits the warning naming the missing
          binary path (§3.3 case 1).
      (c) build_hir_database with proc_macros=false: assert ProcMacroServerChoice::None
          regardless of sysroot state.
      (d) CLI --no-proc-macro flag parses to proc_macros=false; absence parses to true.
      (e) VfsPath exclusion: for at least one #[async_trait]-rewritten call site in
          cfdb-self's HIR walk, assert that no :CallSite node has a `file` attribute
          naming a path absent from the workspace on disk. (Verifies the VfsPath::Virtual
          filter at call_site_emitter.rs:113-114 holds under proc-macro expansion.)
      (f) Descriptor invariant: cfdb schema-describe output for :CallSite.callee_resolved
          contains the I6-prescribed sentence about epistemic-precision shift and the
          absence of a per-keyspace status flag.

  - Self dogfood (cfdb on cfdb):
      Run `cfdb extract --workspace . --hir` with post-RFC-043 binary on a sysroot with
      rust-analyzer-proc-macro-srv installed. Assert:
      (i)  MATCH (cs:CallSite {callee_resolved: true, resolver: "hir"}) RETURN count(cs)
           is strictly greater than the pre-043 baseline count.
      (ii) PR body names at least 3 concrete callee_qnames that flip from
           callee_resolved=false to callee_resolved=true (candidates at
           crates/cfdb-hir-extractor/src/call_site_emitter.rs, crates/cfdb-petgraph/src/eval/).
      (iii) ci/determinism-check.sh --hir mode exits 0: two extracts of cfdb-self produce
            sha256-identical keyspace JSON (G1 holds on proc-macro path).

  - Cross dogfood (cfdb on graph-specs-rust @ pinned SHA):
      ci/cross-dogfood.sh exits 0 with the post-RFC-043 binary. All existing
      .cfdb/queries/*.cypher produce zero new rows. No SchemaVersion bump (§4 I4)
      means no cross-fixture pin bump required (§4 I5). Report in PR body whether
      proc-macro expansion was available on CI or fell back (so reviewer knows which
      epistemic state the cross-dogfood ran under).

  - Target dogfood (qbot-core @ pinned SHA):
      cfdb scope --context trading unwired count after 043-A extract. Number must appear
      in PR body. Acceptance: materially below the pre-RFC-043 ceiling of 1534 (concrete
      lower bound prescribed by implementer based on a live pre-merge extract). PR body
      must include a spot audit of at least 5 items that flip from unwired to reached:
      at minimum one from an #[async_trait] context and one from a #[derive(Builder)]
      context. A non-monotonic result (any context showing unwired INCREASING vs
      --no-proc-macro run) must be explained — it is the canary for phantom call sites.
```

---

## D3. Dual-dogfood proof discipline

**Self-dogfood Cypher shape:**

```cypher
-- Pre-043 baseline (run against pre-RFC binary extract):
MATCH (cs:CallSite {callee_resolved: true, resolver: "hir"})
RETURN count(cs) AS pre_resolved_count

-- Post-043 measure (run against post-RFC binary extract):
MATCH (cs:CallSite {callee_resolved: true, resolver: "hir"})
RETURN count(cs) AS post_resolved_count
```

The post count MUST be strictly greater. The PR body names the specific callee_qnames that flipped — no grep proxy suffices because the resolution gain comes from previously-None HIR results becoming Some.

**Descriptor verification:**

```bash
cfdb schema-describe | grep -A5 "callee_resolved"
```

Output must include the I6-prescribed sentence. The exact wording is implementer-determined; the reviewer verifies it names the epistemic-precision shift and the absence of a per-keyspace status flag.

**Cross-dogfood:** `ci/cross-dogfood.sh` against the pinned graph-specs-rust SHA. Expected exit 0. The vocabulary additions in this RFC (descriptor update only, no new node/edge attributes) do not affect any of the four graph-specs-rust `.cfdb/queries/*.cypher` rules.

---

## D4. Determinism risk enumeration

Deferred to rust-systems as the authoritative voice on `ra_ap_load_cargo` internals and macro expansion determinism. DDD contribution: the VfsPath::Virtual filter at `call_site_emitter.rs:113-114` is the primary protection against non-deterministic synthetic-file injection. If a macro produces non-deterministic output in a concrete filesystem file (e.g., via `build_info!()` embedding a timestamp), the §3.5 same-workspace determinism check will catch it. Cross-workspace drift (absolute-path embedding) is documented in §3.6 as a future-RFC concern. No DDD-vocabulary concerns beyond what §3.6 already names.

---

## D5. Wall-clock budget verdict

Deferred to rust-systems. No DDD objection to the §3.4 tiered thresholds (2x pass-with-note, 4x reject). The `--no-proc-macro` escape restores pre-043 semantics and is the correct operator vocabulary for the degraded-recall case.

---

## D6. Failure-mode policy verdict

The §3.3 three-case distinction is DDD-sound:

- **Case 1 (sysroot binary missing):** Silently falls back, stderr warning. Vocabulary concern: the produced keyspace is indistinguishable from a `--no-proc-macro` run or a pre-043 extract. This is accepted by design (YAGNI cut of `proc_macro_status`). The §4 I6 descriptor update should acknowledge this explicitly so operators reading `cfdb schema-describe` understand the silent-fallback scenario.
- **Case 2 (`load_workspace_at` Err):** Hard fail through existing `HirError::LoadWorkspace`. No new vocabulary. Correct.
- **Case 3 (lazy expansion failure during VFS walk):** Individual call sites emit `callee_resolved=false` — same vocabulary as the pre-043 path. Walk continues. The `callee_resolved=false` ratio at end-of-extract is the operator signal. Correct.

No vocabulary ambiguity introduced by the three-case distinction within a single extract run. The homonym concern (C1) is cross-keyspace, not within-run. The §4 I6 mitigation handles the cross-keyspace case at the descriptor level.
