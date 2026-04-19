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
