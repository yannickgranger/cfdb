use std::fs;
use std::io;
use std::path::Path;

use regex::Regex;

fn deprecated_attr_regex() -> Regex {
    Regex::new(r"#!?\[\s*deprecated\s*[\]\(]").expect("static regex compiles")
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn strip_comments_and_strings(src: &str) -> String {
    let cs: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs[i];
        if c == '/' && cs.get(i + 1) == Some(&'/') {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
            out.push(' ');
            continue;
        }
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
        let prev_is_ident = i > 0 && is_ident_char(cs[i - 1]);
        if !prev_is_ident && (c == 'r' || c == 'b' || c == 'c') {
            if let Some(end) = raw_string_end(&cs, i) {
                out.push(' ');
                i = end;
                continue;
            }
        }
        if !prev_is_ident && c == 'b' {
            if let Some(end) = char_literal_end(&cs, i + 1) {
                out.push(' ');
                i = end;
                continue;
            }
        }
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

fn char_literal_end(cs: &[char], q: usize) -> Option<usize> {
    if cs.get(q) != Some(&'\'') {
        return None;
    }
    if cs.get(q + 1) == Some(&'\\') {
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
    Some(cs.len())
}

pub fn count_deprecated_in_source(src: &str) -> usize {
    deprecated_attr_regex()
        .find_iter(&strip_comments_and_strings(src))
        .count()
}

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

    #[test]
    fn regex_matches_bare_form() {
        let re = deprecated_attr_regex();
        assert_eq!(re.find_iter("#[deprecated]\nfn x() {}").count(), 1);
    }

    #[test]
    fn regex_matches_note_kv_form() {
        let re = deprecated_attr_regex();
        assert_eq!(
            re.find_iter(r##"#[deprecated(note = "use bar instead")]"##)
                .count(),
            1
        );
    }

    #[test]
    fn regex_matches_inner_attribute_form() {
        let re = deprecated_attr_regex();
        assert_eq!(re.find_iter("#![deprecated]").count(), 1);
    }

    #[test]
    fn regex_matches_with_internal_whitespace() {
        let re = deprecated_attr_regex();
        assert_eq!(re.find_iter("#[ deprecated ]").count(), 1);
        assert_eq!(re.find_iter(r##"#[deprecated  (note = "x")]"##).count(), 1);
    }

    #[test]
    fn regex_rejects_multi_segment_paths() {
        let re = deprecated_attr_regex();
        assert_eq!(re.find_iter("#[serde(deprecated = \"true\")]").count(), 0);
        assert_eq!(re.find_iter("#[doc(deprecated)]").count(), 0);
    }

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

    #[test]
    fn real_attr_survives_adjacent_comments_and_strings() {
        let src = r##"
/// This function is going away; docs even show `#[deprecated]` usage.
#[deprecated(note = "use bar")]
fn a() { let msg = "#[deprecated] in a string"; }
"##;
        assert_eq!(count_deprecated_in_source(src), 1);
    }

    #[test]
    fn comment_inside_attr_tokens_still_counts() {
        assert_eq!(
            count_deprecated_in_source("#[/* soon */ deprecated]\nfn a() {}"),
            1
        );
    }

    #[test]
    fn byte_char_literal_with_quote_content_does_not_derail() {
        let src = "let q = b'\"';\n#[deprecated]\nfn old() {}\n";
        assert_eq!(count_deprecated_in_source(src), 1);
    }

    #[test]
    fn byte_char_literals_are_stripped() {
        let src = "let a = b'x';\nlet b = b'\\'';\nlet c = b'#';\n#[deprecated]\nfn old() {}\n";
        assert_eq!(count_deprecated_in_source(src), 1);
    }

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

    #[test]
    fn empty_file_set_counts_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(count_deprecated_in_files(dir.path(), &[]).expect("ok"), 0);
    }
}
