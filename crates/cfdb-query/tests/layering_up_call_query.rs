//! Unit surface for RFC-050 §7 slice 50-C — the up-call layering detector
//! (`examples/queries/layering-up-call.cypher`).
//!
//! The shipped `.cypher` is smoke-parsed by CI, but smoke ignores structure:
//! a silent edit that inverted the tier comparison (`<` → `>`) or dropped the
//! load-bearing `is_test = false` filter would still parse and still pass
//! smoke, yet the detector would then flag legitimate downward calls / spurious
//! dev-dep test back-edges. This test pins the AST to the up-call shape so
//! those regressions fail the build. Mirrors `arch_ban_rfc_rules_parse.rs`'s
//! parse-then-walk helper shape.

use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::PropValue;
use cfdb_core::query::{CompareOp, Expr, Pattern, Predicate, Query};
use cfdb_core::schema::{EdgeLabel, Label};

/// `crates/cfdb-query/` is two levels below the workspace root.
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

/// True iff `e` is a property access `<var>.<name>` on the given property.
fn is_prop(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Property { prop, .. } if prop.as_str() == name)
}

/// Flatten a WHERE predicate tree into its leaf `Compare` nodes. And/Or/Not are
/// walked; NotExists/In/Regex/Ne are not shapes this detector uses.
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

/// Every `(from_label, edge_label, to_label)` triple across the MATCH clauses,
/// each as an owned `String` for easy comparison against schema constants.
fn path_edges(query: &Query) -> Vec<(Option<String>, Option<String>, Option<String>)> {
    let lbl = |l: Option<&Label>| l.map(|l| l.as_str().to_string());
    let edge = |l: Option<&EdgeLabel>| l.map(|l| l.as_str().to_string());
    query
        .match_clauses
        .iter()
        .filter_map(|p| match p {
            Pattern::Path(pp) => Some((
                lbl(pp.from.label.as_ref()),
                edge(pp.edge.label.as_ref()),
                lbl(pp.to.label.as_ref()),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn up_call_query_parses_as_single_statement() {
    // Panics inside the helper on any parse error.
    let _ = up_call_query();
}

#[test]
fn joins_calls_to_two_in_crate_hops() {
    let edges = path_edges(&up_call_query());

    // The CALLS edge carries the cross-tier call itself.
    assert!(
        edges
            .iter()
            .any(|(_, e, _)| e.as_deref() == Some(EdgeLabel::CALLS)),
        "up-call query must traverse a [:CALLS] edge; MATCH edges = {edges:?}"
    );

    // Each endpoint reaches its crate via IN_CRATE so crate_tier is comparable.
    let in_crate_to_crate = edges
        .iter()
        .filter(|(_, e, to)| {
            e.as_deref() == Some(EdgeLabel::IN_CRATE) && to.as_deref() == Some(Label::CRATE)
        })
        .count();
    assert_eq!(
        in_crate_to_crate, 2,
        "up-call query must join BOTH caller and callee to their :Crate via \
         [:IN_CRATE] (one hop each — RFC-050 killed :Item.layer); found {in_crate_to_crate}"
    );
}

#[test]
fn compares_caller_tier_strictly_below_callee_tier() {
    let query = up_call_query();
    let pred = query
        .where_clause
        .as_ref()
        .expect("up-call query has a WHERE clause");
    let mut cmps = Vec::new();
    compares(pred, &mut cmps);

    // The load-bearing predicate: caller_crate.crate_tier < callee_crate.crate_tier.
    // The direction (Lt) IS the semantics — a lower tier calling up into a
    // higher one. A flip to `>` would flag legitimate downward calls, `<=`
    // would flag same-tier intra-layer calls. Exactly one, strictly `<`.
    let tier_ops: Vec<CompareOp> = cmps
        .iter()
        .filter(|c| is_prop(c.1, "crate_tier") && is_prop(c.2, "crate_tier"))
        .map(|c| c.0)
        .collect();
    assert_eq!(
        tier_ops,
        vec![CompareOp::Lt],
        "up-call query must have exactly one crate_tier-to-crate_tier comparison, \
         strictly `<` (caller tier below callee tier); found {tier_ops:?}"
    );
}

#[test]
fn filters_out_test_scoped_calls_on_both_endpoints() {
    // The dev-dep test back-edge (e.g. cfdb-hir-extractor --dev--> cfdb-cli) is
    // the one physical source of a lower->higher cross-crate call; the tier DAG
    // excludes it via kind==Normal, so the query must exclude it via is_test.
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
