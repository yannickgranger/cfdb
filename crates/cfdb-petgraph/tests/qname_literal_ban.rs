//! Architecture test — static ban on hand-written `:Item` node-id
//! literals in cfdb-petgraph production source (RFC-044 §3.4 sub-band 2).
//!
//! The canonical `:Item` node-id formula is `item:<qname>`, owned by
//! `cfdb_core::qname::item_node_id`. Every production site that needs an
//! `:Item` id MUST route through that helper, never through a
//! hand-written `format!("item:{...}")`. A scattered literal is a
//! split-brain: if the prefix convention ever changes (or a new
//! discriminator is added to the id formula), the helper updates but the
//! literal silently drifts, producing dangling cross-extractor edges —
//! exactly the failure mode `cfdb_core::qname` exists to prevent.
//!
//! **Scope:** prod source under `src/` only. Test code is excluded two
//! ways, mirroring `architecture_determinism.rs`:
//! (1) separate test-sibling files (`tests.rs`, `*_tests.rs`) are skipped
//! by name — they legitimately build synthetic `item:` ids as fixtures;
//! (2) inline `#[cfg(test)]` items are stripped before scanning. Test
//! fixtures may construct synthetic node ids by hand; only PRODUCTION
//! code must go through `item_node_id`.
//!
//! **Why a static file scan, not a Cypher rule.** RFC-044 §3.4 prescribes
//! the file-scan test as the required mechanism; the optional
//! `arch-ban-qname-literal.cypher` variant is out of scope for this slice.
//! A file scan catches the violation in every profile in milliseconds
//! without invoking rustc, and the shape we prevent is purely textual
//! ("someone typed `format!(\"item:` into a prod .rs file").
//!
//! **Counter-check (non-vacuity).** The test asserts at least one prod
//! `.rs` file was scanned. If the walk root resolves incorrectly the test
//! fails rather than silently passing on an empty scan.

use std::fs;
use std::path::{Path, PathBuf};

/// The forbidden literal. We match the opening of an item-id format
/// string — `format!("item:` — without requiring the closing `}` so the
/// pattern fires on every interpolated form (`format!("item:{qname}")`,
/// `format!("item:{module}::{callee}")`, …). The canonical replacement is
/// `cfdb_core::qname::item_node_id(&qname)`.
const FORBIDDEN: &str = "format!(\"item:";

#[test]
fn cfdb_petgraph_prod_source_routes_item_ids_through_item_node_id() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = collect_prod_rs_files(&src_root);
    assert!(
        !files.is_empty(),
        "non-vacuity guard: scanned zero prod .rs files under {} — \
         is the crate layout broken / the walk root wrong?",
        src_root.display()
    );

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let source = fs::read_to_string(path).expect("read .rs file");
        let prod_source = strip_test_scopes(&source);
        if prod_source.contains(FORBIDDEN) {
            let rel = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(path);
            violations.push(format!("  {}", rel.display()));
        }
    }

    violations.sort();
    assert!(
        violations.is_empty(),
        "\ncfdb-petgraph prod source builds `:Item` node ids by hand \
         (RFC-044 §3.4):\n\n{}\n\nFix: replace `format!(\"item:{{q}}\")` with \
         `cfdb_core::qname::item_node_id(&q)` — the canonical formula owner. \
         If the literal is a synthetic test fixture, move it into a \
         `#[cfg(test)]` block or a `*_tests.rs` sibling file.\n",
        violations.join("\n")
    );
}

/// Collect every prod `.rs` file under `dir`, skipping separate
/// test-sibling files by name (`tests.rs`, `*_tests.rs`) — those carry
/// synthetic `item:` fixtures that are not production code.
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
/// the pattern scan does not match the forbidden literal quoted in prose
/// (e.g. this very test's remediation message in a doc comment).
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
