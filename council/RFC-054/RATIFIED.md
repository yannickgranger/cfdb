# RATIFIED — RFC-054 (target-scoped `:Item` identity + ingest-contention diagnostics)

**Council:** 2026-07-31, agent-team (4 lens teammates, mailbox + shared task list), `CLAUDE.md §2.3`. Base `origin/develop@45b1ee9`, worktree branch `docs/rfc-054-target-identity` (started as `fix/542-qname-collision`).
**Outcome:** **RATIFIED ×4** (R1: 4× REQUEST CHANGES → amendments folded in R2/R2.1 → R2 unanimous RATIFY, each seat re-verifying its own amendments against source before confirming).

## Verdicts

| Lens | R1 | R2 (after amendments) |
|---|---|---|
| clean-arch | REQUEST CHANGES (findings A-D) | RATIFY |
| ddd-specialist | REQUEST CHANGES (findings 1-3) | RATIFY |
| solid-architect | REQUEST CHANGES (2 findings + 1 R1 follow-up) | RATIFY |
| rust-systems | REQUEST CHANGES (2 findings + 1 upgrade) | RATIFY |

No lens dissented on the core design at any point: suffix-position target discriminator on ids
(`item:{qname}#bin:{name}`), display qname unchanged, producer-agnostic contention warning —
all four lenses independently found the §3.1/§3.4 core sound in R1.

## What the council established

1. **False upstream-API claim caught before implementation (rust-systems Finding 1 +
   clean-arch Finding D, independent convergence).** The R1 draft's "`ra_ap` exposes bin vs
   lib on the crate data" is false for the vendored ra_ap 0.0.328: `CrateData`/`ExtraCrateData`/
   `hir::Crate` carry no target kind; `TargetData.kind` collapses into `is_proc_macro` at
   `workspace.rs:1662` and is discarded; `CrateOrigin::is_lib()` is a false friend
   ("non-member dep", not `[lib]`). For same-named bins (default `src/main.rs`, `cfdb-recall`'s
   bin) `origin` and `display_name` are byte-identical between lib and bin crate inputs — zero
   signal. Remediation ratified: `build_hir_database` **public-signature change** — two-step
   `ProjectWorkspace::load` + `load_workspace` replacing one-shot `load_workspace_at`, retained
   `CargoWorkspace`, `root_file → Vfs::file_path → VfsPath::as_path` correlation against
   `TargetData.{kind,name,root}` (all pub, verified at vendored source).
2. **Consumer-side id reconstruction, a three-lens convergent class.** Code that rebuilds
   `item:` ids from bare qnames breaks silently under discriminated identity: post-walk
   resolvers (`resolver.rs:107/173/257` for RETURNS/TYPE_OF/MATCHES_ON, keyed by
   `emitted_item_qnames: BTreeSet<String>` which cannot represent N target-scoped claims),
   the serde-default enrich pass (`attr_call_resolution.rs`, misses silent by design), and —
   sharpest — the dangle is **invisible by contract**: `synthesize_referenced_items` checks
   bare-qname membership, which is true (the item exists under a different id), so the
   compensating-stub pass correctly declines. Ratified: qname → identities map + target-aware
   `by_last_segment` index as named design surface; resolution policy same-target → lib →
   never-a-foreign-bin (mirrors rustc visibility, confirmed by ddd as the correct bounded-context
   reading, not a heuristic), residual ambiguity skips + warns.
3. **Identity/display conflation is prop-side, not edge-side (solid-architect, refuting the
   team lead's own extension).** Edge endpoints round-trip discriminated ids correctly
   (prefix-strip/suffix algebra is self-correcting); the real defect is the recovered string
   double-used as display props (`caller_qname`, `parent_qname` on `:CallSite`/`:Param`/`:Field`/
   `:Variant` would leak `#bin:{name}`). Ratified: compiler-enforced `(id, display_qname)`
   tuple returns at the three emit owners + suffix-aware `display_qname_from_node_id` in the
   `cfdb-core::qname` shared kernel (ddd: syn emit, extractor synthesize, and the HIR adapter
   are Conformist consumers of one primitive — 9 call sites audited, per-producer patches
   cannot close the class). `callsite:` ids centralized (`callsite_node_id`) — they never were.
4. **Diagnostic surface corrections (clean-arch + ddd, convergent).** `Warning` has no `code`
   field; `WarningKind::EmptyResult` already does double duty (empty result + dropped edge) —
   ratified: new `WarningKind::IdentityContention` variant. `cfdb extract` never reads
   `ingest_warnings` (only `execute()` does) — ratified: inherent
   `PetgraphStore::ingest_warnings()` off the trait, RFC-035 §4 `execute_explained` precedent.
5. **Blast radius into shipped rules (ddd).** `.cfdb/queries/hsb-cluster.cypher:92` dedups
   with `a.qname < b.qname` — structurally silent on the same-qname population this RFC makes
   visible. Tie-break amendment rides in 54-B (rule edits are RFC-gated; develop-parity proven
   before the new-pair fixture lands).
6. **Cleared by verification:** `#` separator safe (node ids are write-only strings
   workspace-wide — full grep, zero parsers); cargo target-model edge cases dispositioned
   (default bins, cross-package duplicate bin names, required-features bins pre-existing,
   autobins resolved by cargo); determinism unchanged (replace semantics pre-existing);
   perf noise (bin items are a small minority; one map build per HIR load).
   **SchemaVersion V0_7_0 → V0_8_0: unanimous 4/4** (V0_6_0 `crate_tier` precedent + wire
   identity change observable by `cfdb diff`/`impact`); graph-specs lockstep PR required.

## Process notes

- Every load-bearing R1 claim was re-verified by the team lead against source before folding;
  one lead-added extension (RETURNS src-dangle) was refuted by the authoring seat with the
  algebraic argument — the challenge/refute loop worked in both directions.
- R2 confirmations were authority-scoped: each seat re-verified the fold of its own amendments
  (rust-systems re-read the vendored ra_ap citations line-by-line; solid-architect re-checked
  the three emit-site line numbers and the hsb-cluster predicate; clean-arch re-verified the
  `result.rs` range and the RFC-035 precedent; ddd re-traced the synthesize.rs chain).
- Slice decomposition with prescribed `Tests:` blocks lives in RFC §7 (54-A diagnostic →
  54-B syn+core → 54-C HIR alignment); the one-shared-fixture discipline (id-discrimination +
  prop-bareness on the same fixture) is deliberate test design against the conflation bug shape.
