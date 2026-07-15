//! `:Literal` extraction from expression-position string literals
//! (RFC-041 slice 041-B).
//!
//! Mirrors [`call_visitor`](crate::call_visitor): an entry function
//! wraps the emitter in a [`LiteralVisitor`] and drives
//! `syn::visit::visit_block` / `visit_expr`. The visitor emits one
//! `:Literal` node per `syn::Lit::Str` it finds (including those
//! inside `vec!`/`format!`/etc. expression-position macro invocations
//! that are re-parsed via the shared
//! [`crate::macro_tokens::walk_macro_tokens`] helper).
//!
//! Per RFC-041 §3.1 the emitted `value` is the **raw inter-delimiter
//! source bytes** of the literal token, NOT
//! [`syn::LitStr::value()`]. Escape decoding would break the
//! `=~`-matches-`grep` invariant the downstream ban rules rely on
//! (`"phase\tname"` is 11 source bytes including the backslash-t;
//! `value()` returns 10 bytes with an embedded TAB).
//!
//! `is_test` is **inherited** from the calling context: callers in
//! [`crate::item_visitor`] pass the precomputed `is_test` (the OR of
//! `attrs_contain_cfg_test` / `attrs_contain_hash_test` plus
//! `is_in_test_mod` depth). The visitor never re-evaluates the
//! predicate at the literal site — RFC-041 §4 forbids a parallel
//! resolver.
//!
//! ## Exclusions (RFC-041 §6)
//!
//! - `cfg(feature = "...")` strings — these live in **attribute**
//!   `Meta`, not in expression position, and are never reached by a
//!   visitor that only overrides `visit_expr_lit` / `visit_expr_macro`
//!   / `visit_stmt_macro`. Already modelled as `:Item.cfg_gate`.
//! - `#[serde(default = "...")]` strings — same: attribute Meta,
//!   never in expression position. Already modelled as
//!   `:CallSite { kind: "serde_default" }`.
//! - `macro_rules!` declarative-macro bodies — `ItemMacro`'s token
//!   tree is opaque to `syn::visit::Visit`; `syn` never recurses into
//!   it. Hard boundary, not a choice.
//! - `#[doc = "..."]` attribute strings — same Meta-not-expr
//!   exclusion. v0 scope explicit per RFC §6.
//!
//! Expression-position literals inside macro **invocations** like
//! `vec!["a"]` / `format!("...", x)` ARE in scope, and are reached by
//! the same shared [`crate::macro_tokens::walk_macro_tokens`] re-parse
//! helper `call_visitor` and `match_visitor` use.

use std::collections::BTreeMap;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use syn::visit::Visit;

use crate::Emitter;

/// Walk a fn-body block and emit `:Literal` nodes per string literal
/// encountered. `is_test` is the enclosing fn's precomputed flag
/// (callers pass it from `ItemVisitor::fn_is_test` etc.); it is
/// stamped on every emitted node without re-evaluation.
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

/// Walk a const/static initializer expression and emit `:Literal`
/// nodes per string literal. `is_test` is the enclosing scope's
/// `is_in_test_mod()` (consts/statics inherit from the surrounding
/// `#[cfg(test)] mod` only — they have no fn-level `#[test]`).
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
        // Non-string lits (Int / Float / Bool / Char / Byte / ByteStr)
        // are out of v0 scope per RFC-041 §6.
        syn::visit::visit_expr_lit(self, node);
    }

    /// Re-parse expression-position macro invocations so literals
    /// inside `vec!["a"]` / `format!("{}", x)` / `json!({...})` are
    /// captured (RFC-041 §6 — "Expression-position literals inside
    /// macro invocations like vec!/format! remain reachable"). Delegates
    /// to the shared [`crate::macro_tokens::walk_macro_tokens`] helper.
    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        crate::macro_tokens::walk_macro_tokens(self, &node.mac);
    }

    /// Statement-position macros (`assert_eq!(...);`, `tracing::info!(...);`,
    /// `println!(...);`) parse as `Stmt::Macro`, not `Expr::Macro`. Without
    /// this override every literal inside a statement-level macro would be
    /// invisible.
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

/// Pure builder: produce the `:Literal` `Node` for one `syn::LitStr`.
///
/// Splitting this out of [`LiteralVisitor::emit_literal`] lets the unit
/// tests assert the contract (RFC §3.1: label, raw-bytes value, no
/// `kind` attr, is_test propagation, node-id shape) without standing
/// up a full `Emitter`.
pub(crate) fn build_literal_node(
    lit: &syn::LitStr,
    file_path: &str,
    crate_name: &str,
    is_test: bool,
) -> Node {
    let value = raw_inter_delimiter_bytes(lit);
    let span = lit.span().start();
    // `proc_macro2::LineColumn::column` is 0-based; convert to the
    // 1-indexed convention used by `:CallSite.line` / our descriptor.
    let line = span.line as i64;
    let col = (span.column as i64) + 1;
    let id = format!("literal:{file_path}:{line}:{col}");

    let mut props: BTreeMap<String, PropValue> = BTreeMap::new();
    // RFC §3.1: descriptor lists exactly these 6 attrs in alphabetical
    // order; no `kind`. Insertion order matters only for human-reading
    // the serialized output — `cfdb-petgraph` keys props by name.
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

/// Extract the raw inter-delimiter bytes of a string literal from its
/// source-token form (`proc_macro2::Literal::to_string()` round-trips
/// to the canonical source representation). For:
///
/// - plain `"abc\n"` — `to_string()` returns `"abc\n"` (12 source
///   chars: the leading quote, `a`, `b`, `c`, `\`, `n`, the trailing
///   quote — escape preserved). We strip the two quotes; the
///   backslash-n remains as two characters.
/// - raw `r#"abc"#` — `to_string()` returns `r#"abc"#`. We strip the
///   `r`, the leading `#`s, the leading quote, the trailing quote,
///   and the matching trailing `#`s. Raw strings carry no escapes by
///   grammar.
/// - byte strings `b"..."` / byte-raw `br#"..."#` — these are
///   `syn::Lit::ByteStr`, not `LitStr`, and never reach this helper.
fn raw_inter_delimiter_bytes(lit: &syn::LitStr) -> String {
    let repr = lit.token().to_string();
    let bytes = repr.as_bytes();
    if bytes.first().copied() == Some(b'r') {
        // Count hash-pad pairs: r###"..."### has 3 `#`s on each side.
        let mut i = 1usize;
        while bytes.get(i).copied() == Some(b'#') {
            i += 1;
        }
        let hashes = i - 1;
        // bytes[i] is the opening quote; content runs to (len - 1 - hashes).
        let end = bytes.len() - 1 - hashes;
        return String::from_utf8_lossy(&bytes[i + 1..end]).into_owned();
    }
    // Plain "..." — strip the two quote bytes.
    String::from_utf8_lossy(&bytes[1..bytes.len() - 1]).into_owned()
}

#[cfg(test)]
mod tests {
    //! Pure-function tests of `build_literal_node` and
    //! `raw_inter_delimiter_bytes` against the RFC-041 §3.1 contract
    //! (cfdb #370 prescribed Unit row).

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
        // Source: "phase\tname" — 11 bytes between the quotes
        // (5 + 1 backslash + 1 t + 4). `LitStr::value()` would
        // decode to "phase<TAB>name" (10 bytes including the TAB).
        // RFC-041 §3.1 mandates the raw source form for the
        // `=~`-matches-`grep` invariant.
        let lit = parse_lit_str("\"phase\\tname\"");
        let v = raw_inter_delimiter_bytes(&lit);
        assert_eq!(v, "phase\\tname");
        assert_eq!(v.len(), 11);
        // Verify divergence from `value()`.
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
        // Content can include `"#` because the closing delimiter is `"##`.
        let lit = parse_lit_str("r##\"foo \\#more\"##");
        assert_eq!(raw_inter_delimiter_bytes(&lit), "foo \\#more");
    }

    #[test]
    fn multiline_literal_preserves_backslash_n_verbatim() {
        // Source: "line1\nline2" — backslash-n stays as two bytes in
        // the raw inter-delimiter form. RFC-041 §3.1.
        let lit = parse_lit_str("\"line1\\nline2\"");
        let v = raw_inter_delimiter_bytes(&lit);
        assert_eq!(v, "line1\\nline2");
        // The backslash is a real character, not part of an escape.
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
        // The parsed literal sits at line 1, column 0 in proc-macro2
        // (its own one-token span). Our descriptor uses 1-indexed col,
        // so col=1.
        let lit = parse_lit_str("\"x\"");
        let node = build_literal_node(&lit, "a/b.rs", "foo", false);
        assert_eq!(node.id, "literal:a/b.rs:1:1");
    }

    #[test]
    fn build_literal_node_props_are_exactly_six_no_kind() {
        // RFC-041 §3.1 + ddd lens: NO `kind` attr; exactly the six
        // descriptor-prescribed attrs.
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
        // RFC-041 §3.1: `value` is the raw inter-delimiter source
        // bytes, NOT `LitStr::value()`. This is the load-bearing
        // contract for the `=~`-matches-`grep` invariant; pinned
        // here separate from the helper-level test.
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
