//! Query result shape — `{rows, warnings}`.
//!
//! Two design notes from the LLM specialist finding (RFC §14 [LLM-Q1]):
//! - Returning `{rows, warnings}` instead of plain `rows` fixes the #1
//!   LLM-consumer failure mode: silent-empty vs schema-mismatch look identical
//!   to an agent. A warning on "label `Ietm` not present in schema — did you
//!   mean `Item`?" is the difference between a self-correcting loop and a
//!   confidently-wrong answer.
//! - Rows are ordered `BTreeMap<column_name, PropValue>` so iteration is
//!   deterministic (G1).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::fact::PropValue;

/// A single result row. Column names come from the `RETURN` clause (explicit
/// `AS alias` or synthesized from the projection expression).
pub type Row = BTreeMap<String, RowValue>;

/// A value appearing in a result row. Mostly `PropValue`, but `COLLECT` /
/// `COLLECT DISTINCT` can produce lists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RowValue {
    Scalar(PropValue),
    List(Vec<PropValue>),
}

impl RowValue {
    pub fn as_scalar(&self) -> Option<&PropValue> {
        match self {
            RowValue::Scalar(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[PropValue]> {
        match self {
            RowValue::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_scalar().and_then(PropValue::as_i64)
    }

    pub fn as_str(&self) -> Option<&str> {
        self.as_scalar().and_then(PropValue::as_str)
    }
}

impl From<PropValue> for RowValue {
    fn from(p: PropValue) -> Self {
        RowValue::Scalar(p)
    }
}

/// A warning attached to a query result. Critical for distinguishing
/// "empty result" from "schema mismatch" (LLM specialist finding).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    pub message: String,
    /// Optional suggestion ("did you mean `Item`?") — lets LLM consumers
    /// self-correct without a round-trip to schema_describe().
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningKind {
    /// A node label in the query is not present in the schema.
    UnknownLabel,
    /// An edge label in the query is not present in the schema.
    UnknownEdgeLabel,
    /// A property key accessed in the query is not known for the bound label.
    UnknownProperty,
    /// The query shape is a known performance footgun (e.g. F1a Cartesian +
    /// function-equality from study 001 §4.2). The evaluator refuses to run it
    /// and the warning names the aggregation rewrite.
    PathologicalShape,
    /// The query parsed but bound no rows.
    EmptyResult,
}

/// The result of executing a query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    pub rows: Vec<Row>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
}

impl QueryResult {
    pub fn empty() -> Self {
        Self {
            rows: vec![],
            warnings: vec![],
        }
    }

    pub fn with_rows(rows: Vec<Row>) -> Self {
        Self {
            rows,
            warnings: vec![],
        }
    }

    pub fn warn(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }
}
