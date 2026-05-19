//! Compile-fail tests verifying `#[non_exhaustive]` annotation on cfdb-core
//! schema enums forces external-crate match sites to include a wildcard arm.
//!
//! RFC-044 §3.7 (slice 044-G) — the trybuild fixture in `tests/ui/` matches
//! cfdb-core's `PropValue` exhaustively without a wildcard from outside the
//! crate; under `#[non_exhaustive]` this MUST fail to compile.

#[test]
fn exhaustive_match_without_wildcard_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/propvalue_no_wildcard.rs");
}
