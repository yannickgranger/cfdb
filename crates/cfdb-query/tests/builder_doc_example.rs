use cfdb_core::Label;
use cfdb_query::QueryBuilder;

#[test]
fn builder_module_example_builds_one_match_clause() {
    let q = QueryBuilder::new()
        .match_node("a", Label::new(Label::ITEM))
        .return_count_star("n")
        .build();
    assert_eq!(q.match_clauses.len(), 1);
}
