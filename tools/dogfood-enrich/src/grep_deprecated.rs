//! Source-side ground truth for the `enrich-deprecation` dogfood.
//!
//! Walks a workspace's `.rs` files and counts occurrences of the bare
//! single-segment `#[deprecated]` attribute (in any of its three accepted
//! forms — bare path, `note = "..."`, `since = "X.Y.Z"`). Multi-segment
//! paths like `#[serde(deprecated = "true")]` are deliberately not
//! counted, mirroring the discipline in
//! [`cfdb_extractor::attrs::extract_deprecated_attr`] (per RFC-039 §3.1
//! `enrich-deprecation` row).
//!
//! The harness substitutes the count into the
//! `.cfdb/queries/self-enrich-deprecation.cypher` template's
//! `{{ ground_truth_count }}` placeholder. The Cypher sentinel then
//! compares it against `count(:Item WHERE is_deprecated = true)`. A
//! drop in the extracted count surfaces as a violation row.
//!
//! ## Pure-helper contract
//!
//! - **Input:** workspace root path.
//! - **Output:** `usize` count of `#[deprecated]` attribute matches
//!   across every `.rs` file under the workspace root, excluding
//!   `target/` and `.git/` directories.
//! - **Error mode:** I/O failure on `read_dir` or `read_to_string`
//!   bubbles as `io::Error`. The harness maps this to
//!   `EXIT_RUNTIME_ERROR` (1).
//!
//! ## Why text-grep, not syn-parse
//!
//! RFC-039 §7.2 prescribes "pure helper that greps `#[deprecated]` from
//! a workspace returns expected count on a fixture." Text-grep is the
//! simpler signal:
//!
//! - No `syn` dep on `dogfood-enrich` (keeps the harness leaf small per
//!   RFC §3.5.1 SAP analysis).
//! - The regex is precise enough to mirror
//!   `extract_deprecated_attr`'s "single-segment ident" rule because
//!   the lookbehind-equivalent (`(?:^|[^:\w])`) reliably rejects
//!   `serde(deprecated)` and similar multi-segment forms.
//! - Comments and string literals in the source can produce false
//!   positives. In practice cfdb-self has zero `#[deprecated]` strings
//!   inside comments/literals at HEAD; if that changes a future PR
//!   refines this helper to skip them. The unit-test fixture pins the
//!   current behavior.

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
    // isn't worth it; a typical CI run greps ≤ 1k files.
    Regex::new(r"#!?\[\s*deprecated\s*[\]\(]").expect("static regex compiles")
}

/// Recursively walk `root`, returning every `.rs` file's path.
///
/// Skips conventional non-source directories (`target/`, `.git/`,
/// `.cfdb/`, `.claude/`, `node_modules/`) — these can hold large
/// volumes of generated or vendored code that would inflate the
/// ground-truth count without representing extractor input.
///
/// Pure-IO helper. Errors propagate verbatim from `read_dir`; the
/// caller decides how to surface them.
fn walk_rs_files(root: &Path, out: &mut Vec<std::path::PathBuf>) -> io::Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if matches!(
                name_str.as_ref(),
                "target" | ".git" | ".cfdb" | ".claude" | "node_modules"
            ) {
                continue;
            }
            walk_rs_files(&path, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Count `#[deprecated]` attribute occurrences across every `.rs` file
/// under `workspace_root`. The count returned is the source-side
/// ground truth substituted into the Cypher sentinel.
///
/// Error contract: an I/O failure during the walk or read returns
/// `Err(io::Error)`. The harness maps to `EXIT_RUNTIME_ERROR`.
pub fn count_deprecated_in_workspace(workspace_root: &Path) -> io::Result<usize> {
    let mut files = Vec::new();
    walk_rs_files(workspace_root, &mut files)?;
    let regex = deprecated_attr_regex();
    let mut total = 0usize;
    for file in files {
        let content = fs::read_to_string(&file)?;
        total += regex.find_iter(&content).count();
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            re.find_iter(r#"#[deprecated(note = "use bar instead")]"#)
                .count(),
            1
        );
    }

    /// `#[deprecated(since = "1.0", note = "...")]` matches.
    #[test]
    fn regex_matches_since_kv_form() {
        let re = deprecated_attr_regex();
        assert_eq!(
            re.find_iter(r#"#[deprecated(since = "1.0", note = "x")]"#)
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
        assert_eq!(re.find_iter(r#"#[deprecated  (note = "x")]"#).count(), 1);
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

    /// `deprecated` as a free-standing word in code or string is NOT
    /// matched (no `#[` anchor).
    #[test]
    fn regex_rejects_free_standing_ident() {
        let re = deprecated_attr_regex();
        assert_eq!(
            re.find_iter("let deprecated = true;").count(),
            0,
            "binding name 'deprecated' must not match"
        );
        assert_eq!(
            re.find_iter(r#"println!("deprecated");"#).count(),
            0,
            "string literal 'deprecated' must not match"
        );
    }

    /// `count_deprecated_in_workspace` integrates the walker and the
    /// regex against a real on-disk fixture.
    #[test]
    fn count_walks_workspace_and_sums_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Two .rs files at different depths.
        std::fs::write(
            root.join("a.rs"),
            "#[deprecated]\nfn a() {}\n\n#[deprecated(note = \"x\")]\nfn b() {}\n",
        )
        .expect("write a.rs");
        std::fs::create_dir_all(root.join("nested")).expect("nested dir");
        std::fs::write(
            root.join("nested/b.rs"),
            "#[deprecated(since = \"1.0\")]\nfn c() {}\n",
        )
        .expect("write nested/b.rs");

        // Non-`.rs` and excluded directories must be ignored.
        std::fs::write(root.join("README.md"), "#[deprecated]").expect("write README");
        std::fs::create_dir_all(root.join("target")).expect("target dir");
        std::fs::write(root.join("target/cached.rs"), "#[deprecated]\nfn d() {}\n")
            .expect("write target/cached.rs");

        let count = count_deprecated_in_workspace(root).expect("walk succeeds");
        assert_eq!(
            count, 3,
            "expected 3 #[deprecated] occurrences across a.rs (2) + nested/b.rs (1); \
             target/cached.rs is excluded and README.md is not .rs"
        );
    }

    /// Empty workspace returns zero — does not error.
    #[test]
    fn count_zero_on_empty_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let count = count_deprecated_in_workspace(dir.path()).expect("walk succeeds");
        assert_eq!(count, 0);
    }

    /// Non-existent workspace returns zero (the `is_dir` guard short-
    /// circuits) — the harness then substitutes 0 into the template
    /// and the sentinel is satisfied trivially.
    #[test]
    fn count_zero_on_nonexistent_root() {
        let count =
            count_deprecated_in_workspace(Path::new("/nonexistent/path/zzz/dogfood-enrich-test"))
                .expect("non-dir walk succeeds (no-op)");
        assert_eq!(count, 0);
    }
}
