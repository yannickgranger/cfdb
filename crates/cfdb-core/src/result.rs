use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::fact::PropValue;

pub type Row = BTreeMap<String, RowValue>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WarningKind {
    UnknownLabel,
    UnknownEdgeLabel,
    UnknownProperty,
    PathologicalShape,
    EmptyResult,
    IdentityContention,
}

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
