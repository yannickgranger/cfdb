use std::fs;
use std::path::{Path, PathBuf};

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
