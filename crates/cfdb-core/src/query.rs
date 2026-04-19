//! Query AST — the interchange format between parser and evaluator.
//!
//! Both the chumsky Cypher-subset parser (cfdb-query::parser) and the Rust
//! builder API (cfdb-query::builder) produce `Query` values. The store
//! evaluator (`StoreBackend::execute`) consumes them. This decoupling is the
//! architectural invariant from study 001 §8.3: backend-swappable by
//! construction.
//!
//! The AST intentionally covers only the v0.1 Cypher subset from study 001
//! §8.4. Extensions belong in v0.2 as additive enum variants — no struct field
//! reshuffles, no breaking changes to existing callers.
//!
//! Submodules:
//! - [`ast`] — core AST node types (`Query`, `Pattern`, `Predicate`, `Expr`,
//!   projections, aggregations, ordering).
//! - [`inventory`] — debt-class taxonomy and `ScopeInventory` envelope
//!   (RFC-cfdb v0.2 addendum §A2/§A3.3).
//! - [`item_kind`] — council-ratified `ItemKind` vocabulary (RATIFIED §A.14).
//! - [`list_items`] — the `list_items_matching` query composer
//!   (RATIFIED §A.14).
//!
//! All public items previously exposed directly from `query` remain available
//! from `crate::query::*` via re-exports below, preserving the v0.1 import
//! surface.

pub mod ast;
pub mod inventory;
pub mod item_kind;
pub mod list_items;

pub use ast::{
    Aggregation, CompareOp, Direction, EdgePattern, Expr, NodePattern, OrderBy, Param, PathPattern,
    Pattern, Predicate, Projection, ProjectionValue, Query, ReturnClause, WithClause,
};
pub use inventory::{
    CanonicalCandidate, DebtClass, Finding, ReachabilityEntry, ScopeInventory, UnknownDebtClass,
};
pub use item_kind::{ItemKind, UnknownItemKind};
pub use list_items::list_items_matching;
