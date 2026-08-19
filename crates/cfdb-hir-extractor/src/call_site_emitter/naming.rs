use cfdb_core::qname::{item_qname, method_qname, normalize_impl_target};
use ra_ap_edition::Edition;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{
    AsAssocItem, AssocItemContainer, DisplayTarget, Function, HasCrate, HirDisplay, Semantics,
};
use ra_ap_syntax::ast::{self, AstNode};
use ra_ap_syntax::SyntaxNode;

pub(super) fn enclosing_fn<DB>(sema: &Semantics<'_, DB>, node: &SyntaxNode) -> Option<Function>
where
    DB: HirDatabase + Sized,
{
    let fn_ast = node.ancestors().find_map(ast::Fn::cast)?;
    sema.to_def(&fn_ast)
}

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

    if let Some(assoc) = AsAssocItem::as_assoc_item(func, db) {
        let display_target = DisplayTarget::from_crate(db, func.krate(db).into());
        match assoc.container(db) {
            AssocItemContainer::Impl(impl_block) => {
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

fn build_module_stack<DB>(db: &DB, func: Function) -> Vec<String>
where
    DB: HirDatabase + Sized,
{
    let Some(module) = Some(func.module(db)) else {
        return Vec::new();
    };
    let mut stack: Vec<String> = module
        .path_to_root(db)
        .into_iter()
        .rev()
        .filter_map(|m| m.name(db))
        .map(|n| n.display_no_db(Edition::Edition2021).to_string())
        .collect();

    let crate_name = crate::crate_name::crate_qname_prefix(db, func.krate(db));
    if !crate_name.is_empty() {
        stack.insert(0, crate_name);
    }
    stack
}
