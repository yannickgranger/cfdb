mod coupling;
mod path;

use coupling::*;

use cfdb_core::fact::PropValue;
use cfdb_core::graph::{GraphReader, NodeHandle};
use cfdb_core::query::{NodePattern, ParamBinding, Pattern, Predicate};
use cfdb_core::result::{Warning, WarningKind};

use super::explain_fmt::format_node_pattern;
use super::util::suggest_label;
use super::{Binding, BindingStream, Bindings, Evaluator};
use crate::explain::ExplainHit;

impl<'a, G: GraphReader + ?Sized> Evaluator<'a, G> {
    pub(super) fn apply_node_pattern<'e>(
        &'e self,
        table: BindingStream<'e>,
        np: &'e NodePattern,
        where_clause: Option<&'e Predicate>,
    ) -> BindingStream<'e> {
        if is_binding_independent_pattern(np, where_clause, self.state) {
            let cached = self.candidate_nodes(np, where_clause, &Bindings::new());
            return Box::new(table.flat_map(move |bindings| {
                let mut out: Vec<Bindings> = Vec::new();
                self.emit_node_bindings(&mut out, bindings, &cached, np);
                out
            }));
        }
        Box::new(table.flat_map(move |bindings| {
            let candidates = self.candidate_nodes(np, where_clause, &bindings);
            let mut out: Vec<Bindings> = Vec::new();
            self.emit_node_bindings(&mut out, bindings, &candidates, np);
            out
        }))
    }

    fn emit_node_bindings(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        candidates: &[NodeHandle],
        np: &NodePattern,
    ) {
        match np.var.as_deref() {
            None => self.emit_anon_node(out, bindings, candidates, np),
            Some(var) if bindings.contains_key(var) => {
                self.emit_bound_node(out, bindings, var, candidates, np);
            }
            Some(var) => self.emit_new_var_node(out, bindings, var, candidates, np),
        }
    }

    fn emit_anon_node(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        candidates: &[NodeHandle],
        np: &NodePattern,
    ) {
        candidates
            .iter()
            .filter(|h| self.node_props_match(**h, np))
            .for_each(|_| out.push(bindings.clone()));
    }

    fn emit_bound_node(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        var: &str,
        candidates: &[NodeHandle],
        np: &NodePattern,
    ) {
        let existing = match bindings.get(var) {
            Some(b) => b,
            None => return,
        };
        let any_hit = candidates
            .iter()
            .any(|h| matches_existing(existing, *h) && self.node_props_match(*h, np));
        if any_hit {
            out.push(bindings);
        }
    }

    fn emit_new_var_node(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        var: &str,
        candidates: &[NodeHandle],
        np: &NodePattern,
    ) {
        candidates
            .iter()
            .filter(|h| self.node_props_match(**h, np))
            .for_each(|h| {
                let mut next = bindings.clone();
                next.insert(var.to_string(), Binding::NodeRef(*h));
                out.push(next);
            });
    }

    pub(super) fn candidate_nodes(
        &self,
        np: &NodePattern,
        where_clause: Option<&Predicate>,
        bindings: &Bindings,
    ) -> Vec<NodeHandle> {
        if let Some(label) = &np.label {
            if !self.state.has_label(label) {
                let known = self.state.labels();
                let suggestion = suggest_label(label.as_str(), known.iter().map(|l| l.as_str()));
                self.warnings.borrow_mut().push(Warning {
                    kind: WarningKind::UnknownLabel,
                    message: format!("unknown node label: {}", label),
                    suggestion,
                });
                return Vec::new();
            }
            let bound_var_prop =
                |var: &str, prop: &str| self.bound_var_prop_value(bindings, var, prop);
            if let Some(indexed) =
                self.state
                    .index_candidates(np, where_clause, self.params, &bound_var_prop)
            {
                self.record_explain(format_node_pattern(np), ExplainHit::Indexed);
                return indexed;
            }
            self.record_explain(format_node_pattern(np), ExplainHit::Fallback);
            self.state.nodes_with_label(label)
        } else {
            self.record_explain(format_node_pattern(np), ExplainHit::Fallback);
            self.state.all_nodes_sorted()
        }
    }

    fn bound_var_prop_value(
        &self,
        bindings: &Bindings,
        var: &str,
        prop: &str,
    ) -> Option<PropValue> {
        let Some(Binding::NodeRef(h)) = bindings.get(var) else {
            return None;
        };
        self.state.node(*h)?.props.get(prop).cloned()
    }

    pub(super) fn node_props_match(&self, h: NodeHandle, np: &NodePattern) -> bool {
        let Some(node) = self.state.node(h) else {
            return false;
        };
        for (k, v) in &np.props {
            match node.props.get(k) {
                Some(actual) if actual == v => {}
                _ => return false,
            }
        }
        true
    }

    pub(super) fn apply_optional<'e>(
        &'e self,
        table: BindingStream<'e>,
        inner: &'e Pattern,
        where_clause: Option<&'e Predicate>,
    ) -> BindingStream<'e> {
        Box::new(table.flat_map(move |bindings| {
            let mut out: Vec<Bindings> = Vec::new();
            self.apply_optional_row(&mut out, bindings, inner, where_clause);
            out
        }))
    }

    fn apply_optional_row(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        inner: &Pattern,
        where_clause: Option<&Predicate>,
    ) {
        let inner_seed: BindingStream<'_> = Box::new(std::iter::once(bindings.clone()));
        let expanded: Vec<Bindings> = self
            .apply_pattern(inner_seed, inner, where_clause)
            .collect();
        if expanded.is_empty() {
            let mut null_filled = bindings;
            for var in collect_pattern_vars(inner) {
                null_filled.entry(var).or_insert(Binding::Null);
            }
            out.push(null_filled);
        } else {
            out.extend(expanded);
        }
    }

    pub(super) fn apply_unwind<'e>(
        &'e self,
        table: BindingStream<'e>,
        list_param: &'e str,
        var: &'e str,
    ) -> BindingStream<'e> {
        let Some(ParamBinding::List(items)) = self.params.get(list_param) else {
            self.warnings.borrow_mut().push(Warning {
                kind: WarningKind::EmptyResult,
                message: format!("UNWIND ${}: parameter missing or not a list", list_param),
                suggestion: None,
            });
            return Box::new(std::iter::empty());
        };
        Box::new(table.flat_map(move |bindings| {
            let mut out: Vec<Bindings> = Vec::new();
            unwind_row(&mut out, &bindings, items, var);
            out
        }))
    }
}
