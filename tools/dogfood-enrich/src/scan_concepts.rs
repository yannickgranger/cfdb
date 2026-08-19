use std::fs;
use std::io;
use std::path::Path;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConceptCounts {
    pub distinct_context_names: usize,
    pub declared_canonical_crate_count: usize,
}

fn name_field_regex() -> Regex {
    Regex::new(r#"^\s*name\s*=\s*"([^"]*)""#).expect("static regex compiles")
}

fn canonical_crate_field_regex() -> Regex {
    Regex::new(r#"^\s*canonical_crate\s*=\s*"([^"]*)""#).expect("static regex compiles")
}

fn section_header_regex() -> Regex {
    Regex::new(r#"^\s*\["#).expect("static regex compiles")
}

struct PerFileScan {
    context_name: String,
    declares_canonical_crate: bool,
}

fn scan_one_toml(path: &Path) -> io::Result<PerFileScan> {
    let content = fs::read_to_string(path)?;
    let name_re = name_field_regex();
    let crate_re = canonical_crate_field_regex();
    let section_re = section_header_regex();

    let mut declared_name: Option<String> = None;
    let mut declares_canonical_crate = false;
    let mut in_section = false;

    for line in content.lines() {
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

    #[test]
    fn missing_concepts_directory_returns_zero_counts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let counts = scan_concepts(dir.path()).expect("scan succeeds on absent dir");
        assert_eq!(counts.distinct_context_names, 0);
        assert_eq!(counts.declared_canonical_crate_count, 0);
    }

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
