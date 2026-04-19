//! Acceptance: the builder and parser produce the same `Query` AST for a
//! matched set of inputs. This validates the "two surfaces, one AST"
//! invariant from study 001 §8.3 — backend-agnosticism rests on it.

use cfdb_core::{Expr, Label, OrderBy, ProjectionValue, PropValue, ReturnClause};
use cfdb_query::{parse, QueryBuilder};

#[test]
fn bare_match_count_star_equivalent() {
    let built = QueryBuilder::new()
        .match_node("a", Label::new(Label::ITEM))
        .return_count_star("n")
        .build();
    let parsed = parse("MATCH (a:Item) RETURN count(*) AS n").expect("parses");
    assert_eq!(built, parsed);
}

#[test]
fn match_with_where_compare_equivalent() {
    let built = QueryBuilder::new()
        .match_node("a", Label::new(Label::ITEM))
        .where_eq(
            Expr::Property {
                var: "a".into(),
                prop: "qname".into(),
            },
            Expr::Literal(PropValue::Str("foo".into())),
        )
        .return_count_star("n")
        .build();
    let parsed =
        parse("MATCH (a:Item) WHERE a.qname = 'foo' RETURN count(*) AS n").expect("parses");
    assert_eq!(built, parsed);
}

#[test]
fn match_with_gt_literal_equivalent() {
    let built = QueryBuilder::new()
        .match_node("a", Label::new(Label::ITEM))
        .where_gt(
            Expr::Property {
                var: "a".into(),
                prop: "line".into(),
            },
            Expr::Literal(PropValue::Int(100)),
        )
        .return_items(vec![cfdb_core::Projection {
            value: ProjectionValue::Expr(Expr::Var("a".into())),
            alias: None,
        }])
        .build();
    let parsed = parse("MATCH (a:Item) WHERE a.line > 100 RETURN a").expect("parses");
    assert_eq!(built, parsed);
}

#[test]
fn return_with_order_by_limit_equivalent() {
    let built = QueryBuilder::new()
        .match_node("a", Label::new(Label::ITEM))
        .return_items(vec![cfdb_core::Projection {
            value: ProjectionValue::Expr(Expr::Property {
                var: "a".into(),
                prop: "qname".into(),
            }),
            alias: None,
        }])
        .order_by(
            Expr::Property {
                var: "a".into(),
                prop: "line".into(),
            },
            true,
        )
        .limit(10)
        .build();

    // Sanity-check expected AST shape.
    let expected_return = ReturnClause {
        projections: vec![cfdb_core::Projection {
            value: ProjectionValue::Expr(Expr::Property {
                var: "a".into(),
                prop: "qname".into(),
            }),
            alias: None,
        }],
        order_by: vec![OrderBy {
            expr: Expr::Property {
                var: "a".into(),
                prop: "line".into(),
            },
            descending: true,
        }],
        limit: Some(10),
        distinct: false,
    };
    assert_eq!(built.return_clause, expected_return);

    let parsed =
        parse("MATCH (a:Item) RETURN a.qname ORDER BY a.line DESC LIMIT 10").expect("parses");
    assert_eq!(built, parsed);
}
