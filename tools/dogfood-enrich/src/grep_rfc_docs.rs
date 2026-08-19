use std::fs;
use std::io;
use std::path::Path;

fn is_rfc_md_filename(name: &str) -> bool {
    name.starts_with("RFC-") && name.ends_with(".md")
}

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

    #[test]
    fn filename_matches_numbered_rfc() {
        assert!(is_rfc_md_filename("RFC-039-dogfood-enrichment-passes.md"));
        assert!(is_rfc_md_filename("RFC-001-foo.md"));
    }

    #[test]
    fn filename_matches_kebab_only_rfc() {
        assert!(is_rfc_md_filename("RFC-cfdb.md"));
    }

    #[test]
    fn filename_rejects_no_dash() {
        assert!(!is_rfc_md_filename("RFC.md"));
    }

    #[test]
    fn filename_rejects_wrong_prefix() {
        assert!(!is_rfc_md_filename("some-rfc-foo.md"));
        assert!(!is_rfc_md_filename("rfc-039-foo.md"));
        assert!(!is_rfc_md_filename("notes-RFC-039.md"));
    }

    #[test]
    fn filename_rejects_wrong_extension() {
        assert!(!is_rfc_md_filename("RFC-039-foo.txt"));
        assert!(!is_rfc_md_filename("RFC-039-foo"));
    }

    #[test]
    fn count_walks_docs_and_filters_to_rfc_glob() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let docs = root.join("docs");
        std::fs::create_dir_all(&docs).expect("docs dir");

        std::fs::write(docs.join("RFC-001-alpha.md"), "# alpha\n").expect("write a");
        std::fs::write(docs.join("RFC-002-beta.md"), "# beta\n").expect("write b");
        std::fs::write(docs.join("RFC-cfdb.md"), "# umbrella\n").expect("write c");

        std::fs::write(docs.join("RFC.md"), "# no-dash\n").expect("write no-dash");
        std::fs::write(docs.join("some-rfc-foo.md"), "# wrong prefix\n").expect("write wp");
        std::fs::write(docs.join("RFC-003-gamma.txt"), "# wrong ext\n").expect("write we");
        std::fs::write(docs.join("README.md"), "# readme\n").expect("write readme");

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

    #[test]
    fn count_zero_on_empty_docs_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("docs")).expect("docs dir");
        let count = count_rfc_md_files(dir.path()).expect("walk succeeds");
        assert_eq!(count, 0);
    }

    #[test]
    fn count_zero_on_missing_docs_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let count = count_rfc_md_files(dir.path()).expect("missing-docs walk succeeds (no-op)");
        assert_eq!(count, 0);
    }

    #[test]
    fn count_zero_on_nonexistent_root() {
        let count = count_rfc_md_files(Path::new(
            "/nonexistent/path/zzz/dogfood-enrich-rfc-docs-test",
        ))
        .expect("non-dir walk succeeds (no-op)");
        assert_eq!(count, 0);
    }
}
