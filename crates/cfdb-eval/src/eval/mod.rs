use std::cell::RefCell;
use std::collections::BTreeMap;

use cfdb_core::graph::{EdgeHandle, GraphReader, NodeHandle};
use cfdb_core::query::{ParamBinding, Pattern, Predicate, Query};
use cfdb_core::result::{QueryResult, RowValue, Warning, WarningKind};

#[cfg(test)]
mod cross_match_tests;
#[cfg(test)]
mod edge_match_regression_tests;
mod explain_fmt;
#[cfg(test)]
mod fast_path_tests;
mod pattern;
mod predicate;
mod return_clause;
#[cfg(test)]
mod target_dogfood_tests;
mod util;
mod with_clause;

pub(super) const DEFAULT_VAR_LENGTH_MAX: u32 = 5;

#[derive(Clone, Debug)]
pub(super) enum Binding {
    NodeRef(NodeHandle),
    EdgeRef(EdgeHandle),
    Value(RowValue),
    Null,
}

pub(super) type Bindings = BTreeMap<String, Binding>;

pub(super) type BindingStream<'e> = Box<dyn Iterator<Item = Bindings> + 'e>;

pub(crate) struct Evaluator<'a, G: ?Sized> {
    pub(crate) state: &'a G,
    pub(crate) params: &'a BTreeMap<String, ParamBinding>,
    pub(crate) warnings: RefCell<Vec<Warning>>,
    pub(crate) explain: Option<RefCell<Vec<crate::explain::ExplainRow>>>,
    pub(crate) regex_cache: RefCell<BTreeMap<String, regex::Regex>>,
}

impl<'a, G: GraphReader + ?Sized> Evaluator<'a, G> {
    pub(crate) fn new(state: &'a G, params: &'a BTreeMap<String, ParamBinding>) -> Self {
        Self {
            state,
            params,
            warnings: RefCell::new(Vec::new()),
            explain: None,
            regex_cache: RefCell::new(BTreeMap::new()),
        }
    }

    pub(crate) fn new_with_explain(
        state: &'a G,
        params: &'a BTreeMap<String, ParamBinding>,
    ) -> Self {
        Self {
            state,
            params,
            warnings: RefCell::new(Vec::new()),
            explain: Some(RefCell::new(Vec::new())),
            regex_cache: RefCell::new(BTreeMap::new()),
        }
    }

    pub(crate) fn compiled_regex<R>(
        &self,
        pattern: &str,
        body: impl FnOnce(&regex::Regex) -> R,
    ) -> Option<R> {
        if self.regex_cache.borrow().contains_key(pattern) {
            let cache = self.regex_cache.borrow();
            return cache.get(pattern).map(body);
        }
        let compiled = regex::Regex::new(pattern).ok()?;
        let mut cache = self.regex_cache.borrow_mut();
        let entry = cache.entry(pattern.to_string()).or_insert(compiled);
        Some(body(entry))
    }

    pub(crate) fn run(self, query: &Query) -> QueryResult {
        let (result, _explain) = self.run_explained(query);
        result
    }

    pub(crate) fn run_explained(
        self,
        query: &Query,
    ) -> (QueryResult, Vec<crate::explain::ExplainRow>) {
        let seed: BindingStream<'_> = Box::new(std::iter::once(BTreeMap::new()));
        let mut stage: BindingStream<'_> = seed;
        let where_ref = query.where_clause.as_ref();
        for pattern in &query.match_clauses {
            stage = self.apply_pattern(stage, pattern, where_ref);
        }

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
        let explain_rows = self
            .explain
            .map(|cell| cell.into_inner())
            .unwrap_or_default();
        (result, explain_rows)
    }

    pub(crate) fn record_explain(&self, pattern: String, hit: crate::explain::ExplainHit) {
        if let Some(cell) = &self.explain {
            cell.borrow_mut()
                .push(crate::explain::ExplainRow { pattern, hit });
        }
    }

    fn apply_pattern<'e>(
        &'e self,
        table: BindingStream<'e>,
        pattern: &'e Pattern,
        where_clause: Option<&'e Predicate>,
    ) -> BindingStream<'e> {
        match pattern {
            Pattern::Node(np) => self.apply_node_pattern(table, np, where_clause),
            Pattern::Path(pp) => self.apply_path_pattern(table, pp, where_clause),
            Pattern::Optional(inner) => self.apply_optional(table, inner, where_clause),
            Pattern::Unwind { list_param, var } => self.apply_unwind(table, list_param, var),
        }
    }
}
