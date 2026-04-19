//! `list_items_matching` — the 16th cfdb verb (RATIFIED.md §A.14).
//!
//! Composes a `Query` AST value from a regex name pattern, an optional
//! [`ItemKind`] filter list, and a `group_by_context` flag. Pure and
//! deterministic — no I/O.

use std::collections::BTreeMap;

use crate::fact::PropValue;
use crate::schema::Label;

use super::ast::{
    Aggregation, Expr, NodePattern, OrderBy, Pattern, Predicate, Projection, ProjectionValue,
    Query, ReturnClause, WithClause,
};
use super::item_kind::ItemKind;

/// Compose the `list_items_matching` query (RATIFIED.md §A.14 — the 16th
/// cfdb verb). Returns a [`Query`] AST value; the caller is responsible for
/// executing it against a [`crate::store::StoreBackend`].
///
/// Semantics:
/// - `name_pattern` — openCypher-compatible regex applied to `:Item.name`.
/// - `kinds` — when `Some`, restricts to items whose `:Item.kind` is one of
///   the provided variants (mapped to the extractor's lowercase emission via
///   [`ItemKind::to_extractor_str`]). When `None`, no kind filter is applied.
/// - `group_by_context` — when `true`, rows are grouped by
///   `:Item.bounded_context` with a `COLLECT` of matching items per group;
///   when `false`, rows are flat `item` bindings.
///
/// The function is pure and deterministic: identical arguments produce
/// structurally equal Queries (by `PartialEq` / serde round-trip). No I/O.
pub fn list_items_matching(
    name_pattern: &str,
    kinds: Option<&[ItemKind]>,
    group_by_context: bool,
) -> Query {
    let match_clauses = vec![Pattern::Node(NodePattern {
        var: Some("item".to_string()),
        label: Some(Label::new(Label::ITEM)),
        props: BTreeMap::new(),
    })];

    let regex_predicate = Predicate::Regex {
        left: Expr::Property {
            var: "item".to_string(),
            prop: "name".to_string(),
        },
        pattern: Expr::Literal(PropValue::Str(name_pattern.to_string())),
    };

    let where_clause = match kinds {
        Some(ks) => {
            let kind_list = ks
                .iter()
                .map(|k| Expr::Literal(PropValue::Str(k.to_extractor_str().to_string())))
                .collect::<Vec<_>>();
            let kinds_predicate = Predicate::In {
                left: Expr::Property {
                    var: "item".to_string(),
                    prop: "kind".to_string(),
                },
                right: Expr::List(kind_list),
            };
            Some(Predicate::And(
                Box::new(regex_predicate),
                Box::new(kinds_predicate),
            ))
        }
        None => Some(regex_predicate),
    };

    // The seven columns surfaced per AC / RATIFIED §A.14:
    // `{qname, name, kind, crate, file, line, bounded_context}`. Projected
    // explicitly as `item.<prop> AS <prop>` so the row shape is a flat object
    // keyed by the property name — not a bare `item` id string.
    let flat_item_projections = || -> Vec<Projection> {
        [
            "qname",
            "name",
            "kind",
            "crate",
            "file",
            "line",
            "bounded_context",
        ]
        .iter()
        .map(|p| Projection {
            value: ProjectionValue::Expr(Expr::Property {
                var: "item".to_string(),
                prop: (*p).to_string(),
            }),
            alias: Some((*p).to_string()),
        })
        .collect()
    };

    if group_by_context {
        let with_clause = WithClause {
            projections: vec![
                Projection {
                    value: ProjectionValue::Expr(Expr::Property {
                        var: "item".to_string(),
                        prop: "bounded_context".to_string(),
                    }),
                    alias: Some("bounded_context".to_string()),
                },
                Projection {
                    value: ProjectionValue::Aggregation(Aggregation::Collect(Expr::Var(
                        "item".to_string(),
                    ))),
                    alias: Some("items".to_string()),
                },
            ],
            where_clause: None,
        };
        let return_clause = ReturnClause {
            projections: vec![
                Projection {
                    value: ProjectionValue::Expr(Expr::Var("bounded_context".to_string())),
                    alias: None,
                },
                Projection {
                    value: ProjectionValue::Expr(Expr::Var("items".to_string())),
                    alias: None,
                },
            ],
            order_by: vec![OrderBy {
                expr: Expr::Var("bounded_context".to_string()),
                descending: false,
            }],
            limit: None,
            distinct: false,
        };
        Query {
            match_clauses,
            where_clause,
            with_clause: Some(with_clause),
            return_clause,
            params: BTreeMap::new(),
        }
    } else {
        let return_clause = ReturnClause {
            projections: flat_item_projections(),
            order_by: vec![OrderBy {
                expr: Expr::Var("qname".to_string()),
                descending: false,
            }],
            limit: None,
            distinct: false,
        };
        Query {
            match_clauses,
            where_clause,
            with_clause: None,
            return_clause,
            params: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex_of(q: &Query) -> &Predicate {
        q.where_clause
            .as_ref()
            .expect("list_items_matching always emits a where_clause")
    }

    #[test]
    fn list_items_matching_filters_by_name_pattern() {
        let q = list_items_matching("^Order.*", None, false);
        // One `MATCH (item:Item)` pattern.
        assert_eq!(q.match_clauses.len(), 1);
        match &q.match_clauses[0] {
            Pattern::Node(NodePattern { var, label, .. }) => {
                assert_eq!(var.as_deref(), Some("item"));
                assert_eq!(label.as_ref().map(Label::as_str), Some("Item"));
            }
            other => panic!("expected MATCH (item:Item), got {other:?}"),
        }
        // When kinds is None, the where clause is a plain Regex (no And).
        match regex_of(&q) {
            Predicate::Regex { left, pattern } => {
                assert_eq!(
                    left,
                    &Expr::Property {
                        var: "item".into(),
                        prop: "name".into(),
                    }
                );
                assert_eq!(pattern, &Expr::Literal(PropValue::Str("^Order.*".into())));
            }
            other => panic!("expected Predicate::Regex, got {other:?}"),
        }
        // Flat row shape when group_by_context=false: 7 explicit column
        // projections per AC row shape {qname, name, kind, crate, file, line,
        // bounded_context}.
        assert!(q.with_clause.is_none());
        assert_eq!(q.return_clause.projections.len(), 7);
        let aliases: Vec<&str> = q
            .return_clause
            .projections
            .iter()
            .map(|p| p.alias.as_deref().expect("alias"))
            .collect();
        assert_eq!(
            aliases,
            vec![
                "qname",
                "name",
                "kind",
                "crate",
                "file",
                "line",
                "bounded_context",
            ]
        );
    }

    #[test]
    fn list_items_matching_filters_by_kinds() {
        let q = list_items_matching(".*", Some(&[ItemKind::Struct, ItemKind::Enum]), false);
        match regex_of(&q) {
            Predicate::And(lhs, rhs) => {
                assert!(
                    matches!(lhs.as_ref(), Predicate::Regex { .. }),
                    "expected Regex on lhs, got {lhs:?}"
                );
                match rhs.as_ref() {
                    Predicate::In { left, right } => {
                        assert_eq!(
                            left,
                            &Expr::Property {
                                var: "item".into(),
                                prop: "kind".into(),
                            }
                        );
                        assert_eq!(
                            right,
                            &Expr::List(vec![
                                Expr::Literal(PropValue::Str("struct".into())),
                                Expr::Literal(PropValue::Str("enum".into())),
                            ]),
                            "kinds filter must use extractor-emitted lowercase spelling"
                        );
                    }
                    other => panic!("expected Predicate::In on rhs, got {other:?}"),
                }
            }
            other => panic!("expected Predicate::And, got {other:?}"),
        }
    }

    #[test]
    fn list_items_matching_group_by_context_partitions_rows() {
        let q = list_items_matching(".*", None, true);
        let with = q
            .with_clause
            .as_ref()
            .expect("group_by_context=true must emit a WithClause");
        assert_eq!(with.projections.len(), 2);
        // First projection is `item.bounded_context AS bounded_context`.
        match &with.projections[0] {
            Projection {
                value: ProjectionValue::Expr(Expr::Property { var, prop }),
                alias,
            } => {
                assert_eq!(var, "item");
                assert_eq!(prop, "bounded_context");
                assert_eq!(alias.as_deref(), Some("bounded_context"));
            }
            other => panic!("expected bounded_context projection, got {other:?}"),
        }
        // Second projection is `COLLECT(item) AS items`.
        match &with.projections[1] {
            Projection {
                value: ProjectionValue::Aggregation(Aggregation::Collect(Expr::Var(v))),
                alias,
            } => {
                assert_eq!(v, "item");
                assert_eq!(alias.as_deref(), Some("items"));
            }
            other => panic!("expected COLLECT(item) projection, got {other:?}"),
        }
        assert_eq!(q.return_clause.projections.len(), 2);
    }

    #[test]
    fn list_items_matching_deterministic_across_runs() {
        // Same inputs must produce byte-identical serialized Queries so that
        // consumers can diff / cache on query identity.
        let a = list_items_matching(
            "^Order.*",
            Some(&[ItemKind::Struct, ItemKind::Enum, ItemKind::Trait]),
            true,
        );
        let b = list_items_matching(
            "^Order.*",
            Some(&[ItemKind::Struct, ItemKind::Enum, ItemKind::Trait]),
            true,
        );
        assert_eq!(a, b, "PartialEq determinism");
        let sa = serde_json::to_string(&a).expect("serialize a");
        let sb = serde_json::to_string(&b).expect("serialize b");
        assert_eq!(sa, sb, "serde-byte determinism");
    }

    #[test]
    fn list_items_matching_implblock_maps_to_unemitted_sentinel() {
        // Council §A.14 accepts ImplBlock in the CLI surface even though the
        // v0.1 syn extractor does not emit :Item nodes for impl blocks. The
        // composer maps ImplBlock to an unmatched sentinel so the Predicate::In
        // filter matches zero rows — the CLI handler surfaces a warning
        // explaining the Phase A limitation (tested in cfdb-cli integration
        // tests, not here).
        let q = list_items_matching(".*", Some(&[ItemKind::ImplBlock]), false);
        let pred = regex_of(&q);
        let (_, rhs) = match pred {
            Predicate::And(l, r) => (l, r),
            other => panic!("expected Predicate::And, got {other:?}"),
        };
        match rhs.as_ref() {
            Predicate::In { right, .. } => {
                assert_eq!(
                    right,
                    &Expr::List(vec![Expr::Literal(PropValue::Str(
                        "<unemitted:impl_block>".into()
                    ))])
                );
            }
            other => panic!("expected Predicate::In, got {other:?}"),
        }
    }
}
