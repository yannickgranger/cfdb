//! Recognizer + canonicalization helpers for `:ConstTable` (RFC-040).
//!
//! The recognizer ([`recognize_const_table`]) is a pure values-in / values-out
//! function over [`syn::ItemConst`]; canonicalization helpers
//! ([`entries_hash_hex`], [`entries_normalized_json`], [`entries_sample_json`])
//! turn the recognizer's output into the wire props the extractor emits.
//! The visitor ([`crate::item_visitor::ItemVisitor::emit_const_table`])
//! orchestrates the recognizer + canonicalizers and writes the
//! `:Item -[:HAS_CONST_TABLE]-> :ConstTable` shape.
//!
//! A const is a recognized candidate iff BOTH:
//!
//! 1. `node.ty` is a literal slice/array of supported element type
//!    (RFC §3.3 — `&[T]`, `&'static [T]`, `[T; N]`, `&[T; N]`,
//!    `&'static [T; N]`). Reference lifetime is ignored — both `&[T]` and
//!    `&'static [T]` are accepted (R2 carried rust-systems N1).
//! 2. `node.expr` is a literal array expression with every element parsing
//!    as a literal of the matching element type.
//!
//! Element types in v0.1: [`ElementType::Str`] (i.e. `&str`),
//! [`ElementType::U32`], [`ElementType::I32`], [`ElementType::U64`],
//! [`ElementType::I64`]. Anything else (booleans, custom types, nested
//! arrays, non-literal expressions) is non-recognized — only the parent
//! `:Item` will be emitted by the visitor.

use sha2::{Digest, Sha256};
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

// ---------------------------------------------------------------------------
// Canonicalization helpers (RFC-040 §3.1).
//
// `entries_hash`, `entries_normalized`, and `entries_sample` are derived from
// the recognizer's `entries: Vec<EntryValue>` (declaration order). The visitor
// calls these to build the wire props.
// ---------------------------------------------------------------------------

/// Sort a copy of `entries` ascending — lexicographic for `Str`, numeric for
/// `Num`. Two consts with the same set but different declaration order
/// produce identical sorted output (the structural-equality key for the
/// overlap detector, RFC §3.4).
pub(crate) fn canonical_sorted_entries(entries: &[EntryValue]) -> Vec<EntryValue> {
    let mut sorted = entries.to_vec();
    // EntryValue derives Ord through its variants — but the recognizer
    // guarantees the slice is homogeneous (one element_type), so we can
    // sort with a variant-aware comparator without ever crossing the
    // Str/Num boundary. The match below makes the homogeneity assumption
    // explicit.
    sorted.sort_by(|a, b| match (a, b) {
        (EntryValue::Str(x), EntryValue::Str(y)) => x.cmp(y),
        (EntryValue::Num(x), EntryValue::Num(y)) => x.cmp(y),
        // Mixed-variant slices are an extractor invariant violation —
        // the recognizer rejects them in `parse_literal`. Falling back to
        // declaration order keeps sort stable; the panic-free path
        // matches the rest of the extractor's resilience model.
        _ => std::cmp::Ordering::Equal,
    });
    sorted
}

/// sha256 hex (lowercase) over the canonical-sorted entry sequence.
///
/// Encoding per RFC §3.1:
/// - `Str` entries: join with `\0` (NUL never appears in a Rust `&str`
///   literal under syn parsing — safe separator that does not require
///   escaping).
/// - `Num` entries: write each in decimal (no leading zeros, no underscores,
///   no thousands separators), join with `\n`.
///
/// Two consts with the same set produce the same hash regardless of
/// declaration order — this is the structural-equality key for the
/// `const-table-overlap.cypher` detector (RFC §3.4).
pub(crate) fn entries_hash_hex(entries: &[EntryValue]) -> String {
    let sorted = canonical_sorted_entries(entries);
    let mut hasher = Sha256::new();
    let bytes = match sorted.first() {
        // Empty entries → hash the empty sequence; sha256("") is a
        // well-defined fixed string. Either separator would produce the
        // same empty input, so the encoding choice is moot here.
        None => Vec::new(),
        Some(EntryValue::Str(_)) => join_str_entries_nul(&sorted),
        Some(EntryValue::Num(_)) => join_num_entries_newline(&sorted),
    };
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        // hex formatting is infallible into a String — `write!` returns
        // `Ok(())` for strings; the result is dropped intentionally.
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn join_str_entries_nul(sorted: &[EntryValue]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, e) in sorted.iter().enumerate() {
        if i > 0 {
            out.push(0u8);
        }
        if let EntryValue::Str(s) = e {
            out.extend_from_slice(s.as_bytes());
        }
    }
    out
}

fn join_num_entries_newline(sorted: &[EntryValue]) -> Vec<u8> {
    let mut out = String::new();
    for (i, e) in sorted.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if let EntryValue::Num(n) = e {
            use std::fmt::Write;
            // i128 Display is canonical decimal — no leading zeros, no
            // underscores. Matches RFC §3.1 wire commitment.
            let _ = write!(&mut out, "{n}");
        }
    }
    out.into_bytes()
}

/// JSON array of the canonical-sorted entries — same byte order used by
/// `entries_hash_hex`. Permanent wire commitment per RFC §3.1: producers
/// re-emit byte-identical normalization across builds; consumers may rely
/// on `JSON.parse` returning a flat array of strings (for `Str`) or
/// integers (for `Num`).
pub(crate) fn entries_normalized_json(entries: &[EntryValue]) -> String {
    let sorted = canonical_sorted_entries(entries);
    encode_entries_json(&sorted)
}

/// JSON array of the FIRST 8 entries in DECLARATION order (no sort, no
/// truncation indicator beyond the natural cap). Triage aid only — two
/// consts with the same set but different declaration order produce
/// divergent samples, which is informational, not a correctness signal
/// (RFC §3.1).
pub(crate) fn entries_sample_json(entries: &[EntryValue]) -> String {
    const SAMPLE_CAP: usize = 8;
    let take = entries.len().min(SAMPLE_CAP);
    encode_entries_json(&entries[..take])
}

fn encode_entries_json(entries: &[EntryValue]) -> String {
    // Build a serde_json::Value array so the encoder handles all string
    // escaping (quotes, backslashes, control chars) per the JSON spec.
    let value = serde_json::Value::Array(
        entries
            .iter()
            .map(|e| match e {
                EntryValue::Str(s) => serde_json::Value::String(s.clone()),
                EntryValue::Num(n) => {
                    // i128 does not impl Into<serde_json::Number>; the
                    // recognizer guarantees the value fits in i128, but
                    // serde_json's number type is bounded by i64/u64/f64.
                    // For values that exceed i64::MAX (i.e. u64 entries
                    // above 2^63-1), encode as a string to preserve the
                    // exact decimal — JSON.parse on the consumer side
                    // will handle either; the wire commitment is "decimal
                    // representation", not "numeric JSON token".
                    if let Ok(n64) = i64::try_from(*n) {
                        serde_json::Value::Number(n64.into())
                    } else if *n >= 0 && *n <= u64::MAX as i128 {
                        serde_json::Value::Number((*n as u64).into())
                    } else {
                        serde_json::Value::String(n.to_string())
                    }
                }
            })
            .collect(),
    );
    serde_json::to_string(&value).unwrap_or_else(|_| String::from("[]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_const(src: &str) -> syn::ItemConst {
        syn::parse_str::<syn::ItemConst>(src).expect("test fixture must be valid Rust const")
    }

    fn recognize(src: &str) -> Option<RecognizedConstTable> {
        // `module_qpath` follows the descriptor convention — fully-qualified
        // path of the enclosing module, which already includes the crate
        // segment.
        recognize_const_table(&parse_const(src), "kraken", "kraken::normalize", false)
    }

    // ---- ElementType wire-string contract -----------------------------------

    #[test]
    fn element_type_wire_strings_are_exhaustive_and_canonical() {
        assert_eq!(ElementType::Str.as_wire_str(), "str");
        assert_eq!(ElementType::U32.as_wire_str(), "u32");
        assert_eq!(ElementType::I32.as_wire_str(), "i32");
        assert_eq!(ElementType::U64.as_wire_str(), "u64");
        assert_eq!(ElementType::I64.as_wire_str(), "i64");
        // Sanity: every variant maps to a distinct, non-empty wire string.
        let all = [
            ElementType::Str.as_wire_str(),
            ElementType::U32.as_wire_str(),
            ElementType::I32.as_wire_str(),
            ElementType::U64.as_wire_str(),
            ElementType::I64.as_wire_str(),
        ];
        for s in all {
            assert!(!s.is_empty(), "wire string must be non-empty");
        }
        let unique: std::collections::HashSet<_> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len(), "wire strings must be unique");
    }

    // ---- Positive recognition: str ------------------------------------------

    #[test]
    fn recognizes_ref_slice_of_str() {
        let r = recognize(r#"const Z: &[&str] = &["a", "b"];"#).expect("recognized");
        assert_eq!(r.element_type, ElementType::Str);
        assert_eq!(
            r.entries,
            vec![EntryValue::Str("a".into()), EntryValue::Str("b".into()),],
        );
        assert_eq!(r.qname, "kraken::normalize::Z");
        assert_eq!(r.name, "Z");
        assert_eq!(r.crate_name, "kraken");
        assert_eq!(r.module_qpath, "kraken::normalize");
        assert!(!r.is_test);
    }

    #[test]
    fn recognizes_static_slice_of_str() {
        let r = recognize(r#"const Z: &'static [&str] = &["a"];"#).expect("recognized");
        assert_eq!(r.element_type, ElementType::Str);
        assert_eq!(r.entries, vec![EntryValue::Str("a".into())]);
    }

    #[test]
    fn recognizes_array_of_str() {
        let r = recognize(r#"const Z: [&str; 2] = ["a", "b"];"#).expect("recognized array literal");
        assert_eq!(r.element_type, ElementType::Str);
        assert_eq!(r.entries.len(), 2);
    }

    #[test]
    fn recognizes_ref_array_of_str() {
        let r = recognize(r#"const Z: &[&str; 2] = &["a", "b"];"#).expect("recognized");
        assert_eq!(r.element_type, ElementType::Str);
        assert_eq!(r.entries.len(), 2);
    }

    #[test]
    fn recognizes_static_ref_array_of_str() {
        let r = recognize(r#"const Z: &'static [&str; 1] = &["a"];"#).expect("recognized");
        assert_eq!(r.element_type, ElementType::Str);
    }

    // ---- Positive recognition: numeric --------------------------------------

    #[test]
    fn recognizes_ref_slice_of_u32() {
        let r = recognize("const Z: &[u32] = &[1, 2, 3];").expect("recognized");
        assert_eq!(r.element_type, ElementType::U32);
        assert_eq!(
            r.entries,
            vec![EntryValue::Num(1), EntryValue::Num(2), EntryValue::Num(3)],
        );
    }

    #[test]
    fn recognizes_array_of_u32() {
        let r = recognize("const Z: [u32; 2] = [1, 2];").expect("recognized");
        assert_eq!(r.element_type, ElementType::U32);
    }

    #[test]
    fn recognizes_ref_slice_of_i32() {
        // i32 with non-negative literals is recognized; signed-negative
        // literals are covered by `i64_unary_negative_literals_are_rejected_as_expected`.
        let r = recognize("const Z: &[i32] = &[0, 1, 2];").expect("recognized");
        assert_eq!(r.element_type, ElementType::I32);
        assert_eq!(r.entries.len(), 3);
    }

    #[test]
    fn recognizes_ref_slice_of_u64() {
        let r = recognize("const Z: &[u64] = &[10, 20];").expect("recognized");
        assert_eq!(r.element_type, ElementType::U64);
    }

    #[test]
    fn recognizes_ref_slice_of_i64() {
        let r = recognize("const Z: &[i64] = &[10, 20];").expect("recognized");
        assert_eq!(r.element_type, ElementType::I64);
    }

    // ---- i128 numeric range -------------------------------------------------

    #[test]
    fn parses_u64_max_without_overflow() {
        // u64::MAX = 18446744073709551615 fits in i128.
        let r = recognize("const Z: &[u64] = &[18446744073709551615u64, 0];").expect("recognized");
        assert_eq!(r.entries[0], EntryValue::Num(u64::MAX as i128));
        assert_eq!(r.entries[1], EntryValue::Num(0));
    }

    #[test]
    fn suffix_stripped_int_literals_parse_identically_to_bare() {
        let bare = recognize("const Z: &[u64] = &[42];").expect("bare literal");
        let suffixed = recognize("const Z: &[u64] = &[42u64];").expect("suffixed literal");
        assert_eq!(bare.entries, suffixed.entries);
        assert_eq!(bare.entries, vec![EntryValue::Num(42)]);
    }

    #[test]
    fn i64_unary_negative_literals_are_rejected_as_expected() {
        // `-1` parses as Expr::Unary(Neg, Lit(1)), not Expr::Lit. The
        // recognizer requires Expr::Lit per RFC §3.3, so a const containing
        // a unary-prefixed integer falls back to "non-recognized — only the
        // parent :Item is emitted". This is the documented v0.1 limitation;
        // upgrading to constant-fold the unary prefix is a follow-up slice.
        assert!(recognize("const Z: &[i64] = &[-1, 0];").is_none());
    }

    // ---- Negative cases ------------------------------------------------------

    #[test]
    fn rejects_slice_of_bool() {
        assert!(recognize("const Z: &[bool] = &[true, false];").is_none());
    }

    #[test]
    fn rejects_slice_of_tuple() {
        assert!(recognize("const Z: &[(u32, u32)] = &[(1, 2)];").is_none());
    }

    #[test]
    fn rejects_slice_of_custom_type() {
        assert!(recognize("const Z: &[CustomType] = &[];").is_none());
    }

    #[test]
    fn rejects_non_literal_expression() {
        // `EMPTY_SLICE` is a path expression, not an array literal.
        assert!(recognize("const Z: &[&str] = EMPTY_SLICE;").is_none());
    }

    #[test]
    fn rejects_scalar_const() {
        assert!(recognize("const Z: u32 = 7;").is_none());
    }

    #[test]
    fn rejects_qualified_path_element_type() {
        // Fully-qualified `core::primitive::u32` is rejected — RFC §3.3
        // commits the recognizer to a single-segment path check.
        assert!(recognize("const Z: &[core::primitive::u32] = &[1];").is_none());
    }

    #[test]
    fn rejects_mixed_literal_kinds() {
        // Numeric type with a string literal inside.
        assert!(recognize(r#"const Z: &[u32] = &[1, "two"];"#).is_none());
    }

    // ---- qname / module_qpath construction ----------------------------------

    #[test]
    fn empty_module_qpath_omits_separator_segment() {
        let node = parse_const(r#"const Z: &[&str] = &["a"];"#);
        let r = recognize_const_table(&node, "kraken", "", false).expect("recognized");
        assert_eq!(r.qname, "kraken::Z");
        assert!(r.module_qpath.is_empty());
    }

    #[test]
    fn is_test_flag_propagates_through_unchanged() {
        let node = parse_const(r#"const Z: &[&str] = &["a"];"#);
        let r = recognize_const_table(&node, "k", "m", true).expect("recognized");
        assert!(r.is_test);
    }

    // ---- Canonicalization: entries_hash_hex ---------------------------------

    #[test]
    fn entries_hash_is_order_invariant_for_strings() {
        let a = vec![
            EntryValue::Str("c".into()),
            EntryValue::Str("a".into()),
            EntryValue::Str("b".into()),
        ];
        let b = vec![
            EntryValue::Str("a".into()),
            EntryValue::Str("b".into()),
            EntryValue::Str("c".into()),
        ];
        assert_eq!(entries_hash_hex(&a), entries_hash_hex(&b));
    }

    #[test]
    fn entries_hash_is_order_invariant_for_numbers() {
        let a = vec![EntryValue::Num(3), EntryValue::Num(1), EntryValue::Num(2)];
        let b = vec![EntryValue::Num(1), EntryValue::Num(2), EntryValue::Num(3)];
        assert_eq!(entries_hash_hex(&a), entries_hash_hex(&b));
    }

    #[test]
    fn entries_hash_distinguishes_supersets() {
        let small = vec![EntryValue::Str("a".into()), EntryValue::Str("b".into())];
        let big = vec![
            EntryValue::Str("a".into()),
            EntryValue::Str("b".into()),
            EntryValue::Str("c".into()),
        ];
        assert_ne!(entries_hash_hex(&small), entries_hash_hex(&big));
    }

    #[test]
    fn entries_hash_is_lowercase_hex_64_chars() {
        let h = entries_hash_hex(&[EntryValue::Str("a".into())]);
        assert_eq!(h.len(), 64, "sha256 hex is 64 chars");
        assert!(
            h.chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()),
            "hash must be lowercase hex: {h}"
        );
    }

    #[test]
    fn entries_hash_uses_nul_separator_for_strings() {
        // "a\0b" must hash differently from "ab" — proves NUL is used as a
        // separator, not a no-op. sha256("a\0b") ≠ sha256("ab").
        let split = entries_hash_hex(&[EntryValue::Str("a".into()), EntryValue::Str("b".into())]);
        let joined = entries_hash_hex(&[EntryValue::Str("ab".into())]);
        assert_ne!(split, joined);
    }

    // ---- Canonicalization: entries_normalized_json --------------------------

    #[test]
    fn entries_normalized_is_sorted_string_array() {
        let json = entries_normalized_json(&[
            EntryValue::Str("zeta".into()),
            EntryValue::Str("alpha".into()),
        ]);
        assert_eq!(json, r#"["alpha","zeta"]"#);
    }

    #[test]
    fn entries_normalized_is_sorted_number_array() {
        let json = entries_normalized_json(&[
            EntryValue::Num(42),
            EntryValue::Num(7),
            EntryValue::Num(13),
        ]);
        assert_eq!(json, "[7,13,42]");
    }

    #[test]
    fn entries_normalized_escapes_special_string_characters() {
        let json = entries_normalized_json(&[EntryValue::Str("a\"b\\c".into())]);
        // serde_json escapes both `"` and `\` per JSON spec.
        assert_eq!(json, r#"["a\"b\\c"]"#);
    }

    #[test]
    fn entries_normalized_emits_u64_max_as_decimal() {
        let json = entries_normalized_json(&[EntryValue::Num(u64::MAX as i128)]);
        // u64::MAX exceeds i64::MAX → encoded as a JSON number via the
        // u64 path. JSON.parse returns a number; consumers treat the wire
        // commitment as "decimal representation".
        assert_eq!(json, format!("[{}]", u64::MAX));
    }

    // ---- Canonicalization: entries_sample_json ------------------------------

    #[test]
    fn entries_sample_preserves_declaration_order() {
        let json = entries_sample_json(&[
            EntryValue::Str("zeta".into()),
            EntryValue::Str("alpha".into()),
            EntryValue::Str("beta".into()),
        ]);
        // Sample MUST NOT sort — the divergent declaration is the triage
        // signal.
        assert_eq!(json, r#"["zeta","alpha","beta"]"#);
    }

    #[test]
    fn entries_sample_caps_at_eight_entries() {
        let entries: Vec<_> = (0..20).map(EntryValue::Num).collect();
        let json = entries_sample_json(&entries);
        assert_eq!(json, "[0,1,2,3,4,5,6,7]");
    }

    #[test]
    fn entries_sample_emits_full_array_when_under_cap() {
        let entries = vec![EntryValue::Num(1), EntryValue::Num(2), EntryValue::Num(3)];
        let json = entries_sample_json(&entries);
        assert_eq!(json, "[1,2,3]");
    }

    #[test]
    fn entries_normalized_empty_set_is_well_formed() {
        let json = entries_normalized_json(&[]);
        assert_eq!(json, "[]");
        // sha256 of empty input — the well-defined fixed hash.
        let h = entries_hash_hex(&[]);
        assert_eq!(
            h,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
    }
}
