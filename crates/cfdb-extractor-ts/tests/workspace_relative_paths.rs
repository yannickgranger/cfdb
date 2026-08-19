use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_extractor_ts::TypeScriptProducer;
use cfdb_lang::LanguageProducer;

#[test]
fn dot_dot_terminated_root_still_names_the_crate_after_the_directory() {
    let root = Path::new("tests/fixtures/ts-minimal/src/..");
    assert!(
        root.join("tsconfig.json").is_file(),
        "fixture missing at {} — cargo test must run with CWD at the crate root",
        root.display()
    );

    let (nodes, _) = TypeScriptProducer
        .produce(root)
        .expect("produce on the fixture must succeed");

    let crate_node = nodes
        .iter()
        .find(|n| n.label.as_str() == "Crate")
        .expect("producer must emit a :Crate node");
    let name = match crate_node.props.get("name") {
        Some(PropValue::Str(s)) => s.as_str(),
        other => panic!(":Crate.name must be a string prop, got {other:?}"),
    };
    assert_eq!(
        name, "ts-minimal",
        "crate must be named after the canonical directory, not the \
         silent `ts_workspace` fallback"
    );
}

#[test]
fn relative_workspace_argument_yields_workspace_relative_file_props() {
    let root = Path::new("tests/fixtures/ts-minimal");
    let (nodes, _) = TypeScriptProducer
        .produce(root)
        .expect("produce on the fixture must succeed");

    let mut checked = 0;
    for node in &nodes {
        if let Some(PropValue::Str(file)) = node.props.get("file") {
            checked += 1;
            assert!(
                !file.starts_with('/'),
                "absolute `file` prop leaked from a relative-root produce(): {file}"
            );
            assert!(
                !file.contains("fixtures"),
                "`file` prop still carries the workspace-root prefix: {file}"
            );
        }
    }
    assert!(
        checked > 0,
        "fixture must emit at least one file-carrying node"
    );
}
