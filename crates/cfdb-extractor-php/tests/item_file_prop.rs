use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;

fn items_of(root: &Path) -> Vec<cfdb_core::fact::Node> {
    let (nodes, _) = PhpProducer
        .produce(root)
        .expect("produce on the fixture must succeed");
    nodes
        .into_iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .collect()
}

#[test]
fn every_item_carries_a_non_empty_file() {
    let root = Path::new("tests/fixtures/php-calls");
    let items = items_of(root);
    assert!(
        !items.is_empty(),
        "fixture yielded no :Item, so this test asserted nothing"
    );
    let missing: Vec<&str> = items
        .iter()
        .filter(|n| !matches!(n.props.get("file"), Some(PropValue::Str(f)) if !f.is_empty()))
        .map(|n| n.id.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "{} of {} :Item nodes carry no non-empty `file`: {:?}",
        missing.len(),
        items.len(),
        missing
    );
}
