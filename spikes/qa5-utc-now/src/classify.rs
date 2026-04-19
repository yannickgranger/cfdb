//! Sub-classification of raw rg-matched lines.
//!
//! Splits `Utc::now` occurrences into `Call` / `FnPtr` / `SerdeAttr` /
//! `StringLit` / `Comment` buckets. These subclass counts are diagnostic
//! only — the allocation logic in `main.rs` treats all non-comment
//! non-test lines as prod scope for recall measurement.

use regex::Regex;

pub fn is_test_path(p: &str) -> bool {
    p.contains("/tests/")
        || p.contains("/benches/")
        || p.contains("/examples/")
        || p.ends_with("_tests.rs")
        || p.ends_with("_test.rs")
        || p.ends_with("/tests.rs")
        || p.contains("/bdd_")
}

pub enum Subclass {
    Call,
    FnPtr,
    SerdeAttr,
    StringLit,
    Comment,
}

pub fn classify_line(content: &str) -> Subclass {
    // Comment: leading `//` or `///` or `//!` after trim.
    let stripped = content.trim_start();
    if stripped.starts_with("//") {
        return Subclass::Comment;
    }

    // fn-pointer references: .unwrap_or_else(Utc::now), .or_else(Utc::now), etc.
    // Matches `(chrono::)?Utc::now` NOT followed by `(` — i.e. symbol passed by
    // name, not invoked in place.
    let fn_ptr_re = Regex::new(
        r"\.(?:unwrap_or_else|or_else|map_or_else|get_or_insert_with|unwrap_or_default)\(\s*(?:chrono::)?Utc::now\s*[,\)]",
    )
    .unwrap();
    if fn_ptr_re.is_match(content) {
        return Subclass::FnPtr;
    }

    // #[serde(default = "Utc::now")] — attribute-based callback name.
    let serde_re = Regex::new(r#"#\[serde\([^\]]*default\s*=\s*"(?:chrono::)?Utc::now""#).unwrap();
    if serde_re.is_match(content) {
        return Subclass::SerdeAttr;
    }

    // Inside a string literal that is not the serde attr case.
    let in_string_re = Regex::new(r#""[^"]*Utc::now[^"]*""#).unwrap();
    if in_string_re.is_match(content) {
        return Subclass::StringLit;
    }

    Subclass::Call
}
