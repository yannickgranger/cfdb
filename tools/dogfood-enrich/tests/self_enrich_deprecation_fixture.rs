use std::fs;

use dogfood_enrich::{grep_deprecated, runner};

const TEMPLATE_REL_PATH: &str = "../../.cfdb/queries/self-enrich-deprecation.cypher";

fn build_known_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("a.rs"),
        "#[deprecated]\nfn a() {}\n\n#[deprecated(note = \"x\")]\nfn b() {}\n",
    )
    .expect("write a.rs");
    fs::create_dir_all(root.join("nested")).expect("nested dir");
    fs::write(
        root.join("nested/b.rs"),
        "#[deprecated(since = \"1.0\")]\nfn c() {}\n",
    )
    .expect("write nested/b.rs");
    fs::write(
        root.join("decoy.rs"),
        "//! `#[deprecated]` in docs\nfn d() { let s = \"#[deprecated]\"; }\n",
    )
    .expect("write decoy.rs");
    fs::write(root.join("unwalked.rs"), "#[deprecated]\nfn e() {}\n").expect("write unwalked.rs");

    dir
}

fn walked_set() -> Vec<String> {
    vec![
        "a.rs".to_string(),
        "decoy.rs".to_string(),
        "nested/b.rs".to_string(),
    ]
}

#[test]
fn broken_extractor_simulation_satisfies_sentinel_predicate() {
    let dir = build_known_workspace();
    let source_count = grep_deprecated::count_deprecated_in_files(dir.path(), &walked_set())
        .expect("reads succeed");
    assert_eq!(
        source_count, 3,
        "fixture must produce a deterministic ground truth of 3 genuine \
         attribute-position occurrences: a.rs (2) + nested/b.rs (1); \
         decoy.rs comment/string mentions and unwalked.rs must not count"
    );

    let template = fs::read_to_string(TEMPLATE_REL_PATH).unwrap_or_else(|e| {
        panic!(
            "expected to read shipped template at {TEMPLATE_REL_PATH}: {e}\n\
             cwd: {:?}",
            std::env::current_dir().ok()
        )
    });
    assert!(
        template.contains("{{ ground_truth_count }}"),
        "shipped template must reference {{{{ ground_truth_count }}}} placeholder \
         (this fixture's contract). Template body:\n{template}"
    );

    let count_str = source_count.to_string();
    let materialized =
        runner::substitute_named(&template, &[("ground_truth_count", count_str.as_str())]);

    assert!(
        !materialized.contains("{{ ground_truth_count }}"),
        "post-substitution Cypher must not contain unbound placeholders. \
         Materialized:\n{materialized}"
    );

    let occurrences = materialized.matches(&source_count.to_string()).count();
    assert!(
        occurrences >= 2,
        "expected source_count={source_count} to appear ≥2 times in \
         materialized Cypher (WHERE and RETURN). Found {occurrences}.\n\
         Materialized:\n{materialized}"
    );

    let broken_extracted = source_count - 1;
    assert!(
        broken_extracted < source_count,
        "broken-extractor simulation: extracted={broken_extracted} < source={source_count}. \
         The materialized WHERE clause `extracted_count < {source_count}` is satisfied — \
         the sentinel emits one row, dogfood-enrich exits 30."
    );
}

#[test]
fn healthy_extractor_simulation_passes_sentinel_predicate() {
    let dir = build_known_workspace();
    let source_count = grep_deprecated::count_deprecated_in_files(dir.path(), &walked_set())
        .expect("reads succeed");

    let template = fs::read_to_string(TEMPLATE_REL_PATH).unwrap_or_else(|e| {
        panic!("expected to read shipped template at {TEMPLATE_REL_PATH}: {e}")
    });
    let count_str = source_count.to_string();
    let materialized =
        runner::substitute_named(&template, &[("ground_truth_count", count_str.as_str())]);

    let executable: String = materialized
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        executable.contains(&format!("extracted_count < {source_count}")),
        "materialized sentinel must compare with STRICT less-than \
         (`extracted_count < {source_count}`). Executable Cypher:\n{executable}"
    );
    assert!(
        !executable.contains("extracted_count <="),
        "a `<=` comparison would emit a violation row on the healthy \
         extracted == source case. Executable Cypher:\n{executable}"
    );
}
