use std::collections::BTreeMap;
use std::str::FromStr;

use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphBackend;
use cfdb_core::result::{Row, RowValue, Warning};
use cfdb_core::schema::Keyspace;
use cfdb_core::store::QueryBackend;
use cfdb_eval::QueryEngine;
use cfdb_query::parse;

use crate::engine::ClassifyError;

pub(crate) mod t1;
pub(crate) mod t3;
#[cfg(test)]
mod tests;

pub(super) const T1_CONTEXT_INVENTORY_CYPHER: &str =
    include_str!("../../../examples/queries/t1-concept-unwired.cypher");

pub(super) const T1_CRATE_NAMES_CYPHER: &str =
    "MATCH (k:Crate) RETURN k.name AS name ORDER BY name ASC";
pub(super) const T1_ITEM_BOUNDED_CONTEXTS_CYPHER: &str =
    "MATCH (i:Item) RETURN i.bounded_context AS bc ORDER BY bc ASC";
pub(super) const T1_RFC_DOCS_CYPHER: &str =
    "MATCH (r:RfcDoc) RETURN r.path AS path, r.title AS title ORDER BY path ASC";

pub(super) const T3_CONCEPT_MULTI_CRATE_CYPHER: &str =
    include_str!("../../../examples/queries/t3-concept-multi-crate.cypher");

pub(super) const T3_CANONICAL_CRATES_CYPHER: &str =
    "MATCH (c:Context) RETURN c.canonical_crate AS canonical_crate ORDER BY canonical_crate ASC";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerId {
    T1,
    T3,
}

impl TriggerId {
    pub fn variants() -> &'static [TriggerId] {
        &[TriggerId::T1, TriggerId::T3]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TriggerId::T1 => "T1",
            TriggerId::T3 => "T3",
        }
    }
}

impl std::fmt::Display for TriggerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TriggerId {
    type Err = UnknownTriggerId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::variants()
            .iter()
            .find(|v| v.as_str() == s)
            .copied()
            .ok_or_else(|| UnknownTriggerId(s.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownTriggerId(pub String);

impl std::fmt::Display for UnknownTriggerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let valid = TriggerId::variants()
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        write!(
            f,
            "unknown TriggerId `{}` — valid values: {}",
            self.0, valid
        )
    }
}

impl std::error::Error for UnknownTriggerId {}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckReport {
    pub trigger: TriggerId,
    pub rows: Vec<Row>,
    pub warnings: Vec<Warning>,
}

impl CheckReport {
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

pub(super) fn execute<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
    cypher: &str,
    rule: &'static str,
) -> Result<Vec<Row>, ClassifyError> {
    let parsed = parse(cypher).map_err(|source| ClassifyError::Parse { rule, source })?;
    Ok(engine.execute(ks, &parsed)?.rows)
}

#[derive(Clone, Debug)]
pub(super) struct T1Row {
    pub(super) verdict: &'static str,
    pub(super) context_name: String,
    pub(super) canonical_crate: Option<String>,
    pub(super) owning_rfc: Option<String>,
    pub(super) evidence: String,
}

impl T1Row {
    pub(super) fn into_row(self) -> Row {
        let mut row = BTreeMap::new();
        row.insert(
            "verdict".to_string(),
            RowValue::Scalar(PropValue::Str(self.verdict.to_string())),
        );
        row.insert(
            "context_name".to_string(),
            RowValue::Scalar(PropValue::Str(self.context_name)),
        );
        row.insert(
            "canonical_crate".to_string(),
            RowValue::Scalar(
                self.canonical_crate
                    .map(PropValue::Str)
                    .unwrap_or(PropValue::Null),
            ),
        );
        row.insert(
            "owning_rfc".to_string(),
            RowValue::Scalar(
                self.owning_rfc
                    .map(PropValue::Str)
                    .unwrap_or(PropValue::Null),
            ),
        );
        row.insert(
            "evidence".to_string(),
            RowValue::Scalar(PropValue::Str(self.evidence)),
        );
        row
    }
}

#[derive(Clone, Debug)]
pub(super) struct ContextRow {
    pub(super) name: String,
    pub(super) canonical_crate: Option<String>,
    pub(super) owning_rfc: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct T3Row {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) n: i64,
    pub(super) n_crates: i64,
    pub(super) n_contexts: i64,
    pub(super) crates: Vec<String>,
    pub(super) bounded_contexts: Vec<String>,
    pub(super) qnames: Vec<String>,
    pub(super) files: Vec<String>,
    pub(super) is_cross_context: bool,
    pub(super) canonical_candidate: Option<String>,
}

impl T3Row {
    pub(super) fn into_row(self) -> Row {
        let mut row = BTreeMap::new();
        row.insert(
            "name".to_string(),
            RowValue::Scalar(PropValue::Str(self.name)),
        );
        row.insert(
            "kind".to_string(),
            RowValue::Scalar(PropValue::Str(self.kind)),
        );
        row.insert("n".to_string(), RowValue::Scalar(PropValue::Int(self.n)));
        row.insert(
            "n_crates".to_string(),
            RowValue::Scalar(PropValue::Int(self.n_crates)),
        );
        row.insert(
            "n_contexts".to_string(),
            RowValue::Scalar(PropValue::Int(self.n_contexts)),
        );
        row.insert(
            "crates".to_string(),
            RowValue::List(self.crates.into_iter().map(PropValue::Str).collect()),
        );
        row.insert(
            "bounded_contexts".to_string(),
            RowValue::List(
                self.bounded_contexts
                    .into_iter()
                    .map(PropValue::Str)
                    .collect(),
            ),
        );
        row.insert(
            "qnames".to_string(),
            RowValue::List(self.qnames.into_iter().map(PropValue::Str).collect()),
        );
        row.insert(
            "files".to_string(),
            RowValue::List(self.files.into_iter().map(PropValue::Str).collect()),
        );
        row.insert(
            "is_cross_context".to_string(),
            RowValue::Scalar(PropValue::Bool(self.is_cross_context)),
        );
        row.insert(
            "canonical_candidate".to_string(),
            RowValue::Scalar(
                self.canonical_candidate
                    .map(PropValue::Str)
                    .unwrap_or(PropValue::Null),
            ),
        );
        row
    }
}
