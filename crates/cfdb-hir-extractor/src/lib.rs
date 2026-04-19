//! `cfdb-hir-extractor` — HIR-backed extractor scaffold (Issue #84).
//!
//! This crate is the Phase B companion to the existing syn-based
//! `cfdb-extractor`. It consumes a `ra_ap_hir::db::HirDatabase` and emits
//! resolved `:CallSite`, `CALLS`, `INVOKES_AT`, and `:EntryPoint` facts
//! into the same `cfdb-core` schema. The two extractors run in parallel
//! and are disambiguated at query time via the `resolver` discriminator
//! property on `:CallSite` (`"syn"` vs `"hir"`, SchemaVersion v0.1.3+
//! per Issue #83).
//!
//! **Slice status — Issue #84 scaffold only.** As shipped by this slice
//! the crate is empty: no public API, no facts emitted. The scaffold
//! proves only that the workspace pins the `ra-ap-*` bundle at the
//! declared `=0.0.328` exact versions and that cfdb-core public
//! signatures remain free of `ra_ap_*` types (architecture gate v0.2-6,
//! asserted by `tests/arch_boundary.rs`). Logic arrives in:
//!
//! - Issue #85 — `build_hir_database` + `extract_call_sites` +
//!   resolved `CALLS` / `INVOKES_AT` + the `cfdb-hir-petgraph-adapter`
//!   crate that isolates ra-ap-* from `cfdb-cli`'s compile tree.
//! - Issue #86 — `:EntryPoint` catalog + `cfdb-cli --features hir`.
//!
//! ## Object-safety constraint (RFC-029 §A1.2 line 87)
//!
//! `ra_ap_hir::db::HirDatabase` is a salsa query database that uses
//! associated types and generic methods — it is explicitly NOT
//! object-safe. Every public function in this crate that accepts the
//! database MUST take it as a monomorphic concrete type or via `impl
//! HirDatabase + Sized`. `dyn HirDatabase` is forbidden and will fail
//! the architecture review on every PR. No exception.
//!
//! ## Boundary contract (RFC-029 §A1.2 line 85)
//!
//! No `ra_ap_*` type ever appears in a `cfdb-core` public signature.
//! The architecture test `tests/arch_boundary.rs` enforces this by
//! scanning `crates/cfdb-core/src/` for the literal token `ra_ap_`
//! and failing on any occurrence. Conversion from HIR types to
//! `cfdb_core::fact::{Node, Edge}` happens inside this crate; the
//! public return type is always the cfdb-core vocabulary.
//!
//! ## No-overlap contract (Issue #84 AC-6)
//!
//! This crate NEVER emits `Label::ITEM`, `Label::CRATE`, or
//! `Label::MODULE` nodes. Those are the exclusive domain of
//! `cfdb-extractor` (syn-based). The exclusion test
//! `tests/exclusion.rs` scans `src/` for the forbidden constant
//! references and fails on any occurrence. Mixing the two breaks the
//! bounded-context split the decomposition of #40 was specifically
//! engineered to preserve.

// Workspace-dep references. The crate does not yet USE the HIR APIs
// (scaffold only), but every declared `ra-ap-*` dep must appear as a
// load-bearing reference so that a future PR which removes one from
// `Cargo.toml` fails to compile rather than silently drifting the
// workspace pin set out of sync with the declared 10+1 bundle. The
// `use … as _;` form brings the crate into the linkage graph without
// introducing any identifier into scope. See the upgrade protocol
// (`docs/ra-ap-upgrade-protocol.md` §2).
use ra_ap_base_db as _;
use ra_ap_hir as _;
use ra_ap_hir_def as _;
use ra_ap_hir_expand as _;
use ra_ap_hir_ty as _;
use ra_ap_ide_db as _;
use ra_ap_proc_macro_api as _;
use ra_ap_project_model as _;
use ra_ap_rustc_type_ir as _;
use ra_ap_syntax as _;
use ra_ap_vfs as _;

// The `emit` module exposes the `CallSiteEmitter` trait and `EmitStats`
// struct — the store-adapter contract. Slice 3b (Issue #92).
pub mod emit;

// Slice 3c (Issue #85c) — the HIR extraction logic.
pub mod call_site_emitter;
pub mod error;
pub mod hir_db;

pub use call_site_emitter::extract_call_sites;
pub use error::HirError;
pub use hir_db::build_hir_database;
