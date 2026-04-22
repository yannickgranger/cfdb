//! Query evaluator — ports the validated Gate 3 spike logic onto the real
//! `cfdb_core::Query` AST.
//!
//! Evaluation stages, in order:
//! 1. Seed a one-row binding stream `[{}]`.
//! 2. For each `MATCH` / `OPTIONAL MATCH` / `UNWIND` pattern, chain the
//!    binding stream through `apply_pattern` — each stage is an iterator
//!    adapter, not a `Vec` reassignment. Peak memory between MATCH stages
//!    is therefore O(per-row expansion), not O(cartesian product).
//! 3. Apply the `WHERE` predicate as a stream filter.
//! 4. Materialise the surviving rows into a `Vec<Bindings>` — required by
//!    `WITH` / `RETURN` because sort / distinct / group-and-aggregate all
//!    need random access.
//! 5. If present, apply `WITH` — project + group + re-filter.
//! 6. Apply `RETURN` — project, distinct, order, limit.
//!
//! Throughout, variable bindings are keyed by the user's variable names. A
//! binding holds either a `NodeRef` (a `NodeIndex` we can dereference on
//! demand) or a `Value` (a scalar literal from `UNWIND` or a WITH projection).
//!
//! Determinism: every join expansion iterates in the sorted order produced by
//! `KeyspaceState::nodes_with_label` or `all_nodes_sorted`. Stream order is
//! preserved — `flat_map` consumes the input iterator in order and emits each
//! per-row expansion in the order `candidate_nodes` produced. Collected rows
//! in the final WHERE-filtered `Vec` therefore carry the same determinism as
//! the prior non-streaming implementation.
//!
//! # Memory note (issue #167 / #168)
//!
//! The earlier implementation materialised a full `Vec<Bindings>` between
//! every MATCH stage. On a 148k-node keyspace even single-MATCH queries built
//! ~89 MB of `BTreeMap` allocations before `WHERE` could discard them; multi-
//! MATCH (inventory-shaped classifier rules) reached tens of GB. Streaming
//! the pipeline keeps peak memory bounded by the per-row expansion plus the
//! final surviving-row count — structurally eliminating the 13 GB OOM
//! documented in #167.

use std::cell::RefCell;
use std::collections::BTreeMap;

use cfdb_core::query::{Param, Pattern, Query};
use cfdb_core::result::{QueryResult, RowValue, Warning, WarningKind};
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;

mod pattern;
mod predicate;
mod return_clause;
mod util;
mod with_clause;

/// Maximum BFS depth when a variable-length pattern omits its upper bound.
/// Matches the Gate 3 spike (`query_f2` uses 5).
pub(super) const DEFAULT_VAR_LENGTH_MAX: u32 = 5;

/// A bound value in the evaluator's scratch table.
#[derive(Clone, Debug)]
pub(super) enum Binding {
    /// A reference to a graph node. Dereferenced when a property access or
    /// projection needs concrete values.
    NodeRef(NodeIndex),
    /// A concrete value — used for `UNWIND $list AS var` cross-joins and for
    /// projection aliases in `WITH`.
    Value(RowValue),
    /// Null-filled binding produced by an `OPTIONAL MATCH` that found no
    /// match for the inner pattern.
    Null,
}

/// One candidate row of bindings, keyed by variable name.
pub(super) type Bindings = BTreeMap<String, Binding>;

/// Boxed iterator stream of binding rows — the streaming join channel
/// between MATCH stages. `Box<dyn>` (rather than `impl Iterator`) is
/// required because the four `apply_*` branches return different concrete
/// iterator types that must unify at the match dispatch site.
pub(super) type BindingStream<'e> = Box<dyn Iterator<Item = Bindings> + 'e>;

/// Evaluator context. Holds the graph plus accumulating warnings and the
/// query's param bag (so nested `NOT EXISTS { MATCH ... }` shares params).
///
/// `warnings` is wrapped in `RefCell` so streaming `apply_*` methods can
/// take `&self` (not `&mut self`) and still accumulate warnings — this is
/// what lets the pipeline chain through `Iterator::flat_map` without
/// running into borrow-checker conflicts against a mutable receiver.
pub(crate) struct Evaluator<'a> {
    pub(crate) state: &'a KeyspaceState,
    pub(crate) params: &'a BTreeMap<String, Param>,
    pub(crate) warnings: RefCell<Vec<Warning>>,
}

impl<'a> Evaluator<'a> {
    pub(crate) fn new(state: &'a KeyspaceState, params: &'a BTreeMap<String, Param>) -> Self {
        Self {
            state,
            params,
            warnings: RefCell::new(Vec::new()),
        }
    }

    /// Entry point — drive the streaming pipeline for a top-level query.
    pub(crate) fn run(self, query: &Query) -> QueryResult {
        let seed: BindingStream<'_> = Box::new(std::iter::once(BTreeMap::new()));
        let mut stage: BindingStream<'_> = seed;
        for pattern in &query.match_clauses {
            stage = self.apply_pattern(stage, pattern);
        }

        // WHERE filter is chained onto the stream, not applied to a
        // fully-materialised `Vec<Bindings>`. Materialisation happens here
        // at `.collect()` — only rows that survive the filter ever land in
        // the table consumed by WITH / RETURN.
        let table: Vec<Bindings> = match &query.where_clause {
            Some(pred) => stage.filter(|b| self.eval_predicate(pred, b)).collect(),
            None => stage.collect(),
        };

        let table = if let Some(with) = &query.with_clause {
            self.apply_with(table, with)
        } else {
            table
        };

        let rows = self.apply_return(&table, &query.return_clause);

        let should_warn_empty = rows.is_empty()
            && !self.warnings.borrow().iter().any(|w| {
                matches!(
                    w.kind,
                    WarningKind::UnknownLabel | WarningKind::UnknownEdgeLabel
                )
            });
        if should_warn_empty {
            self.warnings.borrow_mut().push(Warning {
                kind: WarningKind::EmptyResult,
                message: "query matched no rows".into(),
                suggestion: None,
            });
        }

        let mut result = QueryResult::with_rows(rows);
        result.warnings = self.warnings.into_inner();
        result
    }

    fn apply_pattern<'e>(
        &'e self,
        table: BindingStream<'e>,
        pattern: &'e Pattern,
    ) -> BindingStream<'e> {
        match pattern {
            Pattern::Node(np) => self.apply_node_pattern(table, np),
            Pattern::Path(pp) => self.apply_path_pattern(table, pp),
            Pattern::Optional(inner) => self.apply_optional(table, inner),
            Pattern::Unwind { list_param, var } => self.apply_unwind(table, list_param, var),
        }
    }
}
