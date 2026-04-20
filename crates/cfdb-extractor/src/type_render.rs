//! Textual rendering of `syn::Type` and `syn::Path` into the searchable
//! form the extractor uses for `type_qname` field props and impl-target
//! qnames. This is deliberately shallow — full path resolution through
//! re-exports is RFC §8.2 Phase B (`ra-ap-hir`).

/// Render a `syn::Type` into the caller's `String` buffer. The output
/// is the minimal textual form: last-segment-joined paths, references
/// prefixed with `&` and optional lifetime, tuples parenthesised,
/// slices bracketed, arrays rendered as `[T; _]`, and anything else
/// replaced with `?`. Enough for searchable `type_qname` props.
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

/// Canonical convenience wrapper — allocate a fresh String and render into
/// it via the `render_type` primitive. Callers on a hot path that already
/// own a `String` buffer should call `render_type` directly to avoid the
/// allocation. Minimal rendering suitable for the extractor's searchable
/// `type_qname`; full path resolution is RFC §8.2 Phase B (ra-ap-hir).
pub(crate) fn render_type_string(ty: &syn::Type) -> String {
    let mut out = String::new();
    render_type(ty, &mut out);
    out
}

/// Render a `syn::Path` to its textual form (`a::b::c`). Last-segment only
/// when the path has exactly one segment.
pub(crate) fn render_path(p: &syn::Path) -> String {
    p.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Render a canonical fn signature string for a `syn::Signature`. The
/// output is the form `[const ][async ][unsafe ]fn(<params>) -> <ret>`,
/// where `<params>` is a comma-joined list of parameter TYPE renderings
/// (names omitted on purpose — parameter names are free to diverge across
/// bounded contexts without changing the semantic signature), and
/// `<ret>` is the rendered return type or `()` when the fn returns
/// nothing explicit.
///
/// Invariant (G1 byte-stability, RFC §6): the output is deterministic
/// across two runs on the same source — `syn::Signature::inputs` preserves
/// source order, and the modifier prefix order is fixed by this function.
/// Whitespace is normalized: single spaces separating modifiers from
/// `fn`, `", "` between parameters, `" -> "` before the return type, no
/// trailing or doubled whitespace.
///
/// Used by the `item_visitor` to populate `:Item.signature` on fn /
/// method kinds (issue #47). The rendered form is intentionally
/// LEXICALLY CANONICAL rather than semantically canonical: two `:Item`s
/// that have the same semantic signature but written with different
/// type paths (e.g. `Vec` vs `std::vec::Vec`) will produce different
/// strings. That is the point — the `signature_divergent` UDF picks up
/// those drifts as evidence of potential homonym / split-brain shapes.
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

/// Render one `syn::FnArg` as its TYPE contribution to the canonical
/// signature. Receivers (`&self`, `&mut self`, `self`) render as
/// `&Self`, `&mut Self`, or `Self` — the receiver shape is semantic
/// (governs dispatch) while the containing type is already encoded in
/// the qname, so keeping the receiver shape distinguishes `fn foo(&self)`
/// from `fn foo(&mut self)` without relitigating the receiver type.
/// Typed args render through `render_type`.
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
mod render_fn_signature_tests {
    use super::render_fn_signature;

    fn parse_sig(src: &str) -> syn::Signature {
        // Wrap in a dummy fn item so syn parses the modifiers + inputs
        // + output as one `syn::Signature`.
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
        // #47 core invariant: two :Item signatures must match when only
        // the parameter NAMES differ (names are caller-facing; they do
        // not change the fn's signature in the Rust trait / dispatch
        // sense).
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
        // Wrap in impl so &self parses.
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
        // G1 byte-stability — two calls on the same input produce the
        // same bytes. (The sig is cloned into a new parse each time so
        // no shared mutable state leaks.)
        let s1 = render_fn_signature(&parse_sig("fn f(a: &str, b: Vec<u8>) -> Option<bool>"));
        let s2 = render_fn_signature(&parse_sig("fn f(a: &str, b: Vec<u8>) -> Option<bool>"));
        assert_eq!(s1, s2);
    }
}
