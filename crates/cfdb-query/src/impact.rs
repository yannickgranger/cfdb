//! `impact_query` — the canonical `cfdb impact` reverse-reachability query
//! composer (RFC-047 §3.2, slice 47-A / #489).
//!
//! Given a list of changed-item seed qnames, composes the parameterised
//! `Query` AST that collects every transitive *caller* of those seeds — the
//! blast radius of a change. The traversal is the open variable-length form
//! `(seed)<-[:CALLS*1..]-(affected)` that slice 47-0 (#488) made expressible
//! and un-truncated (RFC-047a B1 + B2): `*1..` is `Some((1, u32::MAX))` and the
//! evaluator walks it unbounded via its visited-set.
//!
//! Like the sibling [`crate::list_items::list_items_matching`] composer, it
//! builds the `Query` AST **directly** (no parse, no I/O), so it is pure,
//! infallible, and deterministic — identical seed lists produce structurally
//! equal `Query` values (by `PartialEq` / serde round-trip). The
//! human-readable [`IMPACT_QUERY`] string documents the same shape and is
//! pinned to the built AST by `impact_query_matches_canonical_cypher`.

use std::collections::BTreeMap;

use cfdb_core::fact::PropValue;
use cfdb_core::query::{
    Direction, EdgePattern, Expr, NodePattern, ParamBinding, PathPattern, Pattern, Predicate,
    Projection, ProjectionValue, Query, ReturnClause,
};
use cfdb_core::schema::{EdgeLabel, Label};

/// The canonical `cfdb impact` reverse-reachability query (RFC-047 §3.2), in
/// the v0.1 Cypher subset. [`impact_query`] builds this exact shape as a
/// `Query` AST — the string is the human-readable source of truth, pinned to
/// the built AST by the `impact_query_matches_canonical_cypher` test.
///
/// `$seeds` binds a `ParamBinding::List` of seed qnames; the open form
/// `<-[:CALLS*1..]-` collects every transitive caller of the bound seeds,
/// unbounded (RFC-047a B2). `DISTINCT` dedups items reachable via more than one
/// call path. Reduced to the single `affected.qname` column this slice asserts
/// on — the production-reachability ranking is slice 47-C.
pub const IMPACT_QUERY: &str = "MATCH (seed:Item)<-[:CALLS*1..]-(affected:Item) \
     WHERE seed.qname IN $seeds \
     RETURN DISTINCT affected.qname AS qname";

/// Compose the canonical `cfdb impact` query (RFC-047 §3.2) for a list of
/// changed-item seed qnames.
///
/// Returns a [`Query`] AST value with `$seeds` already bound to a
/// `ParamBinding::List` of the given qnames, in the order provided. The caller
/// executes it against a [`cfdb_core::store::StoreBackend`]; the result rows'
/// `qname` column is the set of transitive callers (the blast radius).
///
/// `max_depth` bounds the traversal: `None` is the open form `<-[:CALLS*1..]-`
/// (unbounded, the canonical default — [`IMPACT_QUERY`]); `Some(n)` is
/// `<-[:CALLS*1..n]-` (callers within `n` hops). This is the `*1..N` form the
/// `cfdb impact --max-depth` flag maps to (RFC-047a §6 / slice 47-B).
pub fn impact_query<S: AsRef<str>>(seeds: &[S], max_depth: Option<u32>) -> Query {
    // `(var:Item)` endpoint — the seed and affected nodes share this shape.
    let item_endpoint = |var: &str| NodePattern {
        var: Some(var.to_string()),
        label: Some(Label::new(Label::ITEM)),
        props: BTreeMap::new(),
    };

    // `(seed:Item)<-[:CALLS*1..N]-(affected:Item)` — reverse (`Direction::In`)
    // var-length traversal: `affected` is any transitive caller of `seed`.
    // `None` → `u32::MAX` (the open, unbounded form).
    let match_clauses = vec![Pattern::Path(PathPattern {
        from: item_endpoint("seed"),
        edge: EdgePattern {
            var: None,
            label: Some(EdgeLabel::new(EdgeLabel::CALLS)),
            direction: Direction::In,
            var_length: Some((1, max_depth.unwrap_or(u32::MAX))),
        },
        to: item_endpoint("affected"),
    })];

    // `WHERE seed.qname IN $seeds`.
    let where_clause = Some(Predicate::In {
        left: Expr::Property {
            var: "seed".to_string(),
            prop: "qname".to_string(),
        },
        right: Expr::Param("seeds".to_string()),
    });

    // `RETURN DISTINCT affected.qname AS qname`.
    let return_clause = ReturnClause {
        projections: vec![Projection {
            value: ProjectionValue::Expr(Expr::Property {
                var: "affected".to_string(),
                prop: "qname".to_string(),
            }),
            alias: Some("qname".to_string()),
        }],
        order_by: vec![],
        limit: None,
        distinct: true,
    };

    // `$seeds` bound to the given qnames, in order.
    let bound = seeds
        .iter()
        .map(|s| PropValue::Str(s.as_ref().to_string()))
        .collect();
    let mut params = BTreeMap::new();
    params.insert("seeds".to_string(), ParamBinding::List(bound));

    Query {
        match_clauses,
        where_clause,
        with_clause: None,
        return_clause,
        params,
    }
}

/// Compose the projection that feeds `cfdb impact --since` SEED RESOLUTION
/// (RFC-047 §3.3): `(qname, file)` for every `:Item`.
///
/// The caller matches each row's `file` against the `git diff` changed-file set
/// (repo-relative) and seeds [`impact_query`] with the matching qnames. The
/// match is done caller-side (not as a `WHERE i.file IN $files`) on purpose:
/// `:Item.file` is **absolute** on HIR-extracted keyspaces (the ones that carry
/// the resolved `CALLS` impact needs) and **repo-relative** on syn-extracted
/// ones, so an exact `IN` would silently match nothing on exactly the keyspaces
/// where `impact` is meaningful. A suffix-tolerant caller-side match handles
/// both forms.
pub fn items_with_files_query() -> Query {
    let property = |prop: &str| {
        ProjectionValue::Expr(Expr::Property {
            var: "i".to_string(),
            prop: prop.to_string(),
        })
    };
    Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("i".to_string()),
            label: Some(Label::new(Label::ITEM)),
            props: BTreeMap::new(),
        })],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![
                Projection {
                    value: property("qname"),
                    alias: Some("qname".to_string()),
                },
                Projection {
                    value: property("file"),
                    alias: Some("file".to_string()),
                },
            ],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::parse;

    use super::*;

    #[test]
    fn impact_query_binds_seed_list_as_param_in_order() {
        let q = impact_query(&["core::leaf_a", "core::leaf_b"], None);
        let Some(ParamBinding::List(items)) = q.params.get("seeds") else {
            unreachable!("impact_query always binds $seeds as a ParamBinding::List");
        };
        assert_eq!(
            items,
            &vec![
                PropValue::Str("core::leaf_a".into()),
                PropValue::Str("core::leaf_b".into()),
            ],
            "$seeds must be the given qnames, in order, as a ParamBinding::List"
        );
    }

    #[test]
    fn impact_query_is_reverse_unbounded_calls_traversal() {
        let q = impact_query(&["x"], None);
        assert_eq!(q.match_clauses.len(), 1, "one reverse var-length path");
        let Pattern::Path(PathPattern { from, edge, to }) = &q.match_clauses[0] else {
            unreachable!("the canonical impact match is a single Path pattern");
        };
        assert_eq!(from.var.as_deref(), Some("seed"));
        assert_eq!(from.label.as_ref().map(Label::as_str), Some("Item"));
        assert_eq!(to.var.as_deref(), Some("affected"));
        assert_eq!(to.label.as_ref().map(Label::as_str), Some("Item"));
        assert_eq!(edge.label.as_ref().map(EdgeLabel::as_str), Some("CALLS"));
        assert_eq!(
            edge.direction,
            Direction::In,
            "reverse `<-` traversal — the affected items are CALLERS of the seed"
        );
        assert_eq!(
            edge.var_length,
            Some((1, u32::MAX)),
            "open form `*1..` is unbounded (RFC-047a B2) — full transitive closure"
        );
    }

    #[test]
    fn impact_query_max_depth_bounds_the_traversal() {
        // `--max-depth N` maps to the bounded form `*1..N` (RFC-047a §6 / 47-B).
        let q = impact_query(&["x"], Some(3));
        let Pattern::Path(PathPattern { edge, .. }) = &q.match_clauses[0] else {
            unreachable!("the canonical impact match is a single Path pattern");
        };
        assert_eq!(
            edge.var_length,
            Some((1, 3)),
            "Some(n) bounds the var-length traversal at n hops"
        );
    }

    #[test]
    fn impact_query_filters_seed_by_qname_in_seeds() {
        let q = impact_query(&["x"], None);
        let Some(Predicate::In { left, right }) = q.where_clause.as_ref() else {
            unreachable!("impact filters the seed endpoint by `seed.qname IN $seeds`");
        };
        assert_eq!(
            left,
            &Expr::Property {
                var: "seed".into(),
                prop: "qname".into(),
            }
        );
        assert_eq!(
            right,
            &Expr::Param("seeds".into()),
            "the right operand is the bound `$seeds` list param"
        );
    }

    #[test]
    fn impact_query_returns_distinct_affected_qname() {
        let q = impact_query(&["x"], None);
        assert!(
            q.return_clause.distinct,
            "DISTINCT dedups items reachable via more than one call path"
        );
        assert_eq!(q.return_clause.projections.len(), 1, "single qname column");
        assert_eq!(
            q.return_clause.projections[0].alias.as_deref(),
            Some("qname")
        );
    }

    #[test]
    fn impact_query_deterministic_across_runs() {
        let a = impact_query(&["a", "b"], None);
        let b = impact_query(&["a", "b"], None);
        assert_eq!(a, b, "PartialEq determinism");
        let sa = serde_json::to_string(&a).expect("serialize a");
        let sb = serde_json::to_string(&b).expect("serialize b");
        assert_eq!(sa, sb, "serde-byte determinism");
    }

    #[test]
    fn impact_query_matches_canonical_cypher() {
        // The directly-built AST must equal `parse(IMPACT_QUERY)` (params
        // aside): this pins the human-readable canonical string as the source
        // of truth and catches any drift between it and the built AST.
        let mut built = impact_query::<&str>(&[], None);
        built.params.clear();
        let parsed = parse(IMPACT_QUERY).expect("IMPACT_QUERY is a valid Cypher-subset query");
        assert_eq!(
            built, parsed,
            "built impact AST must equal parse(IMPACT_QUERY)"
        );
    }

    #[test]
    fn items_with_files_query_projects_qname_and_file_for_all_items() {
        let q = items_with_files_query();
        // MATCH (i:Item) — no WHERE (caller does the suffix-tolerant match).
        assert_eq!(q.match_clauses.len(), 1);
        let Pattern::Node(NodePattern { var, label, .. }) = &q.match_clauses[0] else {
            unreachable!("seed-resolution projection matches a single (i:Item) node");
        };
        assert_eq!(var.as_deref(), Some("i"));
        assert_eq!(label.as_ref().map(Label::as_str), Some("Item"));
        assert!(q.where_clause.is_none(), "no WHERE — match is caller-side");
        assert!(q.params.is_empty());
        // RETURN i.qname AS qname, i.file AS file
        let aliases: Vec<&str> = q
            .return_clause
            .projections
            .iter()
            .map(|p| p.alias.as_deref().expect("alias"))
            .collect();
        assert_eq!(aliases, vec!["qname", "file"]);
        for (proj, prop) in q.return_clause.projections.iter().zip(["qname", "file"]) {
            assert_eq!(
                proj.value,
                ProjectionValue::Expr(Expr::Property {
                    var: "i".into(),
                    prop: prop.into(),
                })
            );
        }
    }
}
