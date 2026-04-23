//! `.unwrap()` / `.expect()` count + McCabe cyclomatic from a
//! `syn::File`. Pure functions — no I/O beyond `std::fs::read_to_string`
//! in the orchestration entry point.
//!
//! # Cyclomatic rule
//!
//! McCabe: `branches + 1`. Branches counted:
//! - `if` / `else if` (each arm adds 1, baseline arm is the +1)
//! - `match` arms (N arms → N-1 branches, the fall-through is the +1)
//! - `while`, `while let`, `for`, `loop` (each loop head is 1 branch)
//! - `&&`, `||` short-circuits (each adds 1)
//! - `?` operator (early-return counts as 1)
//!
//! The `+1` baseline covers the single straight-line path. A trivial
//! `fn x() {}` has cyclomatic = 1.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use syn::visit::{self, Visit};

use super::FnItem;

/// AST-derived per-function signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AstSignals {
    pub unwrap_count: usize,
    pub cyclomatic: usize,
}

/// Scan every distinct source file referenced by `items` and return the
/// per-qname signals. Missing / unparseable files produce a warning and
/// are skipped (other items still get signals).
pub(crate) fn scan_workspace(
    items: &[FnItem],
    workspace_root: &Path,
    warnings: &mut Vec<String>,
) -> BTreeMap<String, AstSignals> {
    let files = distinct_files(items);
    let mut by_file: BTreeMap<String, syn::File> = BTreeMap::new();
    for rel in &files {
        let abs = workspace_root.join(rel);
        match parse_file(&abs) {
            Ok(f) => {
                by_file.insert(rel.clone(), f);
            }
            Err(e) => warnings.push(format!(
                "{}: failed to parse {}: {e}",
                super::VERB,
                abs.display()
            )),
        }
    }

    let mut out: BTreeMap<String, AstSignals> = BTreeMap::new();
    for item in items {
        let Some(file) = by_file.get(&item.file) else {
            continue;
        };
        if let Some(signals) = compute_for_item(file, &item.name) {
            out.insert(item.qname.clone(), signals);
        }
    }
    out
}

fn distinct_files(items: &[FnItem]) -> Vec<String> {
    let set: HashSet<&str> = items.iter().map(|i| i.file.as_str()).collect();
    let mut v: Vec<String> = set.into_iter().map(str::to_string).collect();
    v.sort();
    v
}

fn parse_file(path: &Path) -> Result<syn::File, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    syn::parse_file(&src).map_err(|e| format!("parse: {e}"))
}

/// Find the first `fn` item with matching `ident == name` anywhere in
/// the file (including nested in `impl` / `mod`) and compute its
/// signals. Returns `None` if no match.
pub fn compute_for_item(file: &syn::File, name: &str) -> Option<AstSignals> {
    let mut finder = FnFinder {
        target: name,
        found: None,
    };
    finder.visit_file(file);
    finder.found.map(|block| compute_for_block(&block))
}

struct FnFinder<'a> {
    target: &'a str,
    found: Option<syn::Block>,
}

impl<'ast, 'a> Visit<'ast> for FnFinder<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if self.found.is_none() && node.sig.ident == self.target {
            self.found = Some((*node.block).clone());
            return;
        }
        visit::visit_item_fn(self, node);
    }
    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if self.found.is_none() && node.sig.ident == self.target {
            self.found = Some(node.block.clone());
            return;
        }
        visit::visit_impl_item_fn(self, node);
    }
    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if self.found.is_none() && node.sig.ident == self.target {
            if let Some(block) = &node.default {
                self.found = Some(block.clone());
                return;
            }
        }
        visit::visit_trait_item_fn(self, node);
    }
}

/// Compute signals for a single block. Pure function over `syn::Block`.
/// Exposed for unit tests.
pub fn compute_for_block(block: &syn::Block) -> AstSignals {
    let mut v = SignalVisitor::default();
    v.visit_block(block);
    AstSignals {
        unwrap_count: v.unwrap_count,
        cyclomatic: v.branches + 1,
    }
}

#[derive(Default)]
struct SignalVisitor {
    unwrap_count: usize,
    branches: usize,
}

impl<'ast> Visit<'ast> for SignalVisitor {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "unwrap" || node.method == "expect" {
            self.unwrap_count += 1;
        }
        visit::visit_expr_method_call(self, node);
    }
    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        self.branches += 1;
        visit::visit_expr_if(self, node);
    }
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        // N arms → N-1 branches (the fall-through is the +1 baseline).
        self.branches += node.arms.len().saturating_sub(1);
        visit::visit_expr_match(self, node);
    }
    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.branches += 1;
        visit::visit_expr_while(self, node);
    }
    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.branches += 1;
        visit::visit_expr_for_loop(self, node);
    }
    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.branches += 1;
        visit::visit_expr_loop(self, node);
    }
    fn visit_expr_try(&mut self, node: &'ast syn::ExprTry) {
        self.branches += 1;
        visit::visit_expr_try(self, node);
    }
    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        if matches!(node.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
            self.branches += 1;
        }
        visit::visit_expr_binary(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_block(src: &str) -> syn::Block {
        syn::parse_str::<syn::Block>(src).expect("test fixture parses")
    }

    #[test]
    fn trivial_fn_has_cyclomatic_one_and_zero_unwraps() {
        let block = parse_block("{ let x = 1; }");
        let s = compute_for_block(&block);
        assert_eq!(s.unwrap_count, 0);
        assert_eq!(s.cyclomatic, 1);
    }

    #[test]
    fn three_unwraps_counted() {
        let block =
            parse_block("{ let a = x.unwrap(); let b = y.expect(\"m\"); let c = z.unwrap(); }");
        let s = compute_for_block(&block);
        assert_eq!(s.unwrap_count, 3);
        assert_eq!(s.cyclomatic, 1);
    }

    #[test]
    fn single_if_adds_one_branch() {
        let block = parse_block("{ if x { 1 } else { 2 } }");
        let s = compute_for_block(&block);
        assert_eq!(s.cyclomatic, 2);
    }

    #[test]
    fn match_with_three_arms_adds_two_branches() {
        let block = parse_block("{ match x { 1 => a, 2 => b, _ => c } }");
        let s = compute_for_block(&block);
        assert_eq!(s.cyclomatic, 3);
    }

    #[test]
    fn while_loop_adds_one_branch() {
        let block = parse_block("{ while x < 10 { step(); } }");
        let s = compute_for_block(&block);
        assert_eq!(s.cyclomatic, 2);
    }

    #[test]
    fn short_circuit_ops_add_branches() {
        let block = parse_block("{ let y = a && b || c; }");
        let s = compute_for_block(&block);
        // `a && b` is one branch, `... || c` is another → 1 + 2 = 3.
        assert_eq!(s.cyclomatic, 3);
    }

    #[test]
    fn try_op_adds_one_branch() {
        let block = parse_block("{ let x = foo()?; }");
        let s = compute_for_block(&block);
        assert_eq!(s.cyclomatic, 2);
    }

    #[test]
    fn compute_for_item_finds_nested_impl_method() {
        let file: syn::File =
            syn::parse_str("struct S; impl S { fn target(&self) -> i32 { x.unwrap() } }")
                .expect("fixture parses");
        let s = compute_for_item(&file, "target").expect("finder locates impl method");
        assert_eq!(s.unwrap_count, 1);
        assert_eq!(s.cyclomatic, 1);
    }

    #[test]
    fn compute_for_item_returns_none_on_miss() {
        let file: syn::File = syn::parse_str("fn other() {}").expect("fixture parses");
        assert!(compute_for_item(&file, "target").is_none());
    }
}
