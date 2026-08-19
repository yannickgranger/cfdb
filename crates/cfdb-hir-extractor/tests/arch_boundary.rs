use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_TOKEN: &str = "ra_ap_";

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("crate manifest resolves to `<workspace>/crates/<crate>`")
        .to_path_buf()
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
fn cfdb_core_contains_no_ra_ap_token() {
    let root = workspace_root();
    let cfdb_core_src = root.join("crates").join("cfdb-core").join("src");
    assert!(
        cfdb_core_src.is_dir(),
        "expected `crates/cfdb-core/src/` to exist at {}",
        cfdb_core_src.display(),
    );

    let mut files = Vec::new();
    collect_rs_files(&cfdb_core_src, &mut files);

    assert!(
        !files.is_empty(),
        "non-vacuity guard: scanned zero .rs files under {} — \
         check the walk root",
        cfdb_core_src.display(),
    );

    let mut offenders: Vec<(PathBuf, Vec<(usize, String)>)> = Vec::new();
    for file in &files {
        let contents = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {} failed: {e}", file.display()));
        let mut hits: Vec<(usize, String)> = Vec::new();
        for (lineno, line) in contents.lines().enumerate() {
            if line.contains(FORBIDDEN_TOKEN) {
                hits.push((lineno + 1, line.trim_end().to_string()));
            }
        }
        if !hits.is_empty() {
            offenders.push((file.clone(), hits));
        }
    }

    if !offenders.is_empty() {
        let mut msg = format!(
            "cfdb-core contains `{FORBIDDEN_TOKEN}` tokens — \
             v0.2-6 boundary violation (RFC-029 §A1.2). \
             Move ra-ap-* references into cfdb-hir-extractor or \
             the cfdb-hir-petgraph-adapter.\n"
        );
        for (file, hits) in &offenders {
            let rel = file.strip_prefix(&root).unwrap_or(file.as_path()).display();
            msg.push_str(&format!("  {rel}:\n"));
            for (lineno, text) in hits {
                msg.push_str(&format!("    line {lineno}: {text}\n"));
            }
        }
        panic!("{msg}");
    }
}
