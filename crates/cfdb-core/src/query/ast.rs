use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::fact::PropValue;
use crate::schema::{EdgeLabel, Label};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub match_clauses: Vec<Pattern>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<Predicate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with_clause: Option<WithClause>,
    pub return_clause: ReturnClause,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamBinding>,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParamBinding {
    Scalar(PropValue),
    List(Vec<PropValue>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    Node(NodePattern),

    Path(PathPattern),

    Optional(Box<Pattern>),

    Unwind { list_param: String, var: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodePattern {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var_length: Option<(u32, u32)>,
}

pub use crate::schema::Direction;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Predicate {
    Compare {
        left: Expr,
        op: CompareOp,
        right: Expr,
    },

    In {
        left: Expr,
        right: Expr,
    },

    Regex {
        left: Expr,
        pattern: Expr,
    },

    Ne {
        left: Expr,
        right: Expr,
    },

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Property { var: String, prop: String },
    Var(String),
    Literal(PropValue),
    Param(String),
    List(Vec<Expr>),
    Call { name: String, args: Vec<Expr> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WithClause {
    pub projections: Vec<Projection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<Predicate>,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProjectionValue {
    Expr(Expr),
    Aggregation(Aggregation),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Aggregation {
    CountStar,
    Count(Expr),
    CountDistinct(Expr),
    Collect(Expr),
    CollectDistinct(Expr),
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
