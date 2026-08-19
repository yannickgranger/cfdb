use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::PropValue;
use cfdb_core::query::{CompareOp, Expr, NodePattern, PathPattern, Pattern, Predicate, Query};
use cfdb_core::schema::{EdgeLabel, Label};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn up_call_query() -> Query {
    let path = workspace_root()
        .join("examples")
        .join("queries")
        .join("layering-up-call.cypher");
    let source =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    cfdb_query::parse(&source).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn paths(query: &Query) -> Vec<&PathPattern> {
    query
        .match_clauses
        .iter()
        .filter_map(|p| match p {
            Pattern::Path(pp) => Some(pp),
            _ => None,
        })
        .collect()
}

fn edge_label(p: &PathPattern) -> Option<&str> {
    p.edge.label.as_ref().map(|l| l.as_str())
}

fn node_label(n: &NodePattern) -> Option<&str> {
    n.label.as_ref().map(|l| l.as_str())
}

fn prop_var(e: &Expr) -> Option<&str> {
    match e {
        Expr::Property { var, .. } => Some(var.as_str()),
        _ => None,
    }
}

fn is_prop(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Property { prop, .. } if prop.as_str() == name)
}

fn compares<'q>(pred: &'q Predicate, out: &mut Vec<(CompareOp, &'q Expr, &'q Expr)>) {
    match pred {
        Predicate::Compare { left, op, right } => out.push((*op, left, right)),
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            compares(a, out);
            compares(b, out);
        }
        Predicate::Not(inner) => compares(inner, out),
        Predicate::NotExists { .. }
        | Predicate::In { .. }
        | Predicate::Regex { .. }
        | Predicate::Ne { .. } => {}
    }
}

#[test]
fn up_call_query_parses_as_single_statement() {
    let _ = up_call_query();
}

#[test]
fn binds_call_direction_through_in_crate_to_the_tier_comparison() {
    let query = up_call_query();
    let all_paths = paths(&query);

    let calls = all_paths
        .iter()
        .copied()
        .find(|p| edge_label(p) == Some(EdgeLabel::CALLS))
        .expect("query must traverse a [:CALLS] edge");
    assert_eq!(
        node_label(&calls.from),
        Some(Label::ITEM),
        "CALLS source must be an :Item"
    );
    assert_eq!(
        node_label(&calls.to),
        Some(Label::ITEM),
        "CALLS dest must be an :Item"
    );
    let caller_item = calls.from.var.as_deref().expect("CALLS source is bound");
    let callee_item = calls.to.var.as_deref().expect("CALLS dest is bound");
    assert_eq!(caller_item, "caller", "CALLS source var");
    assert_eq!(callee_item, "callee", "CALLS dest var");

    let mut item_to_crate: BTreeMap<&str, &str> = BTreeMap::new();
    let mut in_crate_count = 0;
    for p in all_paths.iter().copied() {
        if edge_label(p) != Some(EdgeLabel::IN_CRATE) {
            continue;
        }
        in_crate_count += 1;
        assert_eq!(
            node_label(&p.to),
            Some(Label::CRATE),
            "IN_CRATE target must be a :Crate"
        );
        let item_var = p.from.var.as_deref().expect("IN_CRATE source is bound");
        let crate_var = p.to.var.as_deref().expect("IN_CRATE crate is bound");
        item_to_crate.insert(item_var, crate_var);
    }
    assert_eq!(
        in_crate_count, 2,
        "up-call query must join BOTH caller and callee to their :Crate via \
         [:IN_CRATE] (one hop each — RFC-050 killed :Item.layer)"
    );
    let caller_crate = item_to_crate
        .get(caller_item)
        .copied()
        .unwrap_or_else(|| panic!("caller `{caller_item}` has no IN_CRATE hop to a :Crate"));
    let callee_crate = item_to_crate
        .get(callee_item)
        .copied()
        .unwrap_or_else(|| panic!("callee `{callee_item}` has no IN_CRATE hop to a :Crate"));
    assert_eq!(caller_crate, "cc", "caller's crate var");
    assert_eq!(callee_crate, "dc", "callee's crate var");

    let pred = query
        .where_clause
        .as_ref()
        .expect("up-call query has a WHERE clause");
    let mut cmps = Vec::new();
    compares(pred, &mut cmps);
    let tier_cmp = cmps
        .iter()
        .find(|c| is_prop(c.1, "crate_tier") && is_prop(c.2, "crate_tier"))
        .expect("query must compare crate_tier to crate_tier");
    assert_eq!(
        tier_cmp.0,
        CompareOp::Lt,
        "tier comparison must be strictly `<` (caller tier below callee tier); \
         `>`/`<=` invert or loosen the up-call semantics"
    );
    assert_eq!(
        prop_var(tier_cmp.1),
        Some(caller_crate),
        "LEFT of `<` must be the CALLER's crate tier ({caller_crate}.crate_tier) — the \
         lower layer; a var-swap here silently inverts the detector"
    );
    assert_eq!(
        prop_var(tier_cmp.2),
        Some(callee_crate),
        "RIGHT of `<` must be the CALLEE's crate tier ({callee_crate}.crate_tier) — the \
         higher layer being called up into"
    );
}

#[test]
fn filters_out_test_scoped_calls_on_both_endpoints() {
    let query = up_call_query();
    let pred = query
        .where_clause
        .as_ref()
        .expect("up-call query has a WHERE clause");
    let mut cmps = Vec::new();
    compares(pred, &mut cmps);

    let mut is_test_false_vars: Vec<&str> = cmps
        .iter()
        .filter_map(|c| match (c.0, c.1, c.2) {
            (
                CompareOp::Eq,
                Expr::Property { var, prop },
                Expr::Literal(PropValue::Bool(false)),
            ) if prop.as_str() == "is_test" => Some(var.as_str()),
            _ => None,
        })
        .collect();
    is_test_false_vars.sort_unstable();

    for endpoint in ["caller", "callee"] {
        assert!(
            is_test_false_vars.contains(&endpoint),
            "up-call query must filter `{endpoint}.is_test = false` (dev-dep test \
             back-edges are excluded from the tier DAG and must be excluded here \
             too); is_test=false filters found on = {is_test_false_vars:?}"
        );
    }
}
