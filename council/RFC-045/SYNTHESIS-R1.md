# RFC-045 — Council Round 1 synthesis

**Outcome: 4/4 REQUEST CHANGES** (clean-arch, ddd-specialist, solid-architect, rust-systems). All rulings folded into the CANDIDATE (status `CANDIDATE (R1 folded)`). No design-direction reversal — D1-a (direct edge) + D3-a (defer extends) survive; the changes are precision + correctness fixes.

## Blockers (all folded)
1. **DDD** — D1 homonym on `IMPLEMENTS` (Rust impl-block→trait vs PHP/TS class→interface). Fix: `resolver` attribute on the `IMPLEMENTS` edge (mirrors `:CallSite.resolver`). Real kinds corrected (PHP class+iface→`"trait"`+`php_construct`; TS class→`"struct"`, needs new `ts_construct`).
2. **SOLID + clean-arch** — `cfdb-extractor-shared` is syn-only (`classify_arg_kind(&syn::Expr)`); PHP/TS 0% utilization = CRP violation. Fix: inline resolver-stamp per producer + CI `cargo tree --invert syn` guard. (Escalation: a `cfdb-core`-only `cfdb-extractor-common`, only if dry-run shows real duplication.)
3. **rust-systems** — TS `implements` path is `class_declaration→class_heritage→implements_clause→type` (NOT a direct child); verified against tree-sitter-typescript-0.23.2 node-types.json. Unimplementable as drafted.
4. **rust-systems** — `:CallSite` id scheme `cs:{file}:{line}:{col}` + `callee_name` collides with existing `callsite:{caller}:{callee_path}:{idx}` + `callee_path`. Fix: align to existing namespace.

## Changes folded
- Two-pass `target_resolved`/`callee_resolved` (clean-arch) — post-walk pass, producer-local, NOT reusing Rust `synthesize.rs` (which stamps `kind="trait"`).
- D3-a documented as a **known false-negative** (transitive implementors) + negative-assertion test (ddd).
- `LanguageProducer` explicitly NOT extended; emission is an additive `produce()` pass (solid ISP).
- PHP `nullsafe_member_call_expression` added; TS `call_expression` supertype walk strategy specified; recursive body traversal named as the primary work; `new_expression` constructors → non-goal; "RFC-032 cs_id" citation corrected (rust-systems).

## Confirmed-good (no change)
- ABI 0.23: no grammar/feature bump; all node kinds present in pinned grammars (rust-systems R5).
- Determinism via `sort_key`; byte-stable columns (rust-systems R7/R1).
- Composition root (`cfdb-cli/lang.rs`), `cfdb-lang` trait purity untouched (clean-arch).
- D4 per-language qname endpoints: bounded-context-clean, provided no shared qname normalizer unifies `\` vs `::` (ddd).
- D3-a ratified on YAGNI; cfdb-core stability profile unperturbed by the additive `resolver` attr (solid).

## Carried to Council R2 (post coder dry-run)
- `target_resolved` name confirmation; inline-vs-`cfdb-extractor-common` escalation decision (measured by dry-run); validation that the two-pass resolution + the corrected walk paths are implementable; `ts_construct` additive-attr acceptance.
