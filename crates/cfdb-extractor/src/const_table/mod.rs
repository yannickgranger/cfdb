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
//!
//! **Module split (#350).** Production code is partitioned into two
//! sibling modules to keep each file under the 500-LOC budget:
//!
//! - [`recognize`] — types ([`ElementType`], [`EntryValue`],
//!   [`RecognizedConstTable`]) and the [`recognize_const_table`] entry
//!   point. Pure values-in / values-out.
//! - [`canonical`] — wire-form helpers ([`entries_hash_hex`],
//!   [`entries_normalized_json`], [`entries_sample_json`],
//!   [`canonical_sorted_entries`]) that turn `Vec<EntryValue>` into the
//!   `:ConstTable` props.
//!
//! Re-exports below preserve the original `crate::const_table::Foo` paths
//! used by [`crate::item_visitor::emit`] and [`crate::item_visitor::visits`].

mod canonical;
mod recognize;

pub(crate) use canonical::{entries_hash_hex, entries_normalized_json, entries_sample_json};
pub(crate) use recognize::{recognize_const_table, RecognizedConstTable};
#[cfg(test)]
pub(crate) use recognize::{ElementType, EntryValue};

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
