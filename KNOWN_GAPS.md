# cfdb recall — KNOWN GAPS

This file documents the `cfdb-extractor` public-API recall gap against
`rustdoc-json` ground truth, per RFC-029 §13 acceptance gate Item 2.

Generated manually or by invoking
`cargo run --release -p cfdb-recall -- --workspace .concept-graph/cfdb
--crate <name> --gaps-file .concept-graph/cfdb/KNOWN_GAPS.md` from the
qbot-core worktree root.

## How the gate works

`cfdb-recall` compares the set of items `cfdb-extractor` emits for a
crate (via `syn` AST traversal) against the set of public items
`rustdoc --output-format=json` reports for the same crate. Recall is
`|extracted ∩ public| / |public|`, computed **as a set** over the
shared `PublicItem` qname — see `crates/cfdb-recall/src/lib.rs` for the
pure formula.

**Gate threshold:** 95% per crate. Defined as the `DEFAULT_THRESHOLD`
const in `cfdb-recall/src/lib.rs`; raising it requires a reviewed PR
against that constant per CLAUDE.md §6 rule 8 (no metric ratchet files).

## What the gate measures

**Top-level items only:**
`fn`, `struct`, `enum`, `trait`, `type`, `const`, `static`, `union`.

**Deliberately NOT measured in v0.1:**

- Impl methods (`kind="method"`). rustdoc `Crate::paths` does not
  index methods directly — they live under `Impl` items in
  `Crate::index` and require a secondary walk. Cfdb extractor DOES
  emit methods via `visit_impl_item_fn`, so the extractor adapter
  drops them for symmetry. Deferred to v0.2.
- Fields and enum variants. cfdb tracks these under `Label::FIELD` /
  `Label::VARIANT`; measuring them in the recall gate needs matching
  adapters on both sides. Deferred to v0.2 with `ra-ap-hir`.
- Trait method signatures inside `trait { ... }` bodies. cfdb's
  `visit_item_trait` emits the trait itself but does not recurse into
  method sigs. Deferred to v0.2.
- Modules, impls, macros, proc-macro attributes.

## Current state — 2026-04-14

| Crate          | Total public | Audited | Matched | Missing | Recall  | Verdict |
| :------------- | -----------: | ------: | ------: | ------: | ------: | :------ |
| `cfdb-core`    |           38 |       0 |      38 |       0 | 100.00% | PASS    |

`cfdb-core` is the harness's dogfood target — the integration test at
`crates/cfdb-recall/tests/integration_recall.rs` runs the full pipeline
(real `cfdb-extractor` + real `rustdoc-json` + real `rustdoc-types`
parser) against it on every `cargo test -p cfdb-recall`. The 100% recall
on its current shape is a property of the crate's structure, not a
floor the gate enforces — the 95% threshold is what holds the
CI-visible acceptance line.

Other library crates in the cfdb sub-workspace (`cfdb-query`,
`cfdb-petgraph`, `cfdb-extractor`, `cfdb-recall` itself) have not yet
been measured. Running `cfdb-recall --crate cfdb-query --crate
cfdb-petgraph …` is a follow-on; `cfdb-cli` is a binary and has no
public-API surface to measure.

## Audit list

See `crates/cfdb-recall/recall-audit.txt` for the format. The list is
currently empty — no crate has yet tripped a real gap that needed a
macro-generated carve-out. Entries will be added (with a `#` comment
explaining why) as sibling Phase B issues wire the remaining cfdb
workspace crates into the gate.
