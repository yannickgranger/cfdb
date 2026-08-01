//! Qname derivation — map `hir::Function` and call sites to the
//! canonical `item:<qname>` form shared with the syn extractor. Split
//! out of `call_site_emitter.rs` (#467).

use cfdb_core::qname::{item_qname, method_qname, normalize_impl_target};
use ra_ap_edition::Edition;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{
    AsAssocItem, AssocItemContainer, DisplayTarget, Function, HasCrate, HirDisplay, Semantics,
};
use ra_ap_syntax::ast::{self, AstNode};
use ra_ap_syntax::SyntaxNode;

/// Walk the syntax-tree ancestors of `node` looking for the
/// enclosing `fn` (top-level or associated method). Returns the
/// resolved `hir::Function` — callers derive both the qname (via
/// [`function_qname`]) and the RFC-054 target discriminator (via the
/// function's crate) from it.
pub(super) fn enclosing_fn<DB>(sema: &Semantics<'_, DB>, node: &SyntaxNode) -> Option<Function>
where
    DB: HirDatabase + Sized,
{
    let fn_ast = node.ancestors().find_map(ast::Fn::cast)?;
    sema.to_def(&fn_ast)
}

/// Derive an `item:<qname>`-compatible qname for a `hir::Function`
/// using the canonical `cfdb_core::qname` formula. Both the syn and
/// HIR extractors share this formula so cross-extractor edges land
/// on the same Item node (DDD HIGH finding in #40 decomposition).
///
/// `pub(crate)` so [`crate::entry_point_emitter`] can reuse the same
/// formula when resolving `http_route` handler paths (Issue #124,
/// `ddd-specialist` gate: cross-kind ID stability — a handler fn
/// reached via `Semantics::resolve_path` must produce the same qname
/// as the same fn reached via `Semantics::resolve_method_call`).
pub(crate) fn function_qname<DB>(sema: &Semantics<'_, DB>, func: Function) -> String
where
    DB: HirDatabase + Sized,
{
    let db = sema.db;
    let module_stack = build_module_stack(db, func);
    let fn_name = func
        .name(db)
        .display_no_db(Edition::Edition2021)
        .to_string();

    // If the function is an associated item inside an impl block,
    // produce `<module_qpath>::<impl_target>::<method>`. Else
    // `<module_qpath>::<fn_name>`. This mirrors cfdb-extractor's
    // item_visitor.rs derivation: method qnames interpose the impl
    // target between the enclosing module and the method name.
    if let Some(assoc) = AsAssocItem::as_assoc_item(func, db) {
        let display_target = DisplayTarget::from_crate(db, func.krate(db).into());
        match assoc.container(db) {
            AssocItemContainer::Impl(impl_block) => {
                // `HirDisplay` emits the fully monomorphised form
                // (`Vec<Node>`); `cfdb-extractor`'s syn renderer emits
                // the stripped form (`Vec`). Route through
                // `normalize_impl_target` so both extractors converge
                // on the same qname for `CALLS(Item→Item)` — #94 ddd
                // Q1 fix.
                let rendered = impl_block
                    .self_ty(db)
                    .display(db, display_target)
                    .to_string();
                let target = normalize_impl_target(&rendered);
                method_qname(&module_stack, &target, &fn_name)
            }
            AssocItemContainer::Trait(trait_def) => {
                let target = trait_def
                    .name(db)
                    .display_no_db(Edition::Edition2021)
                    .to_string();
                method_qname(&module_stack, &target, &fn_name)
            }
        }
    } else {
        item_qname(&module_stack, &fn_name)
    }
}

/// Build the module stack for a `hir::Function` — an ordered list
/// of module names from the crate root to (and including) the
/// enclosing module, with the crate name as the first element
/// (matching `cfdb-extractor/src/item_visitor.rs` convention).
fn build_module_stack<DB>(db: &DB, func: Function) -> Vec<String>
where
    DB: HirDatabase + Sized,
{
    let Some(module) = Some(func.module(db)) else {
        return Vec::new();
    };
    // `Module::path_to_root` returns the enclosing module followed
    // by every parent, ending at the crate root.
    let mut stack: Vec<String> = module
        .path_to_root(db)
        .into_iter()
        .rev()
        .filter_map(|m| m.name(db))
        .map(|n| n.display_no_db(Edition::Edition2021).to_string())
        .collect();

    // Root Module::name returns None for the crate root; prepend the
    // crate segment explicitly. Key it off the PACKAGE name — not the bin
    // TARGET name `display_name` yields — so a `[[bin]]` target whose name
    // differs from its package produces the same qname prefix as the syn
    // extractor and cross-producer CALLS edges resolve (#517).
    let crate_name = crate::crate_name::crate_qname_prefix(db, func.krate(db));
    if !crate_name.is_empty() {
        // `path_to_root` does NOT include the crate root itself in
        // name-producing form; we insert it explicitly as element 0.
        stack.insert(0, crate_name);
    }
    stack
}
