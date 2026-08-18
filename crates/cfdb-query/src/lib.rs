//! cfdb-query — Cypher-subset parser + Rust builder API + AST evaluator helpers.
//!
//! This crate is a thin producer of `cfdb_core::Query` values. Two surfaces:
//! - `parser`: chumsky-based parser for the Cypher subset.
//! - `builder`: fluent Rust API constructing the same `Query` AST directly.
//!
//! Both surfaces produce identical `Query` values — this is the architectural
//! invariant.
//!
//! The evaluator lives in the backend crate (cfdb-petgraph), but any
//! shape-level lints that are backend-agnostic (e.g. the Cartesian-with-
//! function-equality footgun) live here as a pre-eval pass callers can
//! run before dispatching.

pub mod builder;
pub mod classify;
pub mod diff;
pub mod impact;
pub mod inventory;
pub mod list_items;
pub mod parser;
pub mod shape_lint;

pub use builder::QueryBuilder;
pub use classify::{ClassifyEnvelope, DiffSourceMeta, CLASSIFY_ENVELOPE_SCHEMA_VERSION};
pub use diff::{
    compute_diff, ChangedFact, DiffEnvelope, DiffError, DiffFact, KindsFilter,
    ENVELOPE_SCHEMA_VERSION,
};
pub use impact::{impact_query, items_with_files_query, IMPACT_QUERY};
pub use inventory::{
    CanonicalCandidate, DebtClass, Finding, ReachabilityEntry, ScopeInventory, UnknownDebtClass,
};
pub use list_items::list_items_matching;
pub use parser::{parse, ParseError};
pub use shape_lint::{lint_shape, ShapeLint};
