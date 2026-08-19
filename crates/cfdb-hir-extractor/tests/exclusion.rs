use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_TOKENS: &[&str] = &["Label::ITEM", "Label::CRATE", "Label::MODULE"];

fn crate_src_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("src")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}) failed: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("directory entry readable");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn cfdb_hir_extractor_does_not_emit_item_crate_or_module_labels() {
    let src = crate_src_root();
    assert!(src.is_dir(), "expected crate src dir at {}", src.display(),);

    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);

    assert!(
        !files.is_empty(),
        "non-vacuity guard: scanned zero .rs files under {}",
        src.display(),
    );

    type Hit = (usize, String, String);
    let mut offenders: Vec<(PathBuf, Vec<Hit>)> = Vec::new();
    for file in &files {
        let contents = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {} failed: {e}", file.display()));
        let mut hits: Vec<Hit> = Vec::new();
        for (lineno, line) in contents.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for token in FORBIDDEN_TOKENS {
                if line.contains(token) {
                    hits.push((
                        lineno + 1,
                        (*token).to_string(),
                        line.trim_end().to_string(),
                    ));
                }
            }
        }
        if !hits.is_empty() {
            offenders.push((file.clone(), hits));
        }
    }

    if !offenders.is_empty() {
        let mut msg = String::from(
            "cfdb-hir-extractor references a structural-extraction label \
             (:Item, :Crate, or :Module) — those belong exclusively to \
             cfdb-extractor (syn-based). Either delete the reference or \
             move the logic back to cfdb-extractor. See Issue #84 AC-6 \
             and #40 umbrella.\n",
        );
        for (file, hits) in &offenders {
            msg.push_str(&format!("  {}:\n", file.display()));
            for (lineno, token, text) in hits {
                msg.push_str(&format!("    line {lineno} [{token}]: {text}\n"));
            }
        }
        panic!("{msg}");
    }
}
