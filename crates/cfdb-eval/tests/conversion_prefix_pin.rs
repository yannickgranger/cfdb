//! The store's `ComputedKey::ConversionPrefix` index key and the
//! `regexp_extract` join in `examples/queries/classifier-random-scattering.cypher`
//! must use the same pattern, byte for byte: the cross-MATCH fast path
//! recognises the computed join only when the Cypher call's pattern
//! argument equals the vetted constant. The two now live in different
//! crates, so the equality is pinned here against the PARSED rule file —
//! never against a third hand-written copy of the literal.

use std::collections::BTreeSet;

use cfdb_core::fact::PropValue;
use cfdb_core::query::{Expr, Predicate, Query};
use cfdb_petgraph::index::spec::CONVERSION_PREFIX_PATTERN;

const RULE: &str = include_str!("../../../examples/queries/classifier-random-scattering.cypher");

/// Every distinct string literal handed to `regexp_extract(_, <literal>)`
/// anywhere in the query's `WHERE` (and nested `NOT EXISTS`) predicates.
fn regexp_extract_patterns(query: &Query) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(pred) = &query.where_clause {
        collect_predicate(pred, &mut out);
    }
    out
}

fn collect_predicate(pred: &Predicate, out: &mut BTreeSet<String>) {
    match pred {
        Predicate::Compare { left, right, .. }
        | Predicate::In { left, right }
        | Predicate::Ne { left, right } => {
            collect_expr(left, out);
            collect_expr(right, out);
        }
        Predicate::Regex { left, pattern } => {
            collect_expr(left, out);
            collect_expr(pattern, out);
        }
        Predicate::NotExists { inner } => out.extend(regexp_extract_patterns(inner)),
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            collect_predicate(a, out);
            collect_predicate(b, out);
        }
        Predicate::Not(inner) => collect_predicate(inner, out),
    }
}

fn collect_expr(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Call { name, args } => {
            if name == "regexp_extract" {
                if let Some(Expr::Literal(PropValue::Str(pattern))) = args.get(1) {
                    out.insert(pattern.clone());
                }
            }
            for arg in args {
                collect_expr(arg, out);
            }
        }
        Expr::List(items) => {
            for item in items {
                collect_expr(item, out);
            }
        }
        Expr::Property { .. } | Expr::Var(_) | Expr::Literal(_) | Expr::Param(_) => {}
    }
}

#[test]
fn conversion_prefix_key_equals_the_rule_files_regexp_extract_literal() {
    let query = cfdb_query::parse(RULE).expect("classifier-random-scattering.cypher parses");
    let patterns = regexp_extract_patterns(&query);
    assert!(
        !patterns.is_empty(),
        "the rule no longer calls regexp_extract with a literal pattern — the ConversionPrefix \
         computed key has lost its consumer; revisit both together"
    );
    assert!(
        patterns.contains(CONVERSION_PREFIX_PATTERN),
        "cfdb_petgraph::index::spec::CONVERSION_PREFIX_PATTERN ({CONVERSION_PREFIX_PATTERN:?}) is not \
         among the regexp_extract literals the rule uses ({patterns:?}); the index bucket key and the \
         query-time join value must be built from the same pattern"
    );
}
