use std::fs;
use std::path::{Path, PathBuf};

const EVAL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/eval");

const FORBIDDEN: &[&str] = &["petgraph", "KeyspaceState", ".raw()", "from_raw("];

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
    rust_sources(Path::new(EVAL_DIR), &mut files);
    assert!(
        files.len() >= 8,
        "expected the evaluator's production sources under {EVAL_DIR}, found {}",
        files.len()
    );
    let mut out = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).expect("read evaluator source");
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            if FORBIDDEN.iter().any(|needle| line.contains(needle)) {
                out.push((file.clone(), n + 1, line.trim().to_string()));
            }
        }
    }
    out
}

#[test]
fn evaluator_production_sources_speak_only_graph_reader() {
    let found = violations();
    assert!(
        found.is_empty(),
        "evaluator production sources reach past GraphReader:\n{}",
        found
            .iter()
            .map(|(f, n, l)| format!("  {}:{n}: {l}", f.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
