use std::path::{Path, PathBuf};
use std::process::Command;

const FORBIDDEN: &[&str] = &["SkillRouting", "skill-routing.toml"];

fn is_historical(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    (s.starts_with("docs/RFC-") && s.ends_with(".md"))
        || s == "CHANGELOG.md"
        || s.starts_with(".context/")
        || s.starts_with(".discovery/")
        || s.starts_with(".prescriptions/")
        || s.starts_with(".proofs/")
        || s.ends_with("tests/skill_routing_deleted.rs")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn tracked_files(root: &Path) -> Vec<PathBuf> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git ls-files failed");
    out.stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| PathBuf::from(String::from_utf8_lossy(p).into_owned()))
        .collect()
}

#[test]
fn no_skill_routing_loader_table_or_toml_anywhere_in_the_tracked_tree() {
    let root = workspace_root();
    let files = tracked_files(&root);
    assert!(
        files.len() > 100,
        "walked only {} tracked files — the walk is not looking at the workspace",
        files.len()
    );

    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for rel in &files {
        if is_historical(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        scanned += 1;
        for (n, line) in text.lines().enumerate() {
            for needle in FORBIDDEN {
                if line.contains(needle) {
                    hits.push(format!("{}:{}: {}", rel.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(scanned > 100, "scanned only {scanned} text files");
    assert!(
        hits.is_empty(),
        "skill routing must stay external to cfdb — {} reappearance(s) of {:?}:\n  {}",
        hits.len(),
        FORBIDDEN,
        hits.join("\n  ")
    );
}
