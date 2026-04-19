//! Query evaluator — ports the validated Gate 3 spike logic onto the real
//! `cfdb_core::Query` AST.
//!
//! Evaluation stages, in order:
//! 1. Seed an empty binding table `[{}]`.
//! 2. For each `MATCH` / `OPTIONAL MATCH` / `UNWIND` pattern, expand the
//!    binding table by joining it against the pattern's matches.
//! 3. Apply the `WHERE` predicate to filter the binding table.
//! 4. If present, apply `WITH` — project + group + re-filter.
//! 5. Apply `RETURN` — project, distinct, order, limit.
//!
//! Throughout, variable bindings are keyed by the user's variable names. A
//! binding holds either a `NodeRef` (a `NodeIndex` we can dereference on
//! demand) or a `Value` (a scalar literal from `UNWIND` or a WITH projection).
//!
//! Determinism: every join expansion iterates in the sorted order produced by
//! `KeyspaceState::nodes_with_label` or `all_nodes_sorted`. The binding table
//! is a plain `Vec` — order in equals order out.

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

/// Evaluator context. Holds the graph plus accumulating warnings and the
/// query's param bag (so nested `NOT EXISTS { MATCH ... }` shares params).
pub(crate) struct Evaluator<'a> {
    pub(crate) state: &'a KeyspaceState,
    pub(crate) params: &'a BTreeMap<String, Param>,
    pub(crate) warnings: Vec<Warning>,
}

impl<'a> Evaluator<'a> {
    pub(crate) fn new(state: &'a KeyspaceState, params: &'a BTreeMap<String, Param>) -> Self {
        Self {
            state,
            params,
            warnings: Vec::new(),
        }
    }

    /// Entry point — drive the 5-stage pipeline for a top-level query.
    pub(crate) fn run(mut self, query: &Query) -> QueryResult {
        let mut table: Vec<Bindings> = vec![BTreeMap::new()];

        for pattern in &query.match_clauses {
            table = self.apply_pattern(table, pattern);
        }

        if let Some(pred) = &query.where_clause {
            table.retain(|b| self.eval_predicate(pred, b));
        }

        let table = if let Some(with) = &query.with_clause {
            self.apply_with(table, with)
        } else {
            table
        };

        let rows = self.apply_return(&table, &query.return_clause);

        if rows.is_empty()
            && !self.warnings.iter().any(|w| {
                matches!(
                    w.kind,
                    WarningKind::UnknownLabel | WarningKind::UnknownEdgeLabel
                )
            })
        {
            self.warnings.push(Warning {
                kind: WarningKind::EmptyResult,
                message: "query matched no rows".into(),
                suggestion: None,
            });
        }

        let mut result = QueryResult::with_rows(rows);
        result.warnings = self.warnings;
        result
    }

    fn apply_pattern(&mut self, table: Vec<Bindings>, pattern: &Pattern) -> Vec<Bindings> {
        match pattern {
            Pattern::Node(np) => self.apply_node_pattern(table, np),
            Pattern::Path(pp) => self.apply_path_pattern(table, pp),
            Pattern::Optional(inner) => self.apply_optional(table, inner),
            Pattern::Unwind { list_param, var } => self.apply_unwind(table, list_param, var),
        }
    }
}
