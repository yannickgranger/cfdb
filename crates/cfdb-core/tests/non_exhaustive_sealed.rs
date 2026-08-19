#[test]
fn exhaustive_match_without_wildcard_fails_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/propvalue_no_wildcard.rs");
}
