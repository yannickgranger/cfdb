use std::fs;
use std::path::{Path, PathBuf};

const SRC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");

const FORBIDDEN_EVERYWHERE: &[&str] = &[
    "petgraph",
    "PetgraphStore",
    "KeyspaceState",
    ".raw()",
    "from_raw(",
    "println!",
    "eprint",
];

const FORBIDDEN_UNDER_CHECK: &[&str] = &["load_store", "parse_and_execute"];

fn is_test_source(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name == "tests.rs" || name.ends_with("_tests.rs")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") && !is_test_source(&path) {
            out.push(path);
        }
    }
}

fn violations() -> Vec<(PathBuf, usize, String)> {
    let mut files = Vec::new();
    rust_sources(Path::new(SRC_DIR), &mut files);
    assert!(
        files.len() >= 4,
        "expected the crate's production sources under {SRC_DIR}, found {}",
        files.len()
    );
    let check_sources = files
        .iter()
        .filter(|f| {
            f.strip_prefix(SRC_DIR)
                .map(|p| p.starts_with("check"))
                .unwrap_or(false)
        })
        .count();
    assert!(
        check_sources >= 2,
        "expected the trigger runners under {SRC_DIR}/check, found {check_sources}"
    );
    let mut out = Vec::new();
    for file in files {
        let under_check = file
            .strip_prefix(SRC_DIR)
            .map(|p| p.starts_with("check"))
            .unwrap_or(false);
        let text = fs::read_to_string(&file).expect("read source");
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            let hit = FORBIDDEN_EVERYWHERE
                .iter()
                .any(|needle| line.contains(needle))
                || (under_check
                    && FORBIDDEN_UNDER_CHECK
                        .iter()
                        .any(|needle| line.contains(needle)));
            if hit {
                out.push((file.clone(), n + 1, line.trim().to_string()));
            }
        }
    }
    out
}

#[test]
fn judgment_layer_production_sources_speak_only_the_port_and_never_print() {
    let found = violations();
    assert!(
        found.is_empty(),
        "cfdb-classify production sources reach past the port or do I/O:\n{}",
        found
            .iter()
            .map(|(f, n, l)| format!("  {}:{n}: {l}", f.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
