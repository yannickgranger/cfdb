pub(crate) fn render_type(ty: &syn::Type, out: &mut String) {
    match ty {
        syn::Type::Path(tp) => {
            let segs: Vec<String> = tp
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            out.push_str(&segs.join("::"));
        }
        syn::Type::Reference(r) => {
            out.push('&');
            if let Some(lt) = &r.lifetime {
                out.push('\'');
                out.push_str(&lt.ident.to_string());
                out.push(' ');
            }
            if r.mutability.is_some() {
                out.push_str("mut ");
            }
            render_type(&r.elem, out);
        }
        syn::Type::Tuple(t) => {
            out.push('(');
            for (i, elem) in t.elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                render_type(elem, out);
            }
            out.push(')');
        }
        syn::Type::Slice(s) => {
            out.push('[');
            render_type(&s.elem, out);
            out.push(']');
        }
        syn::Type::Array(a) => {
            out.push('[');
            render_type(&a.elem, out);
            out.push_str("; _]");
        }
        _ => {
            out.push('?');
        }
    }
}

pub(crate) fn render_type_string(ty: &syn::Type) -> String {
    let mut out = String::new();
    render_type(ty, &mut out);
    out
}

const WRAPPER_TYPES: &[&str] = &[
    "Arc", "Box", "Cell", "Option", "Pin", "RefCell", "Rc", "Result", "Vec",
];

pub(crate) fn render_type_inner(ty: &syn::Type, depth: u8) -> Vec<String> {
    if depth == 0 {
        return Vec::new();
    }
    let syn::Type::Path(tp) = ty else {
        return Vec::new();
    };
    let Some(seg) = tp.path.segments.last() else {
        return Vec::new();
    };
    let name = seg.ident.to_string();
    if !WRAPPER_TYPES.contains(&name.as_str()) {
        return Vec::new();
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for arg in &args.args {
        let syn::GenericArgument::Type(inner_ty) = arg else {
            continue;
        };
        out.push(render_type_string(inner_ty));
        out.extend(render_type_inner(inner_ty, depth - 1));
    }
    out
}

pub(crate) fn render_path(p: &syn::Path) -> String {
    p.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

pub(crate) fn render_fn_signature(sig: &syn::Signature) -> String {
    let mut out = String::new();
    if sig.constness.is_some() {
        out.push_str("const ");
    }
    if sig.asyncness.is_some() {
        out.push_str("async ");
    }
    if sig.unsafety.is_some() {
        out.push_str("unsafe ");
    }
    out.push_str("fn(");
    let mut first = true;
    for input in &sig.inputs {
        if !first {
            out.push_str(", ");
        }
        first = false;
        render_fn_arg(input, &mut out);
    }
    out.push_str(") -> ");
    match &sig.output {
        syn::ReturnType::Default => out.push_str("()"),
        syn::ReturnType::Type(_, ty) => render_type(ty, &mut out),
    }
    out
}

fn render_fn_arg(arg: &syn::FnArg, out: &mut String) {
    match arg {
        syn::FnArg::Receiver(r) => {
            if r.reference.is_some() {
                out.push('&');
                if r.mutability.is_some() {
                    out.push_str("mut ");
                }
            } else if r.mutability.is_some() {
                out.push_str("mut ");
            }
            out.push_str("Self");
        }
        syn::FnArg::Typed(pt) => {
            render_type(&pt.ty, out);
        }
    }
}

#[cfg(test)]
mod render_type_inner_tests {
    use super::render_type_inner;
    use super::WRAPPER_TYPES;

    fn parse_ty(src: &str) -> syn::Type {
        syn::parse_str::<syn::Type>(src).unwrap_or_else(|_| panic!("parse type: {src}"))
    }

    #[test]
    fn vec_of_foo_yields_foo_at_depth_3() {
        let ty = parse_ty("Vec<Foo>");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.contains(&"Foo".to_string()),
            "expected `Vec<Foo>` to yield `Foo` among candidates, got {out:?}"
        );
    }

    #[test]
    fn result_yields_both_arms_at_depth_3() {
        let ty = parse_ty("Result<Ok, Err>");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.contains(&"Ok".to_string()),
            "expected `Result<Ok, Err>` to yield `Ok` candidate, got {out:?}"
        );
        assert!(
            out.contains(&"Err".to_string()),
            "expected `Result<Ok, Err>` to yield `Err` candidate, got {out:?}"
        );
    }

    #[test]
    fn nested_vec_option_arc_foo_yields_foo_at_depth_3() {
        let ty = parse_ty("Vec<Option<Arc<Foo>>>");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.contains(&"Foo".to_string()),
            "expected `Vec<Option<Arc<Foo>>>` to yield `Foo` at depth 3, got {out:?}"
        );
    }

    #[test]
    fn depth_four_nesting_drops_leaf_foo() {
        let ty = parse_ty("Vec<Vec<Vec<Vec<Foo>>>>");
        let out = render_type_inner(&ty, 3);
        assert!(
            !out.contains(&"Foo".to_string()),
            "depth-4 nesting must drop leaf `Foo` at depth-3 budget, got {out:?}"
        );
    }

    #[test]
    fn bare_generic_t_yields_empty() {
        let ty = parse_ty("T");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.is_empty(),
            "bare generic `T` must yield no candidates, got {out:?}"
        );
    }

    #[test]
    fn user_defined_wrapper_yields_empty() {
        let ty = parse_ty("MyBox<Foo>");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.is_empty(),
            "user-defined wrapper `MyBox<Foo>` must yield no candidates, got {out:?}"
        );
    }

    #[test]
    fn qualified_path_std_vec_vec_yields_foo_at_depth_3() {
        let ty = parse_ty("std::vec::Vec<Foo>");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.contains(&"Foo".to_string()),
            "qualified path `std::vec::Vec<Foo>` must still yield `Foo`, got {out:?}"
        );
    }

    #[test]
    fn depth_zero_returns_empty_synchronously() {
        let ty = parse_ty("Vec<Foo>");
        let out = render_type_inner(&ty, 0);
        assert!(
            out.is_empty(),
            "depth==0 must return empty without inspecting type, got {out:?}"
        );
    }

    #[test]
    fn non_path_type_reference_yields_empty() {
        let ty = parse_ty("&Foo");
        let out = render_type_inner(&ty, 3);
        assert!(out.is_empty(), "reference types must yield empty");
    }

    #[test]
    fn non_path_type_tuple_yields_empty() {
        let ty = parse_ty("(Foo, Bar)");
        let out = render_type_inner(&ty, 3);
        assert!(out.is_empty(), "tuple types must yield empty");
    }

    #[test]
    fn wrapper_types_is_closed_nine_entries() {
        assert_eq!(
            WRAPPER_TYPES.len(),
            9,
            "WRAPPER_TYPES must remain the closed 9; got {} entries: {:?}",
            WRAPPER_TYPES.len(),
            WRAPPER_TYPES
        );
        for expected in &[
            "Arc", "Box", "Cell", "Option", "Pin", "RefCell", "Rc", "Result", "Vec",
        ] {
            assert!(
                WRAPPER_TYPES.contains(expected),
                "WRAPPER_TYPES missing `{expected}`"
            );
        }
    }
}

#[cfg(test)]
mod render_fn_signature_tests {
    use super::render_fn_signature;

    fn parse_sig(src: &str) -> syn::Signature {
        let wrapped = format!("{src} {{}}");
        let item: syn::ItemFn =
            syn::parse_str(&wrapped).unwrap_or_else(|_| panic!("parse fn: {src}"));
        item.sig
    }

    #[test]
    fn bare_fn_no_params_unit_return() {
        assert_eq!(render_fn_signature(&parse_sig("fn f()")), "fn() -> ()");
    }

    #[test]
    fn fn_with_params_renders_types_not_names() {
        assert_eq!(
            render_fn_signature(&parse_sig("fn f(a: i32, b: String) -> bool")),
            "fn(i32, String) -> bool"
        );
    }

    #[test]
    fn param_name_does_not_affect_signature() {
        let a = render_fn_signature(&parse_sig("fn f(value: i32) -> bool"));
        let b = render_fn_signature(&parse_sig("fn f(x: i32) -> bool"));
        assert_eq!(a, b);
    }

    #[test]
    fn return_type_divergence_surfaces_in_signature() {
        let a = render_fn_signature(&parse_sig("fn valuation() -> f64"));
        let b = render_fn_signature(&parse_sig("fn valuation() -> (f64, f64)"));
        assert_ne!(a, b);
    }

    #[test]
    fn async_unsafe_const_modifiers_are_prefixed() {
        assert_eq!(
            render_fn_signature(&parse_sig("async fn f()")),
            "async fn() -> ()"
        );
        assert_eq!(
            render_fn_signature(&parse_sig("unsafe fn f()")),
            "unsafe fn() -> ()"
        );
        assert_eq!(
            render_fn_signature(&parse_sig("const fn f()")),
            "const fn() -> ()"
        );
        assert_eq!(
            render_fn_signature(&parse_sig("const async unsafe fn f()")),
            "const async unsafe fn() -> ()"
        );
    }

    #[test]
    fn reference_receiver_renders_as_ref_self() {
        let item: syn::ItemImpl = syn::parse_str("impl X { fn m(&self) {} }").expect("parse impl");
        let syn::ImplItem::Fn(m) = &item.items[0] else {
            panic!("expected fn")
        };
        assert_eq!(render_fn_signature(&m.sig), "fn(&Self) -> ()");
    }

    #[test]
    fn mut_reference_receiver_renders_as_mut_ref_self() {
        let item: syn::ItemImpl =
            syn::parse_str("impl X { fn m(&mut self) {} }").expect("parse impl");
        let syn::ImplItem::Fn(m) = &item.items[0] else {
            panic!("expected fn")
        };
        assert_eq!(render_fn_signature(&m.sig), "fn(&mut Self) -> ()");
    }

    #[test]
    fn owned_receiver_renders_as_self() {
        let item: syn::ItemImpl =
            syn::parse_str("impl X { fn consume(self) {} }").expect("parse impl");
        let syn::ImplItem::Fn(m) = &item.items[0] else {
            panic!("expected fn")
        };
        assert_eq!(render_fn_signature(&m.sig), "fn(Self) -> ()");
    }

    #[test]
    fn deterministic_byte_stable_across_calls() {
        let s1 = render_fn_signature(&parse_sig("fn f(a: &str, b: Vec<u8>) -> Option<bool>"));
        let s2 = render_fn_signature(&parse_sig("fn f(a: &str, b: Vec<u8>) -> Option<bool>"));
        assert_eq!(s1, s2);
    }
}
