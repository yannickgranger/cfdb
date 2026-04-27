//! Recognizer half of [`crate::const_table`]. Pure values-in / values-out
//! function over [`syn::ItemConst`] — no I/O, no allocation beyond the
//! returned [`RecognizedConstTable`]. The companion module
//! [`super::canonical`] turns the recognizer's output into wire props.
//!
//! Split out of `const_table.rs` (#350) to keep each file under the 500-LOC
//! budget. Public surface is unchanged — every item visible to the rest of
//! the crate is re-exported from [`super`].

use syn::{
    Expr, ExprArray, ExprLit, ExprReference, Lit, Type, TypeArray, TypeReference, TypeSlice,
};

/// Closed-set wire vocabulary owner (RFC-038 §3.1 invariant-owner pattern).
///
/// The producer's wire string for the `:ConstTable.element_type` attribute
/// flows through [`ElementType::as_wire_str`] only. Constructing the wire
/// string inline elsewhere (`PropValue::Str("u32".into())` instead of
/// `PropValue::Str(ty.as_wire_str().into())`) is the producer-side
/// split-brain shape that R1 solid-architect B2 flagged; this enum is the
/// single owner.
///
/// Adding a sixth variant is RFC-gated (no-ratchet rule, RFC-040 §4) — the
/// describer's documented wire vocabulary
/// `{"str", "u32", "i32", "u64", "i64"}` and this enum must be expanded
/// together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementType {
    Str,
    U32,
    I32,
    U64,
    I64,
}

impl ElementType {
    /// Wire-string canonical owner. Producers MUST construct
    /// `PropValue::Str` for `element_type` via this method — never inline a
    /// raw literal.
    pub(crate) fn as_wire_str(&self) -> &'static str {
        match self {
            ElementType::Str => "str",
            ElementType::U32 => "u32",
            ElementType::I32 => "i32",
            ElementType::U64 => "u64",
            ElementType::I64 => "i64",
        }
    }
}

/// One literal entry inside a recognized const table.
///
/// `i128` covers the v0.1 supported integer range exactly: `i128::MAX =
/// 2^127 − 1 > u64::MAX`, `i128::MIN = −2^127 < i64::MIN`. There is no
/// silent overflow when parsing `u64::MAX` written as
/// `18446744073709551615u64`. R2 carried rust-systems N2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EntryValue {
    Str(String),
    Num(i128),
}

/// Output of [`recognize_const_table`]. The visitor builds the wire prop
/// map from this — `qname` becomes `:ConstTable.qname`, `entries` is
/// canonicalized into `entries_hash`/`entries_normalized`/`entries_sample`
/// per RFC §3.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecognizedConstTable {
    pub(crate) qname: String,
    pub(crate) name: String,
    pub(crate) crate_name: String,
    pub(crate) module_qpath: String,
    pub(crate) element_type: ElementType,
    /// Declaration-order entries. The visitor sorts a copy to compute
    /// `entries_hash` and `entries_normalized`; this field preserves the
    /// original order for `entries_sample`.
    pub(crate) entries: Vec<EntryValue>,
    pub(crate) is_test: bool,
}

/// Recognize whether `node` is a `:ConstTable` candidate. Returns `None`
/// when the type is not a supported slice/array shape, when any entry is
/// non-literal, or when an integer literal does not fit in `i128`.
///
/// `is_test` is supplied by the caller — the visitor sources it from
/// `self.is_in_test_mod()` so the recognizer stays a pure values-in /
/// values-out function (the [`syn::ItemConst`] alone carries no info about
/// ancestor modules).
pub(crate) fn recognize_const_table(
    node: &syn::ItemConst,
    crate_name: &str,
    module_qpath: &str,
    is_test: bool,
) -> Option<RecognizedConstTable> {
    let element_type = element_type_of(&node.ty)?;
    let entries = entries_from_expr(&node.expr, element_type)?;
    let name = node.ident.to_string();
    let qname = build_qname(crate_name, module_qpath, &name);
    Some(RecognizedConstTable {
        qname,
        name,
        crate_name: crate_name.to_string(),
        module_qpath: module_qpath.to_string(),
        element_type,
        entries,
        is_test,
    })
}

/// Walk the outer type to its inner element type and classify it.
///
/// Accepted shapes:
/// - `&[T]` / `&'static [T]` → strip reference, then strip slice
/// - `[T; N]` → strip array
/// - `&[T; N]` / `&'static [T; N]` → strip reference, then strip array
fn element_type_of(ty: &Type) -> Option<ElementType> {
    let inner = match ty {
        Type::Reference(TypeReference { elem, .. }) => match elem.as_ref() {
            Type::Slice(TypeSlice { elem: inner, .. })
            | Type::Array(TypeArray { elem: inner, .. }) => inner.as_ref(),
            _ => return None,
        },
        Type::Array(TypeArray { elem: inner, .. }) => inner.as_ref(),
        _ => return None,
    };
    classify_element(inner)
}

/// Classify the leaf type. `&str` is the only allowed reference form;
/// numeric leaves are bare `Type::Path` segments.
fn classify_element(ty: &Type) -> Option<ElementType> {
    if let Type::Reference(TypeReference { elem, .. }) = ty {
        if path_is_ident(elem.as_ref(), "str") {
            return Some(ElementType::Str);
        }
        return None;
    }
    if path_is_ident(ty, "u32") {
        return Some(ElementType::U32);
    }
    if path_is_ident(ty, "i32") {
        return Some(ElementType::I32);
    }
    if path_is_ident(ty, "u64") {
        return Some(ElementType::U64);
    }
    if path_is_ident(ty, "i64") {
        return Some(ElementType::I64);
    }
    None
}

/// True iff `ty` is a single-segment path matching `ident`. Rejects
/// fully-qualified paths (e.g. `core::primitive::u32`) — this matches the
/// RFC §3.3 commitment that the recognizer is a textual/syntactic check.
fn path_is_ident(ty: &Type, ident: &str) -> bool {
    if let Type::Path(p) = ty {
        if p.qself.is_none() && p.path.segments.len() == 1 {
            return p.path.segments[0].ident == ident;
        }
    }
    false
}

/// Strip the optional outer `&` from the expression and require an array
/// literal `[a, b, c]`. Each element must be a literal of the type
/// matching `expected`.
fn entries_from_expr(expr: &Expr, expected: ElementType) -> Option<Vec<EntryValue>> {
    let array = match expr {
        Expr::Reference(ExprReference { expr: inner, .. }) => match inner.as_ref() {
            Expr::Array(a) => a,
            _ => return None,
        },
        Expr::Array(a) => a,
        _ => return None,
    };
    parse_literal_entries(array, expected)
}

fn parse_literal_entries(array: &ExprArray, expected: ElementType) -> Option<Vec<EntryValue>> {
    let mut out = Vec::with_capacity(array.elems.len());
    for elem in &array.elems {
        let lit = match elem {
            Expr::Lit(ExprLit { lit, .. }) => lit,
            _ => return None,
        };
        out.push(parse_literal(lit, expected)?);
    }
    Some(out)
}

fn parse_literal(lit: &Lit, expected: ElementType) -> Option<EntryValue> {
    match (expected, lit) {
        (ElementType::Str, Lit::Str(s)) => Some(EntryValue::Str(s.value())),
        (ElementType::U32, Lit::Int(n))
        | (ElementType::I32, Lit::Int(n))
        | (ElementType::U64, Lit::Int(n))
        | (ElementType::I64, Lit::Int(n)) => {
            // base10_parse strips type suffix (`42u64` → `42`) before
            // parsing digits. R2 absorbed rust-systems N2.
            n.base10_parse::<i128>().ok().map(EntryValue::Num)
        }
        _ => None,
    }
}

fn build_qname(crate_name: &str, module_qpath: &str, name: &str) -> String {
    // `module_qpath` matches the descriptor convention shared with
    // `:Item.module_qpath` — the FULLY-QUALIFIED path of the enclosing
    // module, which already includes the crate segment (e.g. `kraken` at
    // crate root, `kraken::normalize` for a child module). The empty
    // string is a degenerate fallback used by unit tests that bypass the
    // visitor; in that case fall back to `{crate}::{name}`.
    if module_qpath.is_empty() {
        format!("{crate_name}::{name}")
    } else {
        format!("{module_qpath}::{name}")
    }
}
