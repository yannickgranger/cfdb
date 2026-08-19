use std::collections::BTreeMap;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use syn::visit::Visit;

use crate::Emitter;

pub(crate) fn walk_literals_in_block(
    emitter: &mut Emitter,
    file_path: &str,
    crate_name: &str,
    block: &syn::Block,
    is_test: bool,
) {
    let mut visitor = LiteralVisitor {
        emitter,
        file_path,
        crate_name,
        is_test,
    };
    syn::visit::visit_block(&mut visitor, block);
}

pub(crate) fn walk_literals_in_expr(
    emitter: &mut Emitter,
    file_path: &str,
    crate_name: &str,
    expr: &syn::Expr,
    is_test: bool,
) {
    let mut visitor = LiteralVisitor {
        emitter,
        file_path,
        crate_name,
        is_test,
    };
    syn::visit::visit_expr(&mut visitor, expr);
}

struct LiteralVisitor<'e, 'a> {
    emitter: &'e mut Emitter,
    file_path: &'a str,
    crate_name: &'a str,
    is_test: bool,
}

impl<'ast> Visit<'ast> for LiteralVisitor<'_, '_> {
    fn visit_expr_lit(&mut self, node: &'ast syn::ExprLit) {
        if let syn::Lit::Str(lit) = &node.lit {
            self.emit_literal(lit);
        }
        syn::visit::visit_expr_lit(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        crate::macro_tokens::walk_macro_tokens(self, &node.mac);
    }

    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        crate::macro_tokens::walk_macro_tokens(self, &node.mac);
    }
}

impl LiteralVisitor<'_, '_> {
    fn emit_literal(&mut self, lit: &syn::LitStr) {
        let node = build_literal_node(lit, self.file_path, self.crate_name, self.is_test);
        self.emitter.emit_node(node);
    }
}

pub(crate) fn build_literal_node(
    lit: &syn::LitStr,
    file_path: &str,
    crate_name: &str,
    is_test: bool,
) -> Node {
    let value = raw_inter_delimiter_bytes(lit);
    let span = lit.span().start();
    let line = span.line as i64;
    let col = (span.column as i64) + 1;
    let id = format!("literal:{file_path}:{line}:{col}");

    let mut props: BTreeMap<String, PropValue> = BTreeMap::new();
    props.insert("col".into(), PropValue::Int(col));
    props.insert("crate".into(), PropValue::Str(crate_name.to_string()));
    props.insert("file".into(), PropValue::Str(file_path.to_string()));
    props.insert("is_test".into(), PropValue::Bool(is_test));
    props.insert("line".into(), PropValue::Int(line));
    props.insert("value".into(), PropValue::Str(value));

    Node {
        id,
        label: Label::new(Label::LITERAL),
        props,
    }
}

fn raw_inter_delimiter_bytes(lit: &syn::LitStr) -> String {
    let repr = lit.token().to_string();
    let bytes = repr.as_bytes();
    if bytes.first().copied() == Some(b'r') {
        let mut i = 1usize;
        while bytes.get(i).copied() == Some(b'#') {
            i += 1;
        }
        let hashes = i - 1;
        let end = bytes.len() - 1 - hashes;
        return String::from_utf8_lossy(&bytes[i + 1..end]).into_owned();
    }
    String::from_utf8_lossy(&bytes[1..bytes.len() - 1]).into_owned()
}

#[cfg(test)]
mod tests {

    use super::*;
    use cfdb_core::schema::Label;

    fn parse_lit_str(src: &str) -> syn::LitStr {
        syn::parse_str(src).expect("valid string literal source")
    }

    #[test]
    fn plain_string_inter_delimiter_bytes() {
        let lit = parse_lit_str("\"hello world\"");
        assert_eq!(raw_inter_delimiter_bytes(&lit), "hello world");
    }

    #[test]
    fn plain_string_preserves_escape_chars_verbatim() {
        let lit = parse_lit_str("\"phase\\tname\"");
        let v = raw_inter_delimiter_bytes(&lit);
        assert_eq!(v, "phase\\tname");
        assert_eq!(v.len(), 11);
        assert_ne!(v, lit.value());
        assert_eq!(lit.value().len(), 10);
    }

    #[test]
    fn raw_string_strips_r_and_hash_delimiters() {
        let lit = parse_lit_str("r#\"shipping\"#");
        assert_eq!(raw_inter_delimiter_bytes(&lit), "shipping");
    }

    #[test]
    fn raw_string_two_hash_pairs() {
        let lit = parse_lit_str("r##\"foo \\#more\"##");
        assert_eq!(raw_inter_delimiter_bytes(&lit), "foo \\#more");
    }

    #[test]
    fn multiline_literal_preserves_backslash_n_verbatim() {
        let lit = parse_lit_str("\"line1\\nline2\"");
        let v = raw_inter_delimiter_bytes(&lit);
        assert_eq!(v, "line1\\nline2");
        assert!(v.contains('\\'));
        assert!(!v.contains('\n'));
    }

    #[test]
    fn build_literal_node_label_is_literal() {
        let lit = parse_lit_str("\"verifying\"");
        let node = build_literal_node(&lit, "crates/foo/src/lib.rs", "foo", false);
        assert_eq!(node.label.as_str(), Label::LITERAL);
    }

    #[test]
    fn build_literal_node_id_is_file_line_col() {
        let lit = parse_lit_str("\"x\"");
        let node = build_literal_node(&lit, "a/b.rs", "foo", false);
        assert_eq!(node.id, "literal:a/b.rs:1:1");
    }

    #[test]
    fn build_literal_node_props_are_exactly_six_no_kind() {
        let lit = parse_lit_str("\"x\"");
        let node = build_literal_node(&lit, "a/b.rs", "foo", false);
        let names: std::collections::BTreeSet<&str> =
            node.props.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::BTreeSet<&str> =
            ["col", "crate", "file", "is_test", "line", "value"]
                .into_iter()
                .collect();
        assert_eq!(names, expected);
        assert!(!node.props.contains_key("kind"));
    }

    #[test]
    fn build_literal_node_is_test_propagates() {
        let lit = parse_lit_str("\"x\"");
        let prod = build_literal_node(&lit, "a/b.rs", "foo", false);
        let test = build_literal_node(&lit, "a/b.rs", "foo", true);
        assert_eq!(prod.props.get("is_test"), Some(&PropValue::Bool(false)));
        assert_eq!(test.props.get("is_test"), Some(&PropValue::Bool(true)));
    }

    #[test]
    fn build_literal_node_value_is_raw_bytes_not_decoded() {
        let lit = parse_lit_str("\"phase\\tname\"");
        let node = build_literal_node(&lit, "a/b.rs", "foo", false);
        let value = node
            .props
            .get("value")
            .and_then(|v| v.as_str())
            .expect("value prop is a string");
        assert_eq!(value, "phase\\tname");
        assert_ne!(value, lit.value());
    }

    #[test]
    fn build_literal_node_crate_and_file_propagate() {
        let lit = parse_lit_str("\"x\"");
        let node = build_literal_node(&lit, "crates/my-crate/src/lib.rs", "my-crate", false);
        assert_eq!(
            node.props.get("file").and_then(|v| v.as_str()),
            Some("crates/my-crate/src/lib.rs")
        );
        assert_eq!(
            node.props.get("crate").and_then(|v| v.as_str()),
            Some("my-crate")
        );
    }
}
