//! Source-side ground truth for the `enrich-concepts` dogfood.
//!
//! Walks a workspace's `.cfdb/concepts/*.toml` files and extracts two
//! counts the [`crate::passes`] `enrich-concepts` cypher template needs
//! substituted before submission:
//!
//!   - `distinct_context_names` — number of distinct top-level
//!     `name = "<value>"` declarations across the directory
//!     (deduplicated by value, NOT by file). Substituted as the
//!     `{{ declared_context_count }}` placeholder.
//!   - `declared_canonical_crate_count` — number of TOML files (or
//!     equivalently, top-level concept entries) that declare a
//!     non-empty `canonical_crate = "<value>"` at top level.
//!     Substituted as the `{{ declared_canonical_crate_count }}`
//!     placeholder. A workspace whose every concept omits
//!     `canonical_crate` (legitimate per
//!     `crates/cfdb-concepts/src/lib.rs:65` — `Option<String>`) yields
//!     `0`, and the cypher template skips sentinel (c) on that case.
//!
//! ## Pure-helper contract
//!
//! - **Input:** workspace root path. The scanner appends
//!   `.cfdb/concepts/` itself.
//! - **Output:** [`ConceptCounts`] with the two `usize` counts. Errors
//!   propagate verbatim from `read_dir` / `read_to_string`; the harness
//!   maps to `EXIT_RUNTIME_ERROR`.
//! - **Non-recursive walk:** only files matching the literal one-segment
//!   glob `.cfdb/concepts/*.toml` are scanned. Sub-directories are
//!   ignored, mirroring `cfdb-concepts::load_concept_overrides`'
//!   single-level directory iteration (one TOML per context).
//!
//! ## Why text parsing, not the `toml` crate
//!
//! RFC-039 §3.5.1 SAP analysis: `tools/dogfood-enrich` is the leaf in
//! the dogfood pipeline (`Ca = 0`); every dependency added here widens
//! the harness's compile cost and surface area. The TOML files this
//! scanner reads are tightly conventioned (one concept per file,
//! top-level scalar fields only — no nested arrays of tables, no
//! computed values), so a line-based regex parser captures the schema
//! the harness needs.
//!
//! Specifically the scanner recognises two top-level field shapes:
//!
//!   - `^\s*name\s*=\s*"([^"]*)"` — outside any `[section]` header
//!   - `^\s*canonical_crate\s*=\s*"([^"]*)"` — outside any `[section]`
//!     header; counted only when the captured string is non-empty
//!
//! Lines starting with `#` are skipped. A `[section.header]` line
//! switches the scanner into "in-section" state, and subsequent
//! `name`/`canonical_crate` matches are ignored until EOF or the next
//! file — top-level fields in TOML must precede any section header.
//!
//! ## Filename-stem fallback
//!
//! If a TOML file declares no top-level `name` field at all, the
//! scanner uses the filename stem (e.g. `cfdb.toml` → `cfdb`) as the
//! synthetic context name. This mirrors the de-facto convention of
//! cfdb-self (`cfdb.toml` declares `name = "cfdb"` — the stem and the
//! declared name agree) and is the safest fallback for hand-written
//! fixtures.

use std::fs;
use std::io;
use std::path::Path;

use regex::Regex;

/// Two counts derived from a `.cfdb/concepts/*.toml` scan. Returned by
/// [`scan_concepts`] and consumed by the harness's per-pass placeholder
/// substitution for `enrich-concepts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConceptCounts {
    /// Number of DISTINCT context names across all TOML files. Two
    /// files declaring `name = "cfdb"` collapse to one.
    pub distinct_context_names: usize,
    /// Number of TOML files that declare a non-empty
    /// `canonical_crate = "..."` at top level. Counted per-file (not
    /// deduplicated by crate name) — the dogfood sentinel only needs
    /// the binary "any declared?" answer to gate sentinel (c).
    pub declared_canonical_crate_count: usize,
}

/// Match a top-level `name = "<value>"` declaration. The value capture
/// group accepts any (possibly-empty) double-quoted string.
fn name_field_regex() -> Regex {
    Regex::new(r#"^\s*name\s*=\s*"([^"]*)""#).expect("static regex compiles")
}

/// Match a top-level `canonical_crate = "<value>"` declaration. The
/// value capture group accepts any (possibly-empty) double-quoted
/// string; the caller filters empty matches per the schema rule "empty
/// string == not declared".
fn canonical_crate_field_regex() -> Regex {
    Regex::new(r#"^\s*canonical_crate\s*=\s*"([^"]*)""#).expect("static regex compiles")
}

/// Match the start of a `[section.header]` line — anything that looks
/// like a TOML table header at the start of the line. Once seen, the
/// scanner ignores subsequent `name` / `canonical_crate` matches in the
/// same file (top-level fields must precede sections per the TOML spec).
fn section_header_regex() -> Regex {
    Regex::new(r#"^\s*\["#).expect("static regex compiles")
}

/// Per-file scan result. Internal — exposed via the aggregated
/// [`ConceptCounts`] in [`scan_concepts`].
struct PerFileScan {
    /// The top-level `name = "..."` value, or the filename stem if no
    /// `name` field was declared in the top-level (pre-section) region.
    context_name: String,
    /// `true` if the file declared a non-empty top-level
    /// `canonical_crate = "<non-empty>"`.
    declares_canonical_crate: bool,
}

/// Scan one TOML file. The function reads the file as text and applies
/// the line-based regex parser described in the module docstring.
fn scan_one_toml(path: &Path) -> io::Result<PerFileScan> {
    let content = fs::read_to_string(path)?;
    let name_re = name_field_regex();
    let crate_re = canonical_crate_field_regex();
    let section_re = section_header_regex();

    let mut declared_name: Option<String> = None;
    let mut declares_canonical_crate = false;
    let mut in_section = false;

    for line in content.lines() {
        // Skip whole-line comments early; partial trailing comments on
        // a `name = "..."` line are still matched because the regex
        // anchors on the prefix and stops at the closing quote.
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if section_re.is_match(line) {
            in_section = true;
            continue;
        }
        if in_section {
            continue;
        }
        if declared_name.is_none() {
            if let Some(caps) = name_re.captures(line) {
                if let Some(m) = caps.get(1) {
                    declared_name = Some(m.as_str().to_string());
                }
            }
        }
        if !declares_canonical_crate {
            if let Some(caps) = crate_re.captures(line) {
                if let Some(m) = caps.get(1) {
                    if !m.as_str().is_empty() {
                        declares_canonical_crate = true;
                    }
                }
            }
        }
    }

    // Filename-stem fallback when no top-level `name` field was
    // declared. `file_stem` strips one extension; for a path like
    // `concepts/cfdb.toml` this yields `cfdb`. Non-UTF-8 stems
    // degrade to lossy conversion — concept TOMLs have ASCII names by
    // convention.
    let context_name = declared_name.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });

    Ok(PerFileScan {
        context_name,
        declares_canonical_crate,
    })
}

/// Scan `<workspace>/.cfdb/concepts/*.toml` and return the two counts
/// substituted into the `self-enrich-concepts.cypher` template. The
/// walk is one segment deep — sub-directories are ignored. Missing
/// directory yields `ConceptCounts { 0, 0 }` so the harness can still
/// evaluate the sentinel (and the conditional sentinel (c) trivially
/// skips on `declared_canonical_crate_count == 0`).
pub fn scan_concepts(workspace: &Path) -> io::Result<ConceptCounts> {
    let dir = workspace.join(".cfdb").join("concepts");
    if !dir.is_dir() {
        return Ok(ConceptCounts {
            distinct_context_names: 0,
            declared_canonical_crate_count: 0,
        });
    }

    let mut distinct_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut declared_canonical_crate_count: usize = 0;

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let scan = scan_one_toml(&path)?;
        distinct_names.insert(scan.context_name);
        if scan.declares_canonical_crate {
            declared_canonical_crate_count += 1;
        }
    }

    Ok(ConceptCounts {
        distinct_context_names: distinct_names.len(),
        declared_canonical_crate_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Single TOML with both `name` and `canonical_crate` — the
    /// canonical cfdb-self shape (mirrors `.cfdb/concepts/cfdb.toml`).
    #[test]
    fn scans_single_toml_with_canonical_crate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let concepts_dir = dir.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts_dir).expect("create concepts dir");
        fs::write(
            concepts_dir.join("cfdb.toml"),
            "# header comment\nname = \"cfdb\"\ncanonical_crate = \"cfdb-core\"\n",
        )
        .expect("write toml");

        let counts = scan_concepts(dir.path()).expect("scan succeeds");
        assert_eq!(counts.distinct_context_names, 1);
        assert_eq!(counts.declared_canonical_crate_count, 1);
    }

    /// TOML with `name` but NO `canonical_crate` — `Option<String>` =
    /// None case per `cfdb-concepts/src/lib.rs:65`. The conditional
    /// sentinel (c) skips on `declared_canonical_crate_count == 0`.
    #[test]
    fn scans_toml_without_canonical_crate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let concepts_dir = dir.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts_dir).expect("create concepts dir");
        fs::write(
            concepts_dir.join("alpha.toml"),
            "name = \"alpha\"\ncrates = []\n",
        )
        .expect("write toml");

        let counts = scan_concepts(dir.path()).expect("scan succeeds");
        assert_eq!(counts.distinct_context_names, 1);
        assert_eq!(
            counts.declared_canonical_crate_count, 0,
            "Option<String> = None case must produce 0"
        );
    }

    /// Empty `canonical_crate = ""` does NOT count — it's the `Some("")`
    /// edge case which the override loader treats as "no canonical
    /// crate declared" semantically. The scanner mirrors that rule.
    #[test]
    fn empty_canonical_crate_string_not_counted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let concepts_dir = dir.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts_dir).expect("create concepts dir");
        fs::write(
            concepts_dir.join("beta.toml"),
            "name = \"beta\"\ncanonical_crate = \"\"\n",
        )
        .expect("write toml");

        let counts = scan_concepts(dir.path()).expect("scan succeeds");
        assert_eq!(counts.distinct_context_names, 1);
        assert_eq!(counts.declared_canonical_crate_count, 0);
    }

    /// Two TOML files declaring the SAME `name = "shared"` value
    /// collapse to one distinct context — deduplication is by NAME,
    /// not by file. The canonical-crate count remains per-file.
    #[test]
    fn deduplicates_distinct_context_names_across_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let concepts_dir = dir.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts_dir).expect("create concepts dir");
        fs::write(
            concepts_dir.join("first.toml"),
            "name = \"shared\"\ncanonical_crate = \"crate-one\"\n",
        )
        .expect("write first.toml");
        fs::write(
            concepts_dir.join("second.toml"),
            "name = \"shared\"\ncanonical_crate = \"crate-two\"\n",
        )
        .expect("write second.toml");

        let counts = scan_concepts(dir.path()).expect("scan succeeds");
        assert_eq!(
            counts.distinct_context_names, 1,
            "two files declaring name = \"shared\" collapse to 1 distinct context"
        );
        assert_eq!(
            counts.declared_canonical_crate_count, 2,
            "canonical-crate declarations are counted per-file"
        );
    }

    /// TOML with no `name` field falls back to filename stem.
    #[test]
    fn falls_back_to_filename_stem_when_name_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let concepts_dir = dir.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts_dir).expect("create concepts dir");
        fs::write(
            concepts_dir.join("nameless.toml"),
            "# no top-level name\ncrates = []\n",
        )
        .expect("write toml");

        let counts = scan_concepts(dir.path()).expect("scan succeeds");
        assert_eq!(
            counts.distinct_context_names, 1,
            "filename stem 'nameless' substitutes for missing name field"
        );
        assert_eq!(counts.declared_canonical_crate_count, 0);
    }

    /// `name` / `canonical_crate` declared INSIDE a `[section]` header
    /// are NOT top-level fields and must be ignored. The scanner falls
    /// back to the filename stem and reports zero canonical-crate
    /// declarations.
    #[test]
    fn ignores_fields_inside_section_headers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let concepts_dir = dir.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts_dir).expect("create concepts dir");
        fs::write(
            concepts_dir.join("sectioned.toml"),
            "[metadata]\nname = \"not-top-level\"\ncanonical_crate = \"also-not\"\n",
        )
        .expect("write toml");

        let counts = scan_concepts(dir.path()).expect("scan succeeds");
        assert_eq!(
            counts.distinct_context_names, 1,
            "section-scoped name is ignored; falls back to stem 'sectioned'"
        );
        assert_eq!(
            counts.declared_canonical_crate_count, 0,
            "section-scoped canonical_crate is ignored"
        );
    }

    /// Missing `.cfdb/concepts/` directory returns zero counts (no
    /// error) so the harness can still substitute the placeholders and
    /// the conditional sentinel (c) trivially passes.
    #[test]
    fn missing_concepts_directory_returns_zero_counts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let counts = scan_concepts(dir.path()).expect("scan succeeds on absent dir");
        assert_eq!(counts.distinct_context_names, 0);
        assert_eq!(counts.declared_canonical_crate_count, 0);
    }

    /// Non-TOML files in the directory are ignored (defensive — the
    /// harness should never see foreign files but production roots
    /// occasionally accumulate `.bak` / `.swp` cruft).
    #[test]
    fn ignores_non_toml_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let concepts_dir = dir.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts_dir).expect("create concepts dir");
        fs::write(
            concepts_dir.join("real.toml"),
            "name = \"real\"\ncanonical_crate = \"real-crate\"\n",
        )
        .expect("write real.toml");
        fs::write(
            concepts_dir.join("real.toml.bak"),
            "name = \"backup-noise\"\ncanonical_crate = \"noise-crate\"\n",
        )
        .expect("write backup");
        fs::write(concepts_dir.join("README.md"), "# not a concept").expect("write README");

        let counts = scan_concepts(dir.path()).expect("scan succeeds");
        assert_eq!(counts.distinct_context_names, 1, "only real.toml counts");
        assert_eq!(counts.declared_canonical_crate_count, 1);
    }
}
