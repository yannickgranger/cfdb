//! Architecture test — source-level determinism invariants for cfdb-extractor.
//!
//! RFC-029 §12.1 G1 requires the extractor to produce byte-identical output
//! across runs. That demands at SOURCE level:
//!
//! - `BTreeMap` / `BTreeSet` — not `HashMap` / `HashSet` (insertion-order non-determinism)
//! - Stable sort — `sort` / `sort_by` / `sort_by_key`, never `sort_unstable*`
//! - Single-threaded writes — no `rayon`, `par_iter`, `parallel_bridge`, or `thread::{spawn,scope}`
//! - No wall-clock reads — no `SystemTime`, `Instant::now`, `chrono::Utc::now`, `chrono::Local::now`
//!
//! Without this gate a future contributor adding `use std::collections::HashMap;`
//! would silently break the G1 guarantee — the existing runtime equivalence test
//! at `lib.rs:1549` (`extractor_is_deterministic_across_two_runs`) would eventually
//! catch the drift, but only after the behavior already regressed. This test
//! prevents the drift at compile time instead.
//!
//! **Scope:** only prod source under `src/`. Items gated on `#[cfg(test)]` are
//! stripped before scanning because test fixtures legitimately use `HashMap`
//! (order-insensitive assertions) and `chrono::Utc::now` (synthetic call-site
//! fixtures for the `arch-ban-utc-now.cypher` rule regression tests).
//!
//! **Implementation:** line-based grep with `#[cfg(test)]` depth-counted
//! stripping. Satisfies the AC's "grep-based or syn-based" option with no
//! extra dev-dependencies.

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn cfdb_extractor_src_has_no_determinism_breaking_patterns() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = collect_rs_files(&src_root);
    assert!(
        !files.is_empty(),
        "no .rs files under {} — is the crate layout broken?",
        src_root.display()
    );

    // Each entry: (substring to search, reason / remediation).
    // Substring matching on a test-scope-stripped source is sufficient for v0.1.
    let forbidden: &[(&str, &str)] = &[
        ("HashMap", "use `BTreeMap` — HashMap iteration order is nondeterministic"),
        ("HashSet", "use `BTreeSet` — HashSet iteration order is nondeterministic"),
        ("sort_unstable", "use stable `sort` / `sort_by` / `sort_by_key` — unstable sort has nondeterministic tie-breaking"),
        ("rayon", "no parallel iteration in the extractor write phase (RFC-029 §12.1 G1)"),
        ("par_iter", "no parallel iteration in the extractor write phase"),
        ("parallel_bridge", "no parallel iteration in the extractor write phase"),
        ("thread::spawn", "no concurrent writes — single-threaded write phase"),
        ("thread::scope", "no concurrent writes — single-threaded write phase"),
        ("SystemTime", "no wall-clock reads — extractor is purely structural"),
        ("Instant::now", "no wall-clock reads — extractor is purely structural"),
        ("Utc::now", "no wall-clock reads — extractor is purely structural"),
        ("Local::now", "no wall-clock reads — extractor is purely structural"),
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
        "\ncfdb-extractor prod source contains determinism-breaking patterns (RFC-029 §12.1 G1):\n\n{}\n\nFix: remove the forbidden pattern, use the suggested alternative, or move the usage inside `#[cfg(test)] mod tests {{ }}` if it is a test fixture.\n",
        violations.join("\n")
    );
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
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
/// the pattern scan does not match text inside doc comments that describes
/// forbidden patterns in prose.
///
/// Coarse by design: does not handle braces or semicolons embedded inside
/// string literals or block comments. For cfdb-extractor v0.1 this is
/// acceptable because the crate has no string literals carrying `{`, `}`, or
/// `;` at a `#[cfg(test)]`-gated position.
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
