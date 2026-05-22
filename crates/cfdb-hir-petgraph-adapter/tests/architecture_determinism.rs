//! Architecture test — source-level determinism invariants for cfdb-hir-petgraph-adapter.
//!
//! RFC-029 §12.1 G1 requires every fact-emitting crate to produce
//! byte-identical output across runs. That demands at SOURCE level:
//!
//! - `BTreeMap` / `BTreeSet` — not `HashMap` / `HashSet` (insertion-order non-determinism)
//! - Stable sort — `sort` / `sort_by` / `sort_by_key`, never `sort_unstable*`
//! - Single-threaded writes — no `rayon`, `par_iter`, `parallel_bridge`, or `thread::{spawn,scope}`
//! - No wall-clock reads — no `SystemTime`, `Instant::now`, `chrono::Utc::now`, `chrono::Local::now`
//!
//! This is the RFC-044 §3.5 propagation of the original
//! `cfdb-extractor/tests/architecture_determinism.rs` gate to every sibling
//! emitter crate. Each copy is self-contained — no shared ban-list, no shared
//! runner — the canonical example of acceptable duplication per the
//! no-monolith directive (RFC-044 §I1). A future contributor adding
//! `use std::collections::HashMap;` to prod source would silently break G1;
//! this test catches the drift at compile time, in every profile, in ms.
//!
//! **Scope:** prod source under `src/` only. Test code is excluded two ways:
//! (1) separate test-sibling files (`tests.rs`, `*_tests.rs`) are skipped by
//! name — they legitimately use `Instant::now` (perf timing) and `"chrono::Utc::now"`
//! string fixtures; (2) inline `#[cfg(test)]` items are stripped before
//! scanning. This dual exclusion is the RFC-044 hardening over the original
//! cfdb-extractor gate (which relied on inline-stripping alone because its own
//! separate test files happened to be clean).

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn cfdb_hir_petgraph_adapter_src_has_no_determinism_breaking_patterns() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = collect_prod_rs_files(&src_root);
    assert!(
        !files.is_empty(),
        "no prod .rs files under {} — is the crate layout broken?",
        src_root.display()
    );

    // Each entry: (substring to search, reason / remediation).
    let forbidden: &[(&str, &str)] = &[
        ("HashMap", "use `BTreeMap` — HashMap iteration order is nondeterministic"),
        ("HashSet", "use `BTreeSet` — HashSet iteration order is nondeterministic"),
        ("sort_unstable", "use stable `sort` / `sort_by` / `sort_by_key` — unstable sort has nondeterministic tie-breaking"),
        ("rayon", "no parallel iteration in the write phase (RFC-029 §12.1 G1)"),
        ("par_iter", "no parallel iteration in the write phase"),
        ("parallel_bridge", "no parallel iteration in the write phase"),
        ("thread::spawn", "no concurrent writes — single-threaded write phase"),
        ("thread::scope", "no concurrent writes — single-threaded write phase"),
        ("SystemTime", "no wall-clock reads — emitters are purely structural"),
        ("Instant::now", "no wall-clock reads — emitters are purely structural"),
        ("Utc::now", "no wall-clock reads — emitters are purely structural"),
        ("Local::now", "no wall-clock reads — emitters are purely structural"),
    ];

    let mut violations: Vec<String> = Vec::new();

    for path in &files {
        let source = fs::read_to_string(path).expect("read .rs file");
        let prod_source = strip_test_scopes(&source);
        for (pattern, reason) in forbidden {
            if prod_source.contains(pattern) {
                let rel = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .unwrap_or(path);
                violations.push(format!("  {}: `{}` — {}", rel.display(), pattern, reason));
            }
        }
    }

    violations.sort();
    assert!(
        violations.is_empty(),
        "\ncfdb-hir-petgraph-adapter prod source contains determinism-breaking patterns (RFC-029 §12.1 G1):\n\n{}\n\nFix: remove the forbidden pattern, use the suggested alternative, or move the usage into a `#[cfg(test)]` block / a `*_tests.rs` sibling file if it is a test fixture.\n",
        violations.join("\n")
    );
}

/// Collect every prod `.rs` file under `dir`, skipping separate test-sibling
/// files by name (`tests.rs`, `*_tests.rs`) — those carry test-only fixtures
/// (`Instant::now`, `"Utc::now"` strings) that are not production code.
fn collect_prod_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

fn is_test_sibling(path: &Path) -> bool {
    match path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => stem == "tests" || stem.ends_with("_tests"),
        None => false,
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs")
                && !is_test_sibling(&path)
            {
                out.push(path);
            }
        }
    }
}

/// Strip `#[cfg(test)]`-gated items and full-line `//` comments from `source`.
///
/// Handles both `#[cfg(test)] mod tests { ... }` (inline body, depth-counted)
/// and `#[cfg(test)] mod tests;` (external module reference, single-statement
/// skip). Full-line comments starting with `//`, `///`, or `//!` are elided so
/// the pattern scan does not match text inside doc comments describing the
/// forbidden patterns in prose.
///
/// Coarse by design: does not handle braces / semicolons embedded in string
/// literals or block comments inside an inline `#[cfg(test)]` block. Separate
/// test-sibling files are already excluded by `collect_prod_rs_files`, so the
/// residual risk is limited to inline test mods in prod-named files.
fn strip_test_scopes(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("#[cfg(test)]") {
            i += 1;
            let mut depth: i32 = 0;
            let mut found_open = false;
            'skip: while i < lines.len() {
                for ch in lines[i].chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            found_open = true;
                        }
                        '}' => {
                            depth -= 1;
                            if found_open && depth == 0 {
                                i += 1;
                                break 'skip;
                            }
                        }
                        ';' if !found_open => {
                            i += 1;
                            break 'skip;
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
            continue;
        }
        if trimmed.starts_with("//") {
            i += 1;
            continue;
        }
        out.push_str(lines[i]);
        out.push('\n');
        i += 1;
    }
    out
}
