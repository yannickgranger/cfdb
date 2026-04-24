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

/// Closed list of standard-library wrapper types whose generic arg is
/// `render_type_inner`'s candidate set. Matching is by **last path
/// segment** (`std::vec::Vec<T>` and `Vec<T>` both match via `"Vec"`).
/// This table is audit-load-bearing per RFC-037 §6 / issue #239 — the
/// closed-list property means extending it is an RFC-gated change, not a
/// silent addition. Keep the 9 entries; do not grow this set in a PR
/// that is not a ratified RFC amendment.
const WRAPPER_TYPES: &[&str] = &[
    "Arc", "Box", "Cell", "Option", "Pin", "RefCell", "Rc", "Result", "Vec",
];

/// Unwrap one layer of standard-library wrapper generics around a
/// `syn::Type` and return the rendered inner candidate names. Used by
/// the post-walk RETURNS / TYPE_OF resolvers as the third match tier —
/// runs ONLY when exact-match and unique-last-segment fallback both
/// miss on the outer `render_type_string` output.
///
/// Matching is by last path segment against [`WRAPPER_TYPES`]. For
/// every `GenericArgument::Type(inner)` under the wrapper, both the
/// inner's own `render_type_string` output AND the recursive
/// `render_type_inner(&inner, depth - 1)` candidates are collected,
/// so a wrapper-around-non-wrapper (`Vec<Foo>`) yields `"Foo"` and a
/// nested wrapper (`Vec<Option<Foo>>`) recurses through the inner
/// unwrap. `Result<T, E>` yields both arms as independent candidates.
///
/// `depth` is a recursion budget. Each recursive call decrements it
/// by 1. A `depth == 0` call returns the empty Vec synchronously
/// without inspecting `ty`; this is the termination condition for
/// pathologically nested wrappers (`Vec<Vec<Vec<Vec<Foo>>>>` at
/// depth 3 exhausts the budget before reaching `Foo`). Callers in
/// the resolvers use `depth = 3` per RFC-037 §6 / issue #239.
///
/// Non-wrapper `Type::Path` (including bare generics like `T` and
/// user-defined types like `MyBox<Foo>`) returns empty Vec. Non-Path
/// `syn::Type` variants (`Reference`, `Tuple`, `Slice`, `Array`,
/// `TraitObject`, `ImplTrait`, `Paren`, `Infer`) return empty Vec —
/// references and tuples are explicit non-goals per the issue body.
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
mod render_type_inner_tests {
    //! Unit tests for [`super::render_type_inner`] — the closed-list
    //! wrapper-unwrap helper (issue #239, RFC-037 §6 closeout).
    //!
    //! Rules under test:
    //! - Wrapper match is by last path segment (`Vec`, `std::vec::Vec`
    //!   both match).
    //! - The closed [`super::WRAPPER_TYPES`] list refuses to unwrap
    //!   anything outside the 9 entries.
    //! - `depth` is a recursion budget; `depth == 0` returns empty
    //!   without inspecting the type; each recursion decrements by 1.
    //! - `Result<T, E>` yields both arms as independent candidates.
    //! - Non-`Type::Path` variants return empty.
    //!
    //! See also the integration-level scar flips in
    //! `tests/returns_emission.rs` and `tests/type_of_emission.rs`.
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
        // Vec<Vec<Vec<Vec<Foo>>>> — four wrapper levels. At depth 3:
        // outer Vec (depth 3, emits "Vec<Vec<Vec<Foo>>>" outer render +
        // recurse at depth 2) → Vec at depth 2 → Vec at depth 1 → Vec
        // at depth 0 returns empty. The leaf "Foo" is behind the
        // depth-0 wall and must not surface.
        let ty = parse_ty("Vec<Vec<Vec<Vec<Foo>>>>");
        let out = render_type_inner(&ty, 3);
        assert!(
            !out.contains(&"Foo".to_string()),
            "depth-4 nesting must drop leaf `Foo` at depth-3 budget, got {out:?}"
        );
    }

    #[test]
    fn bare_generic_t_yields_empty() {
        // `T` — trait-constrained generic. `seg.arguments` is
        // `PathArguments::None`, so no inner generic args to unwrap.
        // Also `T` is not in `WRAPPER_TYPES`. Either reason alone
        // must produce an empty result; the behaviour pins the
        // explicit non-goal at RFC-037 §2 / #239 non-goals.
        let ty = parse_ty("T");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.is_empty(),
            "bare generic `T` must yield no candidates, got {out:?}"
        );
    }

    #[test]
    fn user_defined_wrapper_yields_empty() {
        // `MyBox<Foo>` — `MyBox` is not in WRAPPER_TYPES. Unwrap
        // refuses, pinning the explicit non-goal "user-defined
        // wrappers require RFC addendum".
        let ty = parse_ty("MyBox<Foo>");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.is_empty(),
            "user-defined wrapper `MyBox<Foo>` must yield no candidates, got {out:?}"
        );
    }

    #[test]
    fn qualified_path_std_vec_vec_yields_foo_at_depth_3() {
        // Ambiguity D (last-segment matching): `std::vec::Vec<Foo>`
        // renders via its last path segment `"Vec"`. The helper
        // matches on `tp.path.segments.last().ident`, not on the full
        // rendered path, so qualified and unqualified wrappers both
        // unwrap the same way.
        let ty = parse_ty("std::vec::Vec<Foo>");
        let out = render_type_inner(&ty, 3);
        assert!(
            out.contains(&"Foo".to_string()),
            "qualified path `std::vec::Vec<Foo>` must still yield `Foo`, got {out:?}"
        );
    }

    #[test]
    fn depth_zero_returns_empty_synchronously() {
        // The termination condition. `depth == 0` returns empty Vec
        // without inspecting `ty` — the `ty` may be `Vec<Foo>` but
        // the budget is exhausted.
        let ty = parse_ty("Vec<Foo>");
        let out = render_type_inner(&ty, 0);
        assert!(
            out.is_empty(),
            "depth==0 must return empty without inspecting type, got {out:?}"
        );
    }

    #[test]
    fn non_path_type_reference_yields_empty() {
        // References and tuples are explicit non-goals. The helper
        // returns empty when the outer `syn::Type` is anything other
        // than `Type::Path`.
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
        // Closed-list invariant: the const table holds exactly the
        // 9 standard-library wrappers declared in the issue body.
        // Growing this list is an RFC-gated change; this test pins
        // the current membership so accidental additions flip a
        // unit test before they hit review.
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
