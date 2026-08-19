use cfdb_concepts::load_published_language_crates;

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

    assert!(loaded.is_published_language("qbot-prelude"));
    assert!(loaded.is_published_language("qbot-types"));
    assert!(loaded.is_published_language("execution-primitives"));
    assert!(!loaded.is_published_language("cfdb-core"));
    assert!(!loaded.is_published_language(""));

    assert_eq!(loaded.owning_context("qbot-prelude"), Some("core"));
    assert_eq!(loaded.owning_context("qbot-types"), Some("core"));
    assert_eq!(
        loaded.owning_context("execution-primitives"),
        Some("execution")
    );
    assert_eq!(loaded.owning_context("cfdb-core"), None);

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

#[test]
fn workspace_without_cfdb_dir_returns_empty_loader() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let loaded = load_published_language_crates(tmp.path()).expect("load ok");
    assert!(!loaded.is_published_language("anything"));
    assert_eq!(loaded.owning_context("anything"), None);
    assert_eq!(loaded.allowed_consumers("anything"), None);
}

#[test]
fn cfdb_dir_without_pl_file_returns_empty() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfdb = tmp.path().join(".cfdb");
    std::fs::create_dir_all(&cfdb).expect("mkdir .cfdb");
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
