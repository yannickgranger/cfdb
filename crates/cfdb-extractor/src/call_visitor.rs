use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::argument_node_id;
use cfdb_core::schema::{EdgeLabel, Label, RECEIVER_POSITION};
use cfdb_extractor_shared::classify_arg_kind;
use quote::ToTokens as _;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::item_visitor::emit_call_site_node_and_edge;
use crate::type_render::render_path;
use crate::Emitter;

pub(crate) fn walk_call_sites_with_test_flag(
    emitter: &mut Emitter,
    caller_qname: &str,
    caller_target: &cfdb_core::qname::TargetDiscriminator,
    file_path: &str,
    block: &syn::Block,
    is_test: bool,
) {
    let mut visitor = CallSiteVisitor {
        emitter,
        caller_qname,
        caller_target,
        file_path,
        counts: BTreeMap::new(),
        is_test,
    };
    syn::visit::visit_block(&mut visitor, block);
}

struct CallSiteVisitor<'e, 'a> {
    emitter: &'e mut Emitter,
    caller_qname: &'a str,
    caller_target: &'a cfdb_core::qname::TargetDiscriminator,
    file_path: &'a str,
    counts: BTreeMap<String, usize>,
    is_test: bool,
}

impl<'ast> Visit<'ast> for CallSiteVisitor<'_, '_> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*node.func {
            let callee_path = render_path(&p.path);
            let line = node.func.span().start().line;
            let cs_id = self.emit_call_site(&callee_path, "call", line);
            self.emit_arguments(&cs_id, &node.args, 0);
        }
        for arg in &node.args {
            if let syn::Expr::Path(p) = arg {
                let path = render_path(&p.path);
                let line = p.span().start().line;
                self.emit_call_site(&path, "fn_ptr", line);
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        let line = node.method.span().start().line;
        let cs_id = self.emit_call_site(&method, "method", line);
        self.emit_single_argument(&cs_id, &node.receiver, RECEIVER_POSITION);
        self.emit_arguments(&cs_id, &node.args, 1);
        for arg in &node.args {
            if let syn::Expr::Path(p) = arg {
                let path = render_path(&p.path);
                let arg_line = p.span().start().line;
                self.emit_call_site(&path, "fn_ptr", arg_line);
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        crate::macro_tokens::walk_macro_tokens(self, &node.mac);
    }

    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        crate::macro_tokens::walk_macro_tokens(self, &node.mac);
    }
}

impl CallSiteVisitor<'_, '_> {
    fn emit_call_site(&mut self, callee_path: &str, kind: &str, line: usize) -> String {
        let local_idx = {
            let counter = self.counts.entry(callee_path.to_string()).or_insert(0);
            let idx = *counter;
            *counter += 1;
            idx
        };
        let cs_id = cfdb_core::qname::callsite_node_id(
            &self.caller_target.identity(self.caller_qname),
            callee_path,
            local_idx,
        );
        emit_call_site_node_and_edge(
            self.emitter,
            cs_id.clone(),
            self.caller_qname,
            self.caller_target,
            callee_path,
            kind,
            self.file_path.to_string(),
            line,
            self.is_test,
            BTreeMap::new(),
        );
        cs_id
    }

    fn emit_arguments(
        &mut self,
        cs_id: &str,
        args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
        position_offset: u32,
    ) {
        for (i, arg) in args.iter().enumerate() {
            self.emit_single_argument(cs_id, arg, position_offset + i as u32);
        }
    }

    fn emit_single_argument(&mut self, cs_id: &str, expr: &syn::Expr, position: u32) {
        let arg_id = argument_node_id(cs_id, position);
        let span = expr.span();
        let loc = span.start();
        let source_text = expr.to_token_stream().to_string();
        let kind = classify_arg_kind(expr);

        let mut props = BTreeMap::new();
        props.insert("position".into(), PropValue::Int(i64::from(position)));
        props.insert("kind".into(), PropValue::Str(kind.to_string()));
        props.insert("source_text".into(), PropValue::Str(source_text));
        props.insert("file".into(), PropValue::Str(self.file_path.to_string()));
        props.insert("line".into(), PropValue::Int(loc.line as i64));
        props.insert("col".into(), PropValue::Int(loc.column as i64 + 1));

        self.emitter.emit_node(Node {
            id: arg_id.clone(),
            label: Label::new(Label::ARGUMENT),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: cs_id.to_string(),
            dst: arg_id,
            label: EdgeLabel::new(EdgeLabel::HAS_ARG),
            props: BTreeMap::new(),
        });
    }
}
