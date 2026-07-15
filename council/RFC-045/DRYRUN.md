# RFC-045 — Coder dry-run synthesis (between Council R1 and R2)

Two coder sub-agents compiled scratch implementations (PHP: 45-A/45-C; TS: 45-B/45-D) against the real producers + grammars. Both verdicts: **NOT implementable as written**. Findings + resolutions folded into the CANDIDATE-R2.

## Blockers found by building (not by review)
1. **`target_resolved` is unqueryable** (PHP, compiled): `cfdb-petgraph/src/graph.rs:227-237` drops any edge with an unknown `dst`; PHP/TS path runs no synthesize. → **Resolution:** two-pass, emit `IMPLEMENTS` *iff* target is an in-workspace `:Item`; external → no edge (closed-world, documented); `target_resolved` dropped. Keeps §4.4 intact.
2. **TS has no method-level `:Item`** (TS, `emit.rs`): classes are one `:Item`, methods invisible — nothing to anchor `INVOKES_AT`/`caller_qname`. → **Resolution:** new prerequisite slice **45-D0** (TS method `:Item` + `::Class::method` qname). 45-D is NOT a mirror of 45-C.
3. **cfdb-core attr is not churn-free** (both, reproduced test breaks): `FROZEN_NARRATIVE_DIGEST`, `spec_sections_cover_all_schema_labels` (`specs/concepts/cfdb-core.md`), `:CallSite` resolver/kind enum narrative pins all break. → **Resolution:** §4.1 coordinated-edit checklist per slice.
4. **CALLS is near-zero; even Rust emits none** (TS, `emit/mod.rs:96` hardcodes `callee_resolved=false`). → **Resolution:** PHP emits CALLS for in-workspace static callees only; TS emits zero; "callers of X" via `:CallSite.callee_path` + `INVOKES_AT`.

## Contradictions
- **INVOKES_AT direction**: descriptor `from:CALL_SITE,to:ITEM` (`describe/edges.rs:118`) vs emitter Item→CallSite (`emit/mod.rs:104`). → pin Item→CallSite + fix descriptor in 45-C.
- **TS qname `module::Class.method` doesn't exist** (actual `{crate}::{module}::{name}`, no class/method nesting). → defined in 45-D0.

## Enumeration gaps (now tables in §3.4)
- PHP `scoped_call_expression` scope variants: `self`/`static`/`parent`/`$var` (R1 said only "scoped→resolved").
- TS `call_expression.function` supertype shapes: `this`/`super`/optional-chain/chained/IIFE/tagged-template.
- "read through the `type` wrapper" wrong — no `type` node; children are `type_identifier`/`generic_type`/`nested_type_identifier`.

## Secondary
- CallSite full Rust-parity prop set (9 props), not 4.
- CallSite id has no resolver segment → Invariant 5 scoped to single-producer keyspaces.
- `php_construct`/`ts_construct` → documented in the `:Item` descriptor (were undocumented-but-load-bearing).
- inline-vs-common settled: producers' emit APIs textually incompatible → **inline, no common crate** (§8-Q2 escalation withdrawn).

## Confirmed-good by compiling
Grammar walk paths (`class_interface_clause`/`base_clause`; `class_heritage`), recursive walk feasibility, ABI 0.23, additive descriptor compiles, determinism via `sort_key`.
