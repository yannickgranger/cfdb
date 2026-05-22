//! Source-side ground truth for the `enrich-rfc-docs` dogfood.
//!
//! Walks a workspace's `docs/` directory (one level, NOT recursive) and
//! counts files whose name matches `RFC-*.md` — the literal glob shape
//! prescribed in RFC-039 §3.1 row `enrich-rfc-docs` and AC-1 of
//! issue #344. The harness substitutes the count into the
//! `.cfdb/queries/self-enrich-rfc-docs.cypher` template's
//! `{{ ground_truth_count }}` placeholder. The Cypher sentinel then
//! compares it against `count(:RfcDoc)`. A drop in the extracted count
//! surfaces as a violation row (sentinel `rfc_doc_count`).
//!
//! ## Pure-helper contract
//!
//! - **Input:** workspace root path.
//! - **Output:** `usize` count of files in `<workspace>/docs/` whose
//!   name starts with `RFC-` and ends with `.md`.
//! - **Error mode:** I/O failure on `read_dir` bubbles as `io::Error`.
//!   The harness maps this to `EXIT_RUNTIME_ERROR` (1). A missing
//!   `docs/` directory is NOT an error — it short-circuits to 0
//!   (mirroring `grep_deprecated::walk_rs_files`'s `is_dir` guard).
//!
//! ## Why filename-match, not content-grep
//!
//! RFC-039 §7.3 prescribes "pure helper that counts `docs/RFC-*.md`
//! files returns the expected count on a fixture." The ground truth
//! is filesystem identity — a file named `RFC-040-foo.md` IS an RFC
//! whether or not its content parses. The cypher sentinel cares
//! about node count, not document validity, so a stdlib `read_dir`
//! pass is the right size:
//!
//! - No `regex` or `syn` dep beyond what the crate already pulls in.
//! - The "single segment of glob" rule is exactly `entry.file_name()`
//!   string-prefix + suffix, no traversal needed.
//! - The dash after `RFC` is significant — `docs/RFC.md` is the
//!   un-numbered umbrella RFC and is excluded by the glob (it has no
//!   trailing identifier). Issue #344 explicitly enumerates
//!   `RFC-*.md`, NOT `RFC*.md`.
//!
//! ## Non-recursion rationale
//!
//! The glob is one segment (`docs/RFC-*.md`). RFC files in cfdb live
//! flat at `docs/` — there are no `docs/<sub>/RFC-*.md` files. A
//! recursive walker would inflate the count if a future PR landed an
//! `RFC-*.md` under e.g. `docs/archive/`, which would NOT be ingested
//! by the extractor and would produce false negatives in the sentinel
//! (extracted < ground truth → spurious RED). The non-recursive
//! contract pins the ground truth to exactly what the producer sees.

use std::fs;
use std::io;
use std::path::Path;

/// Returns true iff `name` matches the `RFC-*.md` glob — i.e. starts
/// with the literal prefix `RFC-` and ends with the literal suffix
/// `.md`. The dash is required: `RFC.md` (no dash) does NOT match.
///
/// Pure on `&str`; no I/O. Extracted for unit-test fidelity to the
/// glob semantics independently of `read_dir` behavior.
fn is_rfc_md_filename(name: &str) -> bool {
    name.starts_with("RFC-") && name.ends_with(".md")
}

/// Count files in `<workspace>/docs/` whose name matches `RFC-*.md`.
///
/// One-segment scan (NOT recursive) — the canonical location for cfdb
/// RFCs is flat under `docs/`. Subdirectories are skipped silently;
/// non-`.md` files are skipped silently; a missing `docs/` directory
/// returns `Ok(0)` (no error).
///
/// Error contract: an I/O failure during `read_dir` returns
/// `Err(io::Error)`. The harness maps to `EXIT_RUNTIME_ERROR`.
pub fn count_rfc_md_files(workspace: &Path) -> io::Result<usize> {
    let docs_dir = workspace.join("docs");
    if !docs_dir.is_dir() {
        return Ok(0);
    }
    let mut total = 0usize;
    for entry in fs::read_dir(&docs_dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if is_rfc_md_filename(&name_str) {
            total += 1;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bare `RFC-NNN-foo.md` matches.
    #[test]
    fn filename_matches_numbered_rfc() {
        assert!(is_rfc_md_filename("RFC-039-dogfood-enrichment-passes.md"));
        assert!(is_rfc_md_filename("RFC-001-foo.md"));
    }

    /// `RFC-cfdb.md` (the original umbrella RFC, kebab-only suffix)
    /// matches the glob — no numeric requirement. Mirrors what the
    /// extractor's RFC-doc producer sees.
    #[test]
    fn filename_matches_kebab_only_rfc() {
        assert!(is_rfc_md_filename("RFC-cfdb.md"));
    }

    /// `RFC.md` (no dash) does NOT match. The dash is significant.
    #[test]
    fn filename_rejects_no_dash() {
        assert!(!is_rfc_md_filename("RFC.md"));
    }

    /// Wrong prefix (`some-rfc-foo.md`, lowercase, prefixed) is rejected.
    #[test]
    fn filename_rejects_wrong_prefix() {
        assert!(!is_rfc_md_filename("some-rfc-foo.md"));
        assert!(!is_rfc_md_filename("rfc-039-foo.md"));
        assert!(!is_rfc_md_filename("notes-RFC-039.md"));
    }

    /// Wrong extension is rejected.
    #[test]
    fn filename_rejects_wrong_extension() {
        assert!(!is_rfc_md_filename("RFC-039-foo.txt"));
        assert!(!is_rfc_md_filename("RFC-039-foo"));
    }

    /// `count_rfc_md_files` integrates the filename match against a
    /// real on-disk `docs/` fixture with a mix of matching and non-
    /// matching files.
    #[test]
    fn count_walks_docs_and_filters_to_rfc_glob() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let docs = root.join("docs");
        std::fs::create_dir_all(&docs).expect("docs dir");

        // 3 matching files.
        std::fs::write(docs.join("RFC-001-alpha.md"), "# alpha\n").expect("write a");
        std::fs::write(docs.join("RFC-002-beta.md"), "# beta\n").expect("write b");
        std::fs::write(docs.join("RFC-cfdb.md"), "# umbrella\n").expect("write c");

        // Non-matching files in the same dir.
        std::fs::write(docs.join("RFC.md"), "# no-dash\n").expect("write no-dash");
        std::fs::write(docs.join("some-rfc-foo.md"), "# wrong prefix\n").expect("write wp");
        std::fs::write(docs.join("RFC-003-gamma.txt"), "# wrong ext\n").expect("write we");
        std::fs::write(docs.join("README.md"), "# readme\n").expect("write readme");

        // Subdirectory with a matching name — must be skipped (not a
        // file, and the scan is non-recursive anyway).
        std::fs::create_dir_all(docs.join("RFC-archive")).expect("subdir");
        std::fs::write(docs.join("RFC-archive/RFC-999-buried.md"), "# nested\n")
            .expect("write nested");

        let count = count_rfc_md_files(root).expect("walk succeeds");
        assert_eq!(
            count, 3,
            "expected 3 matches (RFC-001-alpha.md, RFC-002-beta.md, \
             RFC-cfdb.md); RFC.md / some-rfc-foo.md / .txt / README.md \
             excluded; nested file under docs/RFC-archive/ excluded \
             by non-recursion"
        );
    }

    /// Empty `docs/` returns zero — does not error.
    #[test]
    fn count_zero_on_empty_docs_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("docs")).expect("docs dir");
        let count = count_rfc_md_files(dir.path()).expect("walk succeeds");
        assert_eq!(count, 0);
    }

    /// Missing `docs/` returns zero (the `is_dir` guard short-circuits)
    /// — the harness then substitutes 0 into the template and the
    /// rfc-doc-count sentinel is satisfied trivially.
    #[test]
    fn count_zero_on_missing_docs_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        // No `docs/` created.
        let count = count_rfc_md_files(dir.path()).expect("missing-docs walk succeeds (no-op)");
        assert_eq!(count, 0);
    }

    /// Non-existent workspace root returns zero — same `is_dir`
    /// short-circuit applied to a path that doesn't exist at all.
    #[test]
    fn count_zero_on_nonexistent_root() {
        let count = count_rfc_md_files(Path::new(
            "/nonexistent/path/zzz/dogfood-enrich-rfc-docs-test",
        ))
        .expect("non-dir walk succeeds (no-op)");
        assert_eq!(count, 0);
    }
}
