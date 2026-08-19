use cfdb_core::schema::Keyspace;
use cfdb_core::store::QueryBackend;
use cfdb_core::{ParamBinding, PropValue, Query, RowValue};

use crate::engine::ClassifyError;
use crate::taxonomy::{CanonicalCandidate, Finding};

pub(crate) fn validate_context(
    engine: &dyn QueryBackend,
    ks: &Keyspace,
    context: &str,
) -> Result<(), ClassifyError> {
    let known = query_known_contexts(engine, ks)?;
    if !known.iter().any(|c| c == context) {
        return Err(ClassifyError::UnknownContext {
            context: context.to_string(),
            known,
        });
    }
    Ok(())
}

pub(super) fn query_known_contexts(
    engine: &dyn QueryBackend,
    ks: &Keyspace,
) -> Result<Vec<String>, ClassifyError> {
    use cfdb_core::query::{NodePattern, Pattern, ProjectionValue};
    use cfdb_core::{Expr, Projection, ReturnClause};
    use std::collections::BTreeMap;
    let q = Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("c".to_string()),
            label: Some(cfdb_core::Label::new(cfdb_core::Label::CONTEXT)),
            props: BTreeMap::new(),
        })],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Property {
                    var: "c".to_string(),
                    prop: "name".to_string(),
                }),
                alias: Some("name".to_string()),
            }],
            order_by: vec![cfdb_core::OrderBy {
                expr: Expr::Var("name".to_string()),
                descending: false,
            }],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    };
    let result = engine.execute(ks, &q)?;
    let mut names: Vec<String> = result
        .rows
        .iter()
        .filter_map(|r| scalar_str(r, "name").map(String::from))
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

pub(super) fn crates_for_context(
    engine: &dyn QueryBackend,
    ks: &Keyspace,
    context: &str,
) -> Result<std::collections::BTreeSet<String>, ClassifyError> {
    use cfdb_core::query::{NodePattern, Pattern, ProjectionValue};
    use cfdb_core::{CompareOp, Expr, Predicate, Projection, ReturnClause};
    use std::collections::BTreeMap;
    let q = Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("i".to_string()),
            label: Some(cfdb_core::Label::new(cfdb_core::Label::ITEM)),
            props: BTreeMap::new(),
        })],
        where_clause: Some(Predicate::Compare {
            left: Expr::Property {
                var: "i".to_string(),
                prop: "bounded_context".to_string(),
            },
            op: CompareOp::Eq,
            right: Expr::Param("context".to_string()),
        }),
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Property {
                    var: "i".to_string(),
                    prop: "crate".to_string(),
                }),
                alias: Some("crate".to_string()),
            }],
            order_by: vec![],
            limit: None,
            distinct: true,
        },
        params: {
            let mut m = BTreeMap::new();
            m.insert(
                "context".to_string(),
                ParamBinding::Scalar(PropValue::Str(context.to_string())),
            );
            m
        },
    };
    let result = engine.execute(ks, &q)?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| scalar_str(r, "crate").map(String::from))
        .collect())
}

pub(super) fn scalar_str<'a>(row: &'a cfdb_core::Row, column: &str) -> Option<&'a str> {
    row.get(column).and_then(|v| v.as_str())
}

pub(super) fn finding_from_row(row: &cfdb_core::Row) -> Option<Finding> {
    Some(Finding {
        qname: scalar_str(row, "qname")?.to_string(),
        name: scalar_str(row, "name")?.to_string(),
        kind: scalar_str(row, "kind")?.to_string(),
        crate_name: scalar_str(row, "crate")?.to_string(),
        file: scalar_str(row, "file")?.to_string(),
        line: row.get("line").and_then(|v| v.as_i64()).unwrap_or(0) as u64,
        bounded_context: scalar_str(row, "bounded_context")?.to_string(),
    })
}

pub(super) fn row_list_str(row: &cfdb_core::Row, column: &str) -> Vec<String> {
    row.get(column)
        .and_then(|v| match v {
            RowValue::List(xs) => Some(
                xs.iter()
                    .filter_map(|p| p.as_str().map(str::to_string))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

pub(super) fn canonical_candidate_from_row(
    row: &cfdb_core::Row,
    crates_in_context: &std::collections::BTreeSet<String>,
) -> Option<CanonicalCandidate> {
    let crates = row_list_str(row, "crates");
    if !crates.iter().any(|c| crates_in_context.contains(c)) {
        return None;
    }
    Some(CanonicalCandidate {
        name: scalar_str(row, "name")?.to_string(),
        kind: scalar_str(row, "kind")?.to_string(),
        crates,
        qnames: row_list_str(row, "qnames"),
        files: row_list_str(row, "files"),
    })
}
