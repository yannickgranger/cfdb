use std::collections::BTreeSet;
use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;
use cfdb_extractor::extract_workspace;

fn emitted_item_kinds(root: &Path) -> BTreeSet<String> {
    let (nodes, _) = extract_workspace(root).expect("extract_workspace on the kinds fixture");
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .filter_map(|n| n.props.get("kind"))
        .filter_map(PropValue::as_str)
        .map(str::to_owned)
        .collect()
}

#[test]
fn extractor_emits_static_and_union_item_kinds() {
    let root = Path::new("tests/fixtures/item_kinds_fixture");
    assert!(
        root.join("Cargo.toml").is_file(),
        "fixture missing at {} — cargo test must run with CWD at the crate root",
        root.display()
    );

    let kinds = emitted_item_kinds(root);
    assert!(
        kinds.contains("static"),
        "fixture declares `pub static ANSWER` but no :Item {{ kind: \"static\" }} \
         was emitted — kinds seen: {kinds:?}"
    );
    assert!(
        kinds.contains("union"),
        "fixture declares `pub union RawBits` but no :Item {{ kind: \"union\" }} \
         was emitted — the aspirational recall row (KEPT_ITEM_KINDS) is \
         unbacked; kinds seen: {kinds:?}"
    );
}
