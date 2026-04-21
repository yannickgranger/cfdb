//! Integration tests for `.cfdb/published-language-crates.toml` loading.
//!
//! Exercises the full TOML-file → `PublishedLanguageCrates` pipeline over a
//! real filesystem (tempdir), per CLAUDE.md §2.5 test hierarchy row 2.
//! Complements the per-branch unit tests at
//! `crates/cfdb-concepts/src/published_language.rs::tests` by asserting
//! the FULL loaded map's behaviour under realistic multi-entry data.

use cfdb_concepts::load_published_language_crates;

/// Build a tempdir workspace with 3 `[[crate]]` entries covering:
///   - Simple owned+consumers list.
///   - Wildcard consumers (`"*"`).
///   - Distinct owning_context (non-"core") to prove the field flows
///     through unchanged.
#[test]
fn full_pipeline_exercises_all_three_public_methods() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfdb = tmp.path().join(".cfdb");
    std::fs::create_dir_all(&cfdb).expect("mkdir .cfdb");
    std::fs::write(
        cfdb.join("published-language-crates.toml"),
        r#"
# Three-entry fixture — see tests/published_language.rs

[[crate]]
name = "qbot-prelude"
language = "prelude"
owning_context = "core"
consumers = ["trading", "portfolio", "strategy"]

[[crate]]
name = "qbot-types"
language = "types"
owning_context = "core"
consumers = ["*"]

[[crate]]
name = "execution-primitives"
language = "exec-abi"
owning_context = "execution"
consumers = ["trading"]
"#,
    )
    .expect("write toml");

    let loaded = load_published_language_crates(tmp.path()).expect("load ok");

    // is_published_language — mapped and unmapped
    assert!(loaded.is_published_language("qbot-prelude"));
    assert!(loaded.is_published_language("qbot-types"));
    assert!(loaded.is_published_language("execution-primitives"));
    assert!(!loaded.is_published_language("cfdb-core"));
    assert!(!loaded.is_published_language(""));

    // owning_context — including the non-"core" context
    assert_eq!(loaded.owning_context("qbot-prelude"), Some("core"));
    assert_eq!(loaded.owning_context("qbot-types"), Some("core"));
    assert_eq!(
        loaded.owning_context("execution-primitives"),
        Some("execution")
    );
    assert_eq!(loaded.owning_context("cfdb-core"), None);

    // allowed_consumers — including wildcard pass-through
    assert_eq!(
        loaded.allowed_consumers("qbot-prelude"),
        Some(
            [
                "trading".to_string(),
                "portfolio".to_string(),
                "strategy".to_string(),
            ]
            .as_slice()
        )
    );
    assert_eq!(
        loaded.allowed_consumers("qbot-types"),
        Some(["*".to_string()].as_slice())
    );
    assert_eq!(
        loaded.allowed_consumers("execution-primitives"),
        Some(["trading".to_string()].as_slice())
    );
    assert_eq!(loaded.allowed_consumers("cfdb-core"), None);
}

/// A workspace without `.cfdb/` — common greenfield case. Loader MUST
/// return an empty map, NOT an error; every downstream lookup returns
/// `None`/`false`.
#[test]
fn workspace_without_cfdb_dir_returns_empty_loader() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let loaded = load_published_language_crates(tmp.path()).expect("load ok");
    assert!(!loaded.is_published_language("anything"));
    assert_eq!(loaded.owning_context("anything"), None);
    assert_eq!(loaded.allowed_consumers("anything"), None);
}

/// `.cfdb/` exists but no `published-language-crates.toml` — the loader
/// MUST NOT touch `.cfdb/concepts/*.toml` or any other file; only the
/// single named file is its input. Missing file is not an error.
#[test]
fn cfdb_dir_without_pl_file_returns_empty() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfdb = tmp.path().join(".cfdb");
    std::fs::create_dir_all(&cfdb).expect("mkdir .cfdb");
    // Write an unrelated concepts file — the PL loader must ignore it.
    let concepts = cfdb.join("concepts");
    std::fs::create_dir_all(&concepts).expect("mkdir concepts");
    std::fs::write(
        concepts.join("trading.toml"),
        "name = \"trading\"\ncrates = [\"domain-trading\"]\n",
    )
    .expect("write concepts toml");

    let loaded = load_published_language_crates(tmp.path()).expect("load ok");
    assert!(!loaded.is_published_language("domain-trading"));
}
