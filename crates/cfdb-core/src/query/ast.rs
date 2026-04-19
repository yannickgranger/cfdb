//! Query AST core — the shapes produced by the parser/builder and consumed by
//! the store evaluator.
//!
//! This module contains only the AST node types (`Query`, `Pattern`,
//! `Predicate`, `Expr`, `ReturnClause`, ...). The debt-inventory types
//! (`DebtClass`, `ScopeInventory`, ...) and the `list_items_matching` composer
//! live in sibling submodules.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::fact::PropValue;
use crate::schema::{EdgeLabel, Label};

/// A parsed query. Top-level shape:
///
/// ```text
/// MATCH <patterns>
/// [WHERE <predicate>]
/// [WITH <projections> [WHERE <predicate>]]
/// RETURN <items>
/// [ORDER BY <items>] [LIMIT <n>]
/// ```
///
/// `UNWIND $list AS var` is represented as a pattern variant, not a top-level
/// clause, to keep the AST shape uniform.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub match_clauses: Vec<Pattern>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<Predicate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with_clause: Option<WithClause>,
    pub return_clause: ReturnClause,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Param>,
}

impl Query {
    pub fn new(match_clauses: Vec<Pattern>, return_clause: ReturnClause) -> Self {
        Self {
            match_clauses,
            where_clause: None,
            with_clause: None,
            return_clause,
            params: BTreeMap::new(),
        }
    }
}

/// Value bound to a `$name` parameter in a query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Param {
    Scalar(PropValue),
    List(Vec<PropValue>),
}

/// A single MATCH pattern or UNWIND binding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    /// `MATCH (var:Label {prop: value, ...})` — a single node binding. The
    /// label may be absent if the pattern is unlabelled (e.g. anonymous
    /// endpoints in a path).
    Node(NodePattern),

    /// `MATCH (a)-[:LABEL*1..N]->(b)` — a traversal from an existing node
    /// binding to another, optionally with a variable-length quantifier.
    Path(PathPattern),

    /// `OPTIONAL MATCH <inner>` — left-join semantics. Inner bindings become
    /// NULL-filled on no match (F4).
    Optional(Box<Pattern>),

    /// `UNWIND $list AS var` — binds each element of `$list` to `var` and
    /// cross-joins with the rest of the match.
    Unwind { list_param: String, var: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodePattern {
    /// Binding name: `(foo:Label)` → `Some("foo")`; anonymous `()` → None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,
    /// Inline property equalities: `(foo {kind: 'fn'})` → `{"kind": "fn"}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, PropValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathPattern {
    pub from: NodePattern,
    pub edge: EdgePattern,
    pub to: NodePattern,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgePattern {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<EdgeLabel>,
    pub direction: Direction,
    /// `[:LABEL*1..5]` → `Some((1, 5))`; `[:LABEL]` → None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var_length: Option<(u32, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Out,
    In,
    Undirected,
}

/// WHERE clause predicate tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Predicate {
    /// `a.qname = 'foo'` or `a.qname = b.qname` or `a.line < 100`.
    Compare {
        left: Expr,
        op: CompareOp,
        right: Expr,
    },

    /// `a.qname IN $list` or `a.qname IN [x, y, z]`.
    In {
        left: Expr,
        right: Expr,
    },

    /// `a.qname =~ '^foo_.*'` — regex match on string.
    Regex {
        left: Expr,
        pattern: Expr,
    },

    /// `a <> b`.
    Ne {
        left: Expr,
        right: Expr,
    },

    /// `NOT EXISTS { MATCH (a)-[:CALLS]->(b) }` — the sub-match subquery form.
    NotExists {
        inner: Box<Query>,
    },

    And(Box<Predicate>, Box<Predicate>),
    Or(Box<Predicate>, Box<Predicate>),
    Not(Box<Predicate>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Expression appearing in predicates and RETURN/WITH clauses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// `var.prop` — property access on a bound variable.
    Property { var: String, prop: String },
    /// `var` — reference to a full bound node/edge.
    Var(String),
    /// A literal value.
    Literal(PropValue),
    /// `$name` — parameter reference.
    Param(String),
    /// A list literal.
    List(Vec<Expr>),
    /// A function call: `regexp_extract(a.qname, '[^:]+$')`, `starts_with`,
    /// `ends_with`, `size`, etc.
    Call { name: String, args: Vec<Expr> },
}

/// `WITH projection1 AS alias1, COUNT(x) AS n, ... [WHERE predicate]`.
///
/// Non-aggregated items in the projection list form the implicit GROUP BY key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WithClause {
    pub projections: Vec<Projection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<Predicate>,
}

/// `RETURN projection1 AS alias1, ... [ORDER BY ...] [LIMIT n]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReturnClause {
    pub projections: Vec<Projection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_by: Vec<OrderBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    pub distinct: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub value: ProjectionValue,
    /// `AS alias`. When absent, callers get a synthesized column name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProjectionValue {
    Expr(Expr),
    Aggregation(Aggregation),
}

/// Supported aggregations for v0.1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Aggregation {
    /// `COUNT(*)` — count all bindings.
    CountStar,
    /// `COUNT(expr)` — count non-null values of expr.
    Count(Expr),
    /// `COUNT(DISTINCT expr)`.
    CountDistinct(Expr),
    /// `COLLECT(expr)` — gather into a list.
    Collect(Expr),
    /// `COLLECT(DISTINCT expr)`.
    CollectDistinct(Expr),
    /// `SIZE(expr)` — length of a list or string.
    Size(Expr),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderBy {
    pub expr: Expr,
    pub descending: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_roundtrip_via_serde() {
        // F1b (aggregation) — the canonical Pattern A query shape.
        let q = Query::new(
            vec![Pattern::Node(NodePattern {
                var: Some("a".into()),
                label: Some(Label::new(Label::ITEM)),
                props: BTreeMap::new(),
            })],
            ReturnClause {
                projections: vec![Projection {
                    value: ProjectionValue::Aggregation(Aggregation::CountStar),
                    alias: Some("n".into()),
                }],
                order_by: vec![],
                limit: None,
                distinct: false,
            },
        );
        let json =
            serde_json::to_string(&q).expect("Query has derived Serialize over owned fields");
        let back: Query = serde_json::from_str(&json).expect("round-trip of just-serialized Query");
        assert_eq!(q, back);
    }
}
