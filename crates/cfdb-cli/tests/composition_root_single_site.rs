use std::fs;
use std::path::{Path, PathBuf};

const CONSTRUCTOR: &str = "PetgraphStore::new(";

const COMPOSITION_ROOT: &str = "compose.rs";

fn cfdb_cli_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
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
fn petgraph_store_constructed_only_in_compose() {
    let src = cfdb_cli_src();
    assert!(
        src.is_dir(),
        "expected `crates/cfdb-cli/src/` at {}",
        src.display()
    );

    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    assert!(
        !files.is_empty(),
        "non-vacuity guard: scanned zero .rs files under {} — check the walk root",
        src.display()
    );

    let mut sites: Vec<String> = Vec::new();
    for file in &files {
        let contents = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {} failed: {e}", file.display()));
        if contents.contains(CONSTRUCTOR) {
            let rel = file
                .strip_prefix(&src)
                .unwrap_or(file.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            sites.push(rel);
        }
    }
    sites.sort();

    assert!(
        sites.iter().any(|s| s == COMPOSITION_ROOT),
        "non-vacuity guard: `{CONSTRUCTOR}` not found in `{COMPOSITION_ROOT}` — \
         the constructor token or scan root is stale (sites found: {sites:?})"
    );

    let extra: Vec<&String> = sites.iter().filter(|s| *s != COMPOSITION_ROOT).collect();
    assert!(
        extra.is_empty(),
        "composition-root violation (RFC-044 §3.3 / CLEAN-3): `{CONSTRUCTOR}` \
         appears in cfdb-cli/src outside `{COMPOSITION_ROOT}`: {extra:?}\n\
         Only `compose.rs` may construct the store; other modules must receive \
         it from their caller (e.g. `hir.rs` takes `store: &mut PetgraphStore`)."
    );
}
