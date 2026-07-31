//! `:MatchSite` extraction inside a function body (RFC-053 slice 53-A).
//!
//! Mirrors [`call_visitor`](crate::call_visitor) and
//! [`literal_visitor`](crate::literal_visitor): an entry function wraps the
//! emitter in a [`MatchSiteVisitor`] and drives `syn::visit::visit_block`
//! as a third independent per-fn-body pass. The visitor emits one
//! `:MatchSite` node per (`match` expression, distinct name-level
//! arm-pattern-path prefix) pair, plus a `MATCHES_AT` edge from the
//! containing fn/method `:Item` — mirroring `INVOKES_AT` for call sites.
//!
//! **Name-level, unresolved.** `matched_path` is the all-but-last-segment
//! prefix of a multi-segment arm-pattern path exactly as the author wrote
//! it — same doctrine as `:CallSite.callee_path`. Each emitted site also
//! queues its prefix on `Emitter.deferred_match_targets`; the post-walk
//! [`crate::resolver::resolve_deferred_match_targets`] pass (slice 53-B)
//! emits the resolved `MATCHES_ON` edge when the prefix resolves to a
//! workspace enum. An external-type match (e.g. `syn::Visibility`) keeps
//! its `:MatchSite` with no resolved edge.
//!
//! `is_test` propagates the enclosing `#[cfg(test)]` depth flag exactly as
//! `:CallSite` / `:Literal` do — threaded, never re-evaluated at the match
//! site (RFC-041 §4 fidelity invariant).
//!
//! Match expressions inside re-parseable macro *invocation* bodies are
//! extracted via the shared [`crate::macro_tokens::walk_macro_tokens`]
//! helper, consistent with call sites and literals; `macro_rules!`
//! *definitions* stay opaque (RFC-053 §3.3, §3.6). The pure prefix
//! extraction lives in the sibling [`prefix`] module.

mod prefix;

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Label};
use syn::visit::Visit;

use crate::Emitter;

/// Walk a fn-body block and emit `:MatchSite` nodes + `MATCHES_AT` edges
/// per `match` expression. `is_test` is the enclosing fn's precomputed
/// flag (callers pass it exactly as they do for `:CallSite` / `:Literal`);
/// it is stamped on every emitted node without re-evaluation.
pub(crate) fn walk_match_sites_with_test_flag(
    emitter: &mut Emitter,
    fn_qname: &str,
    fn_target: &cfdb_core::qname::TargetDiscriminator,
    file_path: &str,
    crate_name: &str,
    block: &syn::Block,
    is_test: bool,
) {
    let mut visitor = MatchSiteVisitor {
        emitter,
        fn_qname,
        fn_target,
        file_path,
        crate_name,
        counts: BTreeMap::new(),
        is_test,
    };
    syn::visit::visit_block(&mut visitor, block);
}

struct MatchSiteVisitor<'e, 'a> {
    emitter: &'e mut Emitter,
    fn_qname: &'a str,
    fn_target: &'a cfdb_core::qname::TargetDiscriminator,
    file_path: &'a str,
    crate_name: &'a str,
    /// Count of prior occurrences of each matched-path prefix within this
    /// fn body — builds collision-free `:MatchSite` ids when the same
    /// prefix appears in more than one `match` expression (even on the same
    /// line). The counter is incremented once per DISTINCT prefix per match
    /// expression (dedup happens in [`prefix::match_facts`], BEFORE the id
    /// is built — RFC-053 §3.1), so a multi-arm single-prefix `match`
    /// increments it exactly once.
    counts: BTreeMap<String, usize>,
    is_test: bool,
}

impl<'ast> Visit<'ast> for MatchSiteVisitor<'_, '_> {
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        let facts = prefix::match_facts(&node.arms);
        // 1-indexed line of the `match` keyword — the "where the match
        // happens" a human reader points to (mirrors the `impl_token`
        // line convention in `item_visitor/visits.rs`).
        let line = node.match_token.span.start().line;
        for matched_path in &facts.prefixes {
            self.emit_match_site(matched_path, line, facts.arm_count, facts.wildcard);
        }
        // Recurse — nested matches inside arm bodies must not be lost.
        syn::visit::visit_expr_match(self, node);
    }

    /// Re-parse expression-position macro invocations so `match`
    /// expressions inside `vec![...]`, `some_macro!(...)`, etc. are
    /// captured. Delegates to the shared helper `:CallSite` / `:Literal`
    /// use — macro *invocation* bodies are in scope, `macro_rules!`
    /// definitions are not.
    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        crate::macro_tokens::walk_macro_tokens(self, &node.mac);
    }

    /// Statement-position macros (`some_macro!(...);`) parse as
    /// `Stmt::Macro`, not `Expr::Macro`; without this override a `match`
    /// inside a statement-level macro is invisible. Same delegation.
    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        crate::macro_tokens::walk_macro_tokens(self, &node.mac);
    }
}

impl MatchSiteVisitor<'_, '_> {
    fn emit_match_site(&mut self, matched_path: &str, line: usize, arm_count: u32, wildcard: bool) {
        let local_idx = {
            let counter = self.counts.entry(matched_path.to_string()).or_insert(0);
            let idx = *counter;
            *counter += 1;
            idx
        };
        // Extractor-local id — deliberately NOT a `cfdb_core::qname` helper
        // (RFC-032 §3 keeps site-id schemes out of core so the syn tier and
        // a future HIR tier can differ). The prefix is a mandatory id
        // component: one `match` expression can emit several sites.
        let id = format!(
            "matchsite:{}:{}:{}",
            self.fn_target.identity(self.fn_qname),
            matched_path,
            local_idx
        );

        let mut props = BTreeMap::new();
        props.insert("arm_count".into(), PropValue::Int(i64::from(arm_count)));
        props.insert("crate".into(), PropValue::Str(self.crate_name.to_string()));
        props.insert("file".into(), PropValue::Str(self.file_path.to_string()));
        props.insert("is_test".into(), PropValue::Bool(self.is_test));
        props.insert("line".into(), PropValue::Int(line as i64));
        props.insert(
            "matched_path".into(),
            PropValue::Str(matched_path.to_string()),
        );
        props.insert("wildcard".into(), PropValue::Bool(wildcard));

        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::MATCH_SITE),
            props,
        });
        // Queue the name-level prefix for post-walk MATCHES_ON resolution
        // (RFC-053 §3.2, slice 53-B). The resolver
        // (`resolve_deferred_match_targets`) emits the edge only when the
        // prefix resolves to a workspace enum; an external / unresolvable
        // prefix leaves this `:MatchSite` with no MATCHES_ON edge — the
        // honest name-level-only representation. Queued in walk order so
        // the emitted edges are queue-order-independent after the final
        // sort (G1).
        self.emitter.deferred_match_targets.push((
            id.clone(),
            matched_path.to_string(),
            self.fn_target.clone(),
        ));
        // MATCHES_AT: the containing fn/method Item → this MatchSite,
        // mirroring INVOKES_AT for call sites.
        self.emitter.emit_edge(Edge {
            src: item_node_id(&self.fn_target.identity(self.fn_qname)),
            dst: id,
            label: EdgeLabel::new(EdgeLabel::MATCHES_AT),
            props: BTreeMap::new(),
        });
    }
}

#[cfg(test)]
mod tests;
