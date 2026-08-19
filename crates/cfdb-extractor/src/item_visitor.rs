use cfdb_core::qname::{item_qname, module_qpath};
use cfdb_core::Visibility;

use crate::file_walker::PendingExternalMod;
use crate::Emitter;

mod emit;
mod visits;

#[cfg(test)]
mod parse_syn_visibility_tests;

pub(crate) use emit::emit_call_site_node_and_edge;

pub(crate) struct ItemVisitor<'e> {
    pub(crate) emitter: &'e mut Emitter,
    pub(crate) crate_id: String,
    pub(crate) crate_name: String,
    pub(crate) file_path: String,
    pub(crate) bounded_context: String,
    pub(crate) target: cfdb_core::qname::TargetDiscriminator,
    pub(crate) module_stack: Vec<String>,
    pub(crate) pending_external_mods: Vec<PendingExternalMod>,
    pub(crate) current_impl_target: Option<String>,
    pub(crate) test_mod_depth: u32,
}

fn impl_block_qname(module_stack: &[String], target: &str, trait_qname: Option<&str>) -> String {
    let module = module_qpath(module_stack);
    let prefix = if module.is_empty() {
        String::new()
    } else {
        format!("{module}::")
    };
    let trait_segment = trait_qname
        .map(|t| format!("_{}", t.replace("::", "_")))
        .unwrap_or_default();
    format!("{prefix}{target}::impl{trait_segment}")
}

fn impl_block_name(target: &str, trait_qname: Option<&str>) -> String {
    match trait_qname {
        Some(t) => format!("impl {t} for {target}"),
        None => format!("impl {target}"),
    }
}

fn resolve_target_qname(module_stack: &[String], type_or_trait: &str) -> String {
    if type_or_trait.contains("::") {
        return type_or_trait.to_string();
    }
    item_qname(module_stack, type_or_trait)
}

fn span_line(ident: &syn::Ident) -> usize {
    ident.span().start().line
}

fn parse_syn_visibility(vis: &syn::Visibility) -> Visibility {
    match vis {
        syn::Visibility::Public(_) => Visibility::Public,
        syn::Visibility::Inherited => Visibility::Private,
        syn::Visibility::Restricted(r) => {
            let segments: Vec<String> = r
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            let has_in = r.in_token.is_some();
            match (segments.len(), segments.first().map(String::as_str), has_in) {
                (1, Some("crate"), false) => Visibility::CrateLocal,
                (1, Some("super"), false) | (1, Some("self"), false) => Visibility::Module,
                _ => Visibility::Restricted(segments.join("::")),
            }
        }
    }
}
