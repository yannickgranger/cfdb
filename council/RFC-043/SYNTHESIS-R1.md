# RFC-043 — SYNTHESIS R1 (convener notes)

**Round 1 verdicts on the v1 (pre-trim) RFC:** archived under `council/RFC-043/v1-pre-trim-verdicts/` (4 lenses returned; 4 REQUEST CHANGES).

**R1 verdict summary:**

| Lens | Verdict | Headline findings |
|---|---|---|
| clean-arch | REQUEST CHANGES | Finding 2 (fallback retry logic should hoist out of `build_hir_database`); Finding 3 (proc_macro_status placement clarification) |
| ddd-specialist | REQUEST CHANGES | Finding 1 (homonym: `callee_resolved=true` semantics shift between pre/post-RFC keyspaces); Finding 2 (VfsPath::Virtual filter protects against phantom poll-call edges — non-blocking) |
| solid-architect | REQUEST CHANGES | SRP violation on `build_hir_database` once fallback + status-tagging both inside it |
| rust-systems | REQUEST CHANGES | **RS-1 (BLOCKING)** lifetime bug: `_proc_macro_client` dropped at function exit; **RS-2 (BLOCKING)** `rust-analyzer-proc-macro-srv` not in stable sysroot; RS-3 (non-blocking) `proc_macro_cwd` determinism risk; RS-4 / RS-5 (non-blocking) feature-flag + numerical consistency notes |

## Convener pre-trim pass (interleaved with R1)

Mid-R1, the convener received feedback from the user directing a **YAGNI + reuse** pass on the v1 RFC before continuing the council. The pre-trim cuts:

| v1 Feature | v2 Status | Rationale |
|---|---|---|
| `ProcMacroPolicy::{Enabled, Disabled}` wrapper enum | Cut → `proc_macros: bool` | Split-brain with upstream `ra_ap_load_cargo::ProcMacroServerChoice`. The upstream enum is the source of truth; a wrapper adds a parallel vocabulary with no value. |
| `--strict-proc-macro` flag | Cut | Speculative escape hatch. No concrete consumer named. |
| `extract.proc_macro_status` keyspace metadata | Cut | Speculative — no concrete consumer reads it. The stderr warning on degraded fallback IS the operator signal. |
| `cfdb schema-describe` extension consuming `proc_macro_status` | Cut | Existed only to expose the cut metadata. Circular justification. |
| Retry-after-`Err` tolerant fallback | Cut | Speculative. The hard-fail message is the signal; `--no-proc-macro` is the escape. |
| `tests/fixtures/proc_macro_determinism/` synthetic fixture | Cut | cfdb-self is macro-heavy enough (`#[derive]`, `#[tokio::test]`) to serve as the determinism corpus. |
| 4 slices (043-A/B/C/D) | Collapsed to 1 vertical slice | Code + dogfood + recall refresh fit in one PR; horizontal splitting was a v1 artifact. |

The trim absorbed the bulk of R1's REQUEST CHANGES:

- clean-arch Finding 2 (fallback hoists out) → moot; fallback removed entirely.
- clean-arch Finding 3 (status metadata placement) → moot; metadata removed entirely.
- solid-architect SRP violation → moot; fallback logic that caused the violation is removed.
- ddd-specialist Finding 2 → no action needed (VfsPath::Virtual filter is protective by current code; documented in §5.2).

## Surviving R1 findings (v2 mitigations)

### ddd-specialist Finding 1 — homonym on `callee_resolved=true`

The cut of `proc_macro_status` metadata makes this finding WORSE in one respect (consumers can't tag-and-filter across keyspaces) but the alternative (restoring the metadata) was rejected as YAGNI. The mitigation in v2:

- **Added §4 I7 → I6** (descriptor caveat invariant): the `:CallSite.callee_resolved` schema descriptor at `crates/cfdb-core/src/schema/describe/nodes.rs` gains a sentence noting the epistemic-precision shift. Consumers federating across keyspaces must re-extract — there is no per-keyspace status flag.
- **Added §2 Scope row:** "Schema descriptor caveat" listed as a deliverable.
- **Added §7.1 Scope row:** the descriptor edit is in slice 043-A.

This is the smallest-possible mitigation: descriptor-only, no schema vocabulary change, no SchemaVersion bump.

### rust-systems RS-1 — `_proc_macro_client` lifetime bug (BLOCKING)

This is a real systems-level correctness bug that the v1 trim did not address. The v2 mitigation:

- **§3.1 design code block** updated: `build_hir_database` returns `(RootDatabase, Vfs, ProcMacroClient)`. The third element MUST outlive the salsa DB's last use; caller stack frame owns it.
- **§4 I7 added:** "`ProcMacroClient` lifetime bounded by extraction scope" invariant.
- **§7.1 Scope row updated:** caller owns the returned `ProcMacroClient` for the lifetime of the extraction walk.
- **§7.1 Tests Unit row updated:** unit test asserts `ProcMacroClient` is returned and held by caller.

This is correctness, not speculation. The wrapper-enum-vs-bool trade off does NOT apply — the lifetime mechanism is a tuple element, not a new abstraction.

### rust-systems RS-2 — `rust-analyzer-proc-macro-srv` not in stable sysroot (BLOCKING)

Stock CI runners that install rustc without the `rust-analyzer` rustup component lack the binary. v1's hard-fail policy would break CI on every PR in that scenario. v2 mitigation:

- **§3.1 design code block** updated: `proc_macro_server_available()` probe gates `ProcMacroServerChoice::Sysroot`. Probe-true → Sysroot; probe-false → None + stderr warning.
- **§3.3 case 1** added: "Sysroot binary missing" — silent API fallback, loud stderr warning.
- **§2 Scope rows updated:** availability probe is a named deliverable.
- **§5.4 lens text** acknowledges RS-2 inline.
- **§7.1 Tests Unit row updated:** unit asserts probe returns true on a real sysroot and false on a tmpdir-stubbed empty sysroot.

This availability fallback IS a tolerant-fallback mode — but its scope is narrow (one specific availability case) and there's a named concrete consumer (CI on stock rustc). The bar for resurrecting a cut feature is "name the consumer that breaks without it" (per the user's YAGNI directive). RS-2 satisfies that bar.

The other tolerant-fallback mode (retry-after-`Err` from `load_workspace_at`) remains cut — no concrete consumer.

### rust-systems RS-3 — `proc_macro_cwd` determinism (non-blocking)

Documented in v2 §3.6 for future-RFC awareness. No design change in 043-A. The §3.5 same-workspace determinism gate covers regression; cross-workspace drift is a future concern.

### rust-systems RS-4 / RS-5 — non-blocking notes

RS-4 (feature-flag-vs-runtime-flag alongside existing `hir` feature gate): the v1 RFC was correct in choosing runtime flag (no feature gate fragmentation). v2 retains this. Council R2 may revisit if the lens has stronger arguments.

RS-5 (`proc_macro_processes = 0` with `Sysroot` is inconsistent but harmless): style nit, easy to handle at implementation time.

## v2 RFC delta summary

Total v1→v2 diff:
- `docs/RFC-043-hir-proc-macro-server.md`: ~400 lines → ~280 lines (after trim, plus +60 lines for RS-1/RS-2/RS-3 mitigations and §5.4 acknowledgements).
- `council/RFC-043/BRIEF.md`: updated to mention the pre-trim pass and the surviving R1 findings.

## Council R2 scope

The R2 council deliberates on the **v2 RFC**, NOT the v1. R2 lenses:
- Re-read the (updated) RFC v2 + BRIEF v2.
- Cite R1 findings where they want to push back on a v2 mitigation OR raise a NEW concern that v1 didn't catch.
- Render fresh verdicts. The convener targets 4/4 RATIFY in R2.

R1 verdicts are evidence of the trim's principled basis. R2 verdicts are the actual ratification gate.
