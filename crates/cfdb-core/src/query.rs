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
//! - [`item_kind`] — council-ratified `ItemKind` vocabulary (RATIFIED §A.14).
//!
//! Moved to `cfdb-query` per RFC-031 §3 (CRP — verb-level composers and
//! debt taxonomy do not belong in the most-stable crate):
//! - `inventory` (`DebtClass`, `ScopeInventory`, `Finding`, `CanonicalCandidate`,
//!   `ReachabilityEntry`, `UnknownDebtClass`) — now `cfdb_query::inventory::*`.
//! - `list_items` (`list_items_matching` composer) — now `cfdb_query::list_items`.
//!
//! `ItemKind` is deliberately retained here per RFC-031 §3 note — the
//! schema-vs-verb question is deferred to v0.2 schema design.

pub mod ast;
pub mod item_kind;

pub use ast::{
    Aggregation, CompareOp, Direction, EdgePattern, Expr, NodePattern, OrderBy, Param, PathPattern,
    Pattern, Predicate, Projection, ProjectionValue, Query, ReturnClause, WithClause,
};
pub use item_kind::{ItemKind, UnknownItemKind};
