//! Two bounded contexts share this crate and never import from each other:
//! debt classification (`taxonomy`, `scope`, `classify`, `explain`, `engine`)
//! and the editorial-drift triggers (`check`). The wall is a rule, not an
//! accident — a `check` module naming a `DebtClass`, or a scope module
//! naming a `TriggerId`, is a boundary breach even if it compiles.
//!
//! Test sources are exempt; the trigger half is vacuous until `check` moves
//! in, so the non-vacuity guard sits on the classification half.

use std::fs;
use std::path::{Path, PathBuf};

const SRC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");

/// Vocabulary of the trigger context — must not appear in classification sources.
const TRIGGER_VOCABULARY: &[&str] = &["TriggerId", "ContextRow", "T1Row", "T3Row", "CheckReport"];
/// Vocabulary of the classification context — must not appear under `src/check/`.
const CLASSIFICATION_VOCABULARY: &[&str] = &[
    "DebtClass",
    "ScopeInventory",
    "CanonicalCandidate",
    "ReachabilityEntry",
    "ClassifyEnvelope",
];

fn is_test_source(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name == "tests.rs" || name.ends_with("_tests.rs")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read.map(|e| e.expect("dir entry").path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") && !is_test_source(&path) {
            out.push(path);
        }
    }
}

fn production_lines(file: &Path) -> Vec<(usize, String)> {
    let text = fs::read_to_string(file).expect("read source");
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        out.push((n + 1, line.to_string()));
    }
    out
}

/// A `use` line or a path segment naming one of `words`. Doc comments and
/// plain prose are not imports; only code lines count.
fn names_any(line: &str, words: &[&str]) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    words.iter().any(|w| code.contains(w))
}

#[test]
fn classification_modules_never_name_the_trigger_context() {
    let root = Path::new(SRC_DIR);
    let mut files = Vec::new();
    for module in [
        "taxonomy.rs",
        "classify.rs",
        "explain.rs",
        "engine.rs",
        "scope.rs",
        "lib.rs",
    ] {
        let p = root.join(module);
        if p.exists() {
            files.push(p);
        }
    }
    rust_sources(&root.join("scope"), &mut files);
    rust_sources(&root.join("classify"), &mut files);
    assert!(
        files.len() >= 3,
        "expected the classification modules under {SRC_DIR}, found {}",
        files.len()
    );
    let mut hits = Vec::new();
    for file in &files {
        for (n, line) in production_lines(file) {
            if names_any(&line, TRIGGER_VOCABULARY) {
                hits.push(format!("  {}:{n}: {}", file.display(), line.trim()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "classification modules name the trigger context:\n{}",
        hits.join("\n")
    );
}

#[test]
fn trigger_modules_never_name_the_classification_context() {
    let root = Path::new(SRC_DIR);
    let mut files = Vec::new();
    let check_rs = root.join("check.rs");
    if check_rs.exists() {
        files.push(check_rs);
    }
    rust_sources(&root.join("check"), &mut files);
    // Vacuous until the trigger context moves in; the classification half above
    // carries the non-vacuity guard.
    let mut hits = Vec::new();
    for file in &files {
        for (n, line) in production_lines(file) {
            if names_any(&line, CLASSIFICATION_VOCABULARY) {
                hits.push(format!("  {}:{n}: {}", file.display(), line.trim()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "trigger modules name the classification context:\n{}",
        hits.join("\n")
    );
}
