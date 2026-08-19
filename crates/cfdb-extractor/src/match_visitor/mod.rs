mod prefix;

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use syn::visit::Visit;

use crate::Emitter;

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
    counts: BTreeMap<String, usize>,
    is_test: bool,
}

impl<'ast> Visit<'ast> for MatchSiteVisitor<'_, '_> {
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        let facts = prefix::match_facts(&node.arms);
        let line = node.match_token.span.start().line;
        for matched_path in &facts.prefixes {
            self.emit_match_site(matched_path, line, facts.arm_count, facts.wildcard);
        }
        syn::visit::visit_expr_match(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        crate::macro_tokens::walk_macro_tokens(self, &node.mac);
    }

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
        let id = cfdb_core::qname::matchsite_node_id(
            &self.fn_target.identity(self.fn_qname),
            matched_path,
            local_idx,
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
        self.emitter.deferred_match_targets.push((
            id.clone(),
            matched_path.to_string(),
            self.fn_target.clone(),
        ));
        self.emitter.emit_edge(Edge {
            src: cfdb_core::qname::item_node_id_for_target(self.fn_qname, self.fn_target),
            dst: id,
            label: EdgeLabel::new(EdgeLabel::MATCHES_AT),
            props: BTreeMap::new(),
        });
    }
}

#[cfg(test)]
mod tests;
