//! Source-side ground truth for the `enrich-deprecation` dogfood.
//!
//! Counts occurrences of the bare single-segment `#[deprecated]`
//! attribute (in any of its three accepted forms — bare path,
//! `note = "..."`, `since = "X.Y.Z"`) at **attribute position in real
//! code**: comments and string/char literals are lexically stripped
//! before the regex runs, and only files the extractor actually walked
//! (the keyspace's `:File` node set, supplied by the caller) are
//! counted. Multi-segment paths like `#[serde(deprecated = "true")]`
//! are deliberately not counted, mirroring the discipline in
//! [`cfdb_extractor::attrs::extract_deprecated_attr`] (per RFC-039
//! §3.1 `enrich-deprecation` row).
//!
//! The harness substitutes the count into the
//! `.cfdb/queries/self-enrich-deprecation.cypher` template's
//! `{{ ground_truth_count }}` placeholder. The Cypher sentinel then
//! compares it against `count(:Item WHERE is_deprecated = true)`. A
//! drop in the extracted count surfaces as a violation row.
//!
//! ## Why comment/string stripping is load-bearing
//!
//! The original helper (issue #343) ran the regex over raw file text
//! across a full workspace walk. That overcounted massively: at the
//! PR #563 diagnosis, raw-text matches on cfdb-self stood at 73 while
//! genuine attribute-position occurrences in extractor-walked files
//! stood at exactly 1 — the other 72 lived in doc comments, string
//! literals (this very file's tests), and fixture crates the extractor
//! never walks (`examples/queries/fixtures/`, `crates/*/tests/`
//! integration-test targets). The mismatch was masked by the
//! count()-over-empty-match evaluator bug (#564) until cfdb-self
//! gained its first real `#[deprecated]` item.
//!
//! ## Scoping contract (extractor-walked files only)
//!
//! The caller passes the workspace-relative file list from the
//! keyspace's `:File` nodes ([`crate::extracted_files`]). The sentinel
//! therefore asserts: *within the files the extractor claims to have
//! walked, every attribute-position `#[deprecated]` is reflected in
//! the graph*. A file the extractor wrongly skips entirely shrinks
//! both sides of the comparison in lockstep and is invisible here —
//! accepted residual risk: the only current net for that class is the
//! nightly `cfdb-recall` ratio check (threshold-based and not yet a
//! PR-blocking required context), and re-deriving the walk in this
//! harness would duplicate exactly the resolution logic under test.
//!
//! Second known granularity gap (#565): `#[deprecated]` on a struct
//! field or enum variant is counted here (attribute position in a
//! walked file) but the extractor only emits `is_deprecated` on
//! `:Item` — `:Field`/`:Variant` cannot carry it. The first such
//! attribute added to a walked tree turns this gate RED with no
//! extractor regression (fails closed). Resolution is RFC-gated
//! schema work tracked in #565.
//!
//! ## Why text-lex, not syn-parse
//!
//! RFC-039 §3.5.1 keeps `dogfood-enrich` a small leaf — no `syn` dep.
//! The stripper is a ~100-line lexical scanner (line/block comments,
//! plain/raw/byte strings, char literals vs lifetimes) — enough to
//! kill every comment/literal false positive without a full parser.
//! Known residual false positive: `#[deprecated]` inside a macro token
//! tree (e.g. `quote! { #[deprecated] fn x() {} }`) would still count
//! while the extractor emits nothing. cfdb-self has zero such
//! occurrences; if one appears, the sentinel fails closed (RED), which
//! is the safe direction for a recall gate.

use std::fs;
use std::io;
use std::path::Path;

use regex::Regex;

/// Match a bare `#[deprecated]` attribute or its kv form
/// `#[deprecated(...)]` on the `deprecated` ident at attribute position.
///
/// Anchored on `#[` (or `#![` for inner attrs) followed by optional
/// whitespace, the bare `deprecated` ident, then either `]` (bare form)
/// or `(` (kv form). The leading `#[` / `#![` anchor is the equivalent
/// of an attribute-position guard — `serde(deprecated)` does not match
/// because no `#[` precedes it inside the attribute body.
fn deprecated_attr_regex() -> Regex {
    // Compiled once per call — small enough that the OnceLock complexity
    // isn't worth it; a typical CI run lexes ≤ 1k files.
    Regex::new(r"#!?\[\s*deprecated\s*[\]\(]").expect("static regex compiles")
}

/// Is `c` a character that can appear in an identifier? Used to decide
/// whether `r` / `b` / `c` starts a raw-string prefix or is the tail of
/// an identifier (`for`, `crab`, …), and whether a `'` opens a char
/// literal or a lifetime.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Lexically strip comments and string/char literals from Rust source,
/// replacing each stripped region with a single space so the
/// surrounding token stream stays intact for the attribute regex.
///
/// Handles: `//` line comments (incl. `///` and `//!` doc comments),
/// nested `/* */` block comments (incl. `/** */`), plain strings with
/// escapes, raw strings `r"…"` / `r#"…"#` (any fence width, with `b` /
/// `c` prefixes), byte/C strings via the plain-string path, char
/// literals `'x'` / `'\n'` / `'\u{…}'` and byte-char literals `b'x'` /
/// `b'"'` — while leaving lifetimes (`'a`, `'_`) untouched. The
/// byte-char case is load-bearing: `b'"'` is legal Rust whose content
/// quote would otherwise open a phantom string that swallows the rest
/// of the file (PR #563 adversarial review, finding A2).
///
/// Pure function: `&str` in, `String` out, no I/O. Malformed input
/// (unterminated literal at EOF) strips to end-of-input rather than
/// panicking — the compiler owns rejecting such files; the sentinel
/// only needs to never miscount on code that builds. Same reasoning
/// bounds the char/lifetime heuristic: a lifetime abutting a quote
/// with zero separator (`'a'"'…`) can mislead it, but that adjacency
/// is not legal Rust, and every input here is a compiling workspace
/// member by construction (the keyspace's `:File` set).
pub fn strip_comments_and_strings(src: &str) -> String {
    let cs: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs[i];
        // Line comment — skip to end of line (newline kept).
        if c == '/' && cs.get(i + 1) == Some(&'/') {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
            out.push(' ');
            continue;
        }
        // Block comment — nested per Rust lexing.
        if c == '/' && cs.get(i + 1) == Some(&'*') {
            let mut depth = 1usize;
            i += 2;
            while i < cs.len() && depth > 0 {
                if cs[i] == '/' && cs.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if cs[i] == '*' && cs.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            out.push(' ');
            continue;
        }
        // Raw string (with optional b/c prefix): r"…", r#"…"#, br#"…"#…
        // Only when the prefix starts a new token (previous char is not
        // an identifier char — `abr#"` must not be misread).
        let prev_is_ident = i > 0 && is_ident_char(cs[i - 1]);
        if !prev_is_ident && (c == 'r' || c == 'b' || c == 'c') {
            if let Some(end) = raw_string_end(&cs, i) {
                out.push(' ');
                i = end;
                continue;
            }
        }
        // Byte-char literal `b'…'`. Must be caught BEFORE the quote is
        // reached: at the quote, `prev_is_ident` is true (the `b`), so
        // the char-literal branch below would skip it and `b'"'` would
        // open a phantom string.
        if !prev_is_ident && c == 'b' {
            if let Some(end) = char_literal_end(&cs, i + 1) {
                out.push(' ');
                i = end;
                continue;
            }
        }
        // Plain string literal (also reached after a copied b/c prefix
        // char): skip with escape handling.
        if c == '"' {
            i += 1;
            while i < cs.len() {
                if cs[i] == '\\' {
                    i += 2;
                } else if cs[i] == '"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            out.push(' ');
            continue;
        }
        // Char literal vs lifetime. A char literal is `'` + (escape or
        // one char) + `'`. Anything else (`'a>`, `'_ `, `'static`) is a
        // lifetime — copy the quote and move on.
        if c == '\'' && !prev_is_ident {
            if let Some(end) = char_literal_end(&cs, i) {
                out.push(' ');
                i = end;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// If `cs[q..]` opens a char literal (`'` + escape-or-one-char + `'`),
/// return the index one past its closing quote. `None` when the quote
/// is a lifetime opener (or not a quote at all). Shared by the plain
/// (`'x'`) and byte (`b'x'`) char-literal branches.
fn char_literal_end(cs: &[char], q: usize) -> Option<usize> {
    if cs.get(q) != Some(&'\'') {
        return None;
    }
    if cs.get(q + 1) == Some(&'\\') {
        // Escaped char literal — skip to the closing quote (or EOF,
        // the unterminated fail-safe).
        let mut j = q + 2;
        while j < cs.len() && cs[j] != '\'' {
            j += 1;
        }
        return Some((j + 1).min(cs.len()));
    }
    if cs.get(q + 2) == Some(&'\'') && cs.get(q + 1) != Some(&'\'') {
        return Some(q + 3);
    }
    None
}

/// If `cs[start..]` opens a raw-string literal (optional `b`/`c`, then
/// `r`, then `#`-fence, then `"`), return the index one past its
/// closing delimiter. Otherwise `None`.
fn raw_string_end(cs: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    if cs.get(i) == Some(&'b') || cs.get(i) == Some(&'c') {
        i += 1;
    }
    if cs.get(i) != Some(&'r') {
        return None;
    }
    i += 1;
    let mut hashes = 0usize;
    while cs.get(i) == Some(&'#') {
        hashes += 1;
        i += 1;
    }
    if cs.get(i) != Some(&'"') {
        return None;
    }
    i += 1;
    // Scan for `"` followed by `hashes` × `#`.
    while i < cs.len() {
        if cs[i] == '"' {
            let fence_end = i + 1 + hashes;
            if cs[i + 1..cs.len().min(fence_end)].iter().all(|&h| h == '#') && fence_end <= cs.len()
            {
                return Some(fence_end);
            }
        }
        i += 1;
    }
    Some(cs.len()) // unterminated at EOF — strip to end, fail-safe
}

/// Count attribute-position `#[deprecated]` occurrences in one file's
/// source text: strip comments and literals, then run the regex.
pub fn count_deprecated_in_source(src: &str) -> usize {
    deprecated_attr_regex()
        .find_iter(&strip_comments_and_strings(src))
        .count()
}

/// Count attribute-position `#[deprecated]` occurrences across the
/// extractor-walked file set. `rel_paths` are workspace-relative paths
/// as recorded on the keyspace's `:File` nodes (see
/// [`crate::extracted_files::file_paths_in_keyspace`]); each is joined
/// onto `workspace_root` and read.
///
/// Error contract: an unreadable file returns `Err(io::Error)` naming
/// the path — a keyspace that references files missing on disk is a
/// configuration problem (stale keyspace vs. checkout), not a
/// violation. The harness maps this to `EXIT_RUNTIME_ERROR`.
pub fn count_deprecated_in_files(workspace_root: &Path, rel_paths: &[String]) -> io::Result<usize> {
    let mut total = 0usize;
    for rel in rel_paths {
        let path = workspace_root.join(rel);
        let content = fs::read_to_string(&path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("reading extracted file {}: {e}", path.display()),
            )
        })?;
        total += count_deprecated_in_source(&content);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── regex (attribute-position) ────────────────────────────────────

    /// Bare `#[deprecated]` matches.
    #[test]
    fn regex_matches_bare_form() {
        let re = deprecated_attr_regex();
        assert_eq!(re.find_iter("#[deprecated]\nfn x() {}").count(), 1);
    }

    /// `#[deprecated(note = "...")]` matches.
    #[test]
    fn regex_matches_note_kv_form() {
        let re = deprecated_attr_regex();
        assert_eq!(
            re.find_iter(r##"#[deprecated(note = "use bar instead")]"##)
                .count(),
            1
        );
    }

    /// Inner attribute `#![deprecated]` matches (extractor accepts both).
    #[test]
    fn regex_matches_inner_attribute_form() {
        let re = deprecated_attr_regex();
        assert_eq!(re.find_iter("#![deprecated]").count(), 1);
    }

    /// Whitespace tolerance — `#[ deprecated ]` and `#[deprecated  (...)`
    /// both match.
    #[test]
    fn regex_matches_with_internal_whitespace() {
        let re = deprecated_attr_regex();
        assert_eq!(re.find_iter("#[ deprecated ]").count(), 1);
        assert_eq!(re.find_iter(r##"#[deprecated  (note = "x")]"##).count(), 1);
    }

    /// Multi-segment paths like `#[serde(deprecated)]` do NOT match —
    /// the regex anchors on `#[` directly preceding the `deprecated`
    /// ident, so an inner `(deprecated)` is not at attribute position.
    #[test]
    fn regex_rejects_multi_segment_paths() {
        let re = deprecated_attr_regex();
        assert_eq!(re.find_iter("#[serde(deprecated = \"true\")]").count(), 0);
        assert_eq!(re.find_iter("#[doc(deprecated)]").count(), 0);
    }

    // ── stripper: false positives the raw grep used to count ──────────

    /// A `#[deprecated]` mention inside a `//` / `///` / `//!` comment
    /// is not a fact about any item — must not count.
    #[test]
    fn line_comments_do_not_count() {
        assert_eq!(
            count_deprecated_in_source("// the #[deprecated] attribute\nfn a() {}\n"),
            0
        );
        assert_eq!(
            count_deprecated_in_source("/// docs mention `#[deprecated]` here\nfn a() {}\n"),
            0
        );
        assert_eq!(
            count_deprecated_in_source("//! module docs: #[deprecated(note = \"x\")]\n"),
            0
        );
    }

    /// Block comments (including nested) must not count.
    #[test]
    fn block_comments_do_not_count() {
        assert_eq!(
            count_deprecated_in_source("/* #[deprecated] */ fn a() {}"),
            0
        );
        assert_eq!(
            count_deprecated_in_source(
                "/* outer /* #[deprecated] inner */ still outer */ fn a() {}"
            ),
            0
        );
    }

    /// String literals carrying attribute text (this crate's own tests
    /// are full of them) must not count.
    #[test]
    fn string_literals_do_not_count() {
        assert_eq!(
            count_deprecated_in_source(r##"let s = "#[deprecated]"; fn a() {}"##),
            0
        );
        assert_eq!(
            count_deprecated_in_source(
                "let s = \"prefix #[deprecated(note = \\\"x\\\")] suffix\";"
            ),
            0
        );
    }

    /// Raw string literals (the shape `parse_attrs(r#"#[deprecated]"#)`
    /// used throughout cfdb-extractor's attr tests) must not count.
    #[test]
    fn raw_string_literals_do_not_count() {
        assert_eq!(
            count_deprecated_in_source(r####"let a = parse(r#"#[deprecated]"#);"####),
            0
        );
        assert_eq!(
            count_deprecated_in_source(r####"let a = r"#[deprecated]";"####),
            0
        );
        assert_eq!(
            count_deprecated_in_source(r####"let a = br#"#[deprecated(note = "x")]"#;"####),
            0
        );
    }

    /// Real attribute BETWEEN stripped regions still counts — stripping
    /// must not eat surrounding code.
    #[test]
    fn real_attr_survives_adjacent_comments_and_strings() {
        let src = r##"
/// This function is going away; docs even show `#[deprecated]` usage.
#[deprecated(note = "use bar")]
fn a() { let msg = "#[deprecated] in a string"; }
"##;
        assert_eq!(count_deprecated_in_source(src), 1);
    }

    /// Comments inside the attribute token run are legal Rust — the
    /// stripped space keeps the regex matching.
    #[test]
    fn comment_inside_attr_tokens_still_counts() {
        assert_eq!(
            count_deprecated_in_source("#[/* soon */ deprecated]\nfn a() {}"),
            1
        );
    }

    /// Byte-char literals must be consumed as literals — `b'"'` is
    /// legal Rust whose content quote would otherwise open a phantom
    /// string and swallow every later `#[deprecated]` in the file
    /// (adversarial-review finding A2, PR #563).
    #[test]
    fn byte_char_literal_with_quote_content_does_not_derail() {
        let src = "let q = b'\"';\n#[deprecated]\nfn old() {}\n";
        assert_eq!(count_deprecated_in_source(src), 1);
    }

    /// The mundane byte-char forms are stripped cleanly too.
    #[test]
    fn byte_char_literals_are_stripped() {
        let src = "let a = b'x';\nlet b = b'\\'';\nlet c = b'#';\n#[deprecated]\nfn old() {}\n";
        assert_eq!(count_deprecated_in_source(src), 1);
    }

    /// Lifetimes and char literals must not derail the scanner: a `'a`
    /// lifetime is not a string opener, and a `'"'` char literal must
    /// not open a phantom string that swallows a real attr.
    #[test]
    fn lifetimes_and_char_literals_are_handled() {
        let src = r#"
fn f<'a>(x: &'a str) -> &'_ str { x }
const Q: char = '"';
const E: char = '\'';
const U: char = '\u{1F600}';
#[deprecated]
fn old() {}
"#;
        assert_eq!(count_deprecated_in_source(src), 1);
    }

    // ── file-set counting ─────────────────────────────────────────────

    /// Counts only the files in the provided (extractor-walked) set —
    /// a file on disk with a real attr but absent from the set is
    /// invisible, mirroring extractor scope.
    #[test]
    fn counts_only_files_in_the_provided_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(
            root.join("src/lib.rs"),
            "#[deprecated]\nfn a() {}\n\n#[deprecated(note = \"x\")]\nfn b() {}\n",
        )
        .expect("write lib.rs");
        std::fs::write(
            root.join("src/unwalked.rs"),
            "#[deprecated(since = \"1.0\")]\nfn c() {}\n",
        )
        .expect("write unwalked.rs");

        let count =
            count_deprecated_in_files(root, &["src/lib.rs".to_string()]).expect("read succeeds");
        assert_eq!(
            count, 2,
            "src/lib.rs has 2 attribute-position occurrences; src/unwalked.rs \
             is not in the extracted set and must not be counted"
        );
    }

    /// A set file whose only mentions live in comments/strings
    /// contributes zero.
    #[test]
    fn set_file_with_only_comment_mentions_contributes_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(
            root.join("doc_only.rs"),
            "//! `#[deprecated]` is documented here\nlet s = \"#[deprecated]\";\n",
        )
        .expect("write doc_only.rs");
        let count =
            count_deprecated_in_files(root, &["doc_only.rs".to_string()]).expect("read succeeds");
        assert_eq!(count, 0);
    }

    /// A keyspace file missing on disk is a configuration error, not a
    /// zero — the harness must exit 1, never silently under-truth.
    #[test]
    fn missing_set_file_errors_with_path_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = count_deprecated_in_files(dir.path(), &["ghost.rs".to_string()])
            .expect_err("missing file must error");
        assert!(
            err.to_string().contains("ghost.rs"),
            "error must name the missing file: {err}"
        );
    }

    /// Empty file set counts zero — does not error. (The harness guards
    /// zero-`:File` keyspaces upstream in `extracted_files`.)
    #[test]
    fn empty_file_set_counts_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(count_deprecated_in_files(dir.path(), &[]).expect("ok"), 0);
    }
}
