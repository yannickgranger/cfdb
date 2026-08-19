use cfdb_core::CfgGate;
use syn::punctuated::Punctuated;
use syn::Token;

pub(crate) fn extract_serde_default_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut found: Option<String> = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                if let Ok(value) = meta.value() {
                    if let Ok(lit) = value.parse::<syn::LitStr>() {
                        found = Some(lit.value());
                    }
                }
            }
            Ok(())
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

pub(crate) fn extract_path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("path") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = &nv.value
            {
                return Some(lit_str.value());
            }
        }
    }
    None
}

pub(crate) fn attrs_contain_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let mut has_test = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                has_test = true;
                return Ok(());
            }
            if meta.path.is_ident("all") {
                let _ = meta.parse_nested_meta(|inner| {
                    if inner.path.is_ident("test") {
                        has_test = true;
                    }
                    Ok(())
                });
            }
            Ok(())
        });
        has_test
    })
}

pub(crate) fn attrs_contain_hash_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("test") {
            return false;
        }
        matches!(attr.meta, syn::Meta::Path(_))
    })
}

pub(crate) fn extract_deprecated_attr(attrs: &[syn::Attribute]) -> (bool, Option<String>) {
    let mut is_deprecated = false;
    let mut since: Option<String> = None;
    for attr in attrs {
        if !attr.path().is_ident("deprecated") {
            continue;
        }
        is_deprecated = true;
        if let syn::Meta::List(list) = &attr.meta {
            if let Some(v) = parse_deprecated_since(list) {
                since = Some(v);
            }
        }
    }
    (is_deprecated, since)
}

fn parse_deprecated_since(list: &syn::MetaList) -> Option<String> {
    let mut since: Option<String> = None;
    let _ = list.parse_nested_meta(|meta| {
        if !meta.path.is_ident("since") {
            return Ok(());
        }
        let value = meta.value()?;
        let lit: syn::LitStr = value.parse()?;
        since = Some(lit.value());
        Ok(())
    });
    since
}

pub(crate) fn extract_cfg_feature_gate(attrs: &[syn::Attribute]) -> Option<CfgGate> {
    let mut gates: Vec<CfgGate> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("cfg") {
            continue;
        }
        let Ok(inner) = attr.parse_args::<syn::Meta>() else {
            return None;
        };
        let gate = meta_to_feature_gate(&inner)?;
        gates.push(gate);
    }
    match gates.len() {
        0 => None,
        1 => Some(gates.into_iter().next().expect("len==1 so first() exists")),
        _ => Some(CfgGate::All(gates)),
    }
}

fn meta_to_feature_gate(meta: &syn::Meta) -> Option<CfgGate> {
    match meta {
        syn::Meta::NameValue(nv) if nv.path.is_ident("feature") => {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            {
                Some(CfgGate::Feature(s.value()))
            } else {
                None
            }
        }
        syn::Meta::List(list) if list.path.is_ident("all") => {
            let children = parse_meta_list_children(list)?;
            if children.is_empty() {
                None
            } else {
                Some(CfgGate::All(children))
            }
        }
        syn::Meta::List(list) if list.path.is_ident("any") => {
            let children = parse_meta_list_children(list)?;
            if children.is_empty() {
                None
            } else {
                Some(CfgGate::Any(children))
            }
        }
        syn::Meta::List(list) if list.path.is_ident("not") => {
            let mut children = parse_meta_list_children(list)?;
            if children.len() != 1 {
                None
            } else {
                Some(CfgGate::Not(Box::new(children.remove(0))))
            }
        }
        _ => None,
    }
}

fn parse_meta_list_children(list: &syn::MetaList) -> Option<Vec<CfgGate>> {
    let metas: Punctuated<syn::Meta, Token![,]> =
        list.parse_args_with(Punctuated::parse_terminated).ok()?;
    metas.iter().map(meta_to_feature_gate).collect()
}

#[cfg(test)]
mod tests;
