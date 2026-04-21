//! `cfdb check --trigger <ID>` — cfdb editorial-drift trigger verb.
//!
//! Runs a closed, versioned registry of cypher rules against a
//! keyspace, computes per-trigger findings, tags each row with its
//! verdict / correlation columns, and emits one JSON payload. v0.1
//! ships two triggers:
//!
//! - `T1` — concept-declared-in-TOML-but-missing-in-code (three
//!   sub-verdicts: CONCEPT_UNWIRED / MISSING_CANONICAL_CRATE /
//!   STALE_RFC_REFERENCE).
//! - `T3` — concept-name-in-≥2-crates raw Pattern A detection with
//!   per-row `is_cross_context` boolean + `canonical_candidate`
//!   lookup.
//!
//! # Verdict / correlation computation lives in Rust
//!
//! Both triggers run primitive cypher reads and apply per-row logic
//! in Rust because the cfdb-query v0.1 subset evaluator has three
//! anti-join limitations (outer-bound vars inaccessible in
//! `NOT EXISTS`, `OPTIONAL MATCH WHERE` drops unmatched rows instead
//! of null-filling, `collect()` lists not addressable by `IN`) that
//! make pure-cypher expression of these patterns fragile. The
//! read/project/emit split keeps each cypher rule self-standing and
//! deterministic; correlation is a closed typed Rust computation.
//! Header comments on the respective `.cypher` files carry the
//! per-rule rationale.
//!
//! # TriggerId bounded context
//!
//! `cfdb_cli::check::TriggerId` is distinct from
//! `check_prelude_triggers::trigger_id::TriggerId`:
//!
//! - This enum: cfdb editorial-drift triggers, variants `T1..Tn`
//!   (capital-T), detecting TOML-vs-code drift.
//! - That enum: RFC-034 mechanical C-triggers for graph-specs-rust
//!   companion prelude enforcement, variants `C1..C9` (capital-C).
//!
//! Different bounded contexts, different serialization namespaces,
//! independent change vectors.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use cfdb_core::fact::PropValue;
use cfdb_core::result::{Row, RowValue};

mod t1;
mod t3;
#[cfg(test)]
mod tests;

/// Embedded cypher that dumps the `:Context` inventory for T1. The
/// file lives under `examples/queries/` so operators can also run it
/// standalone via `cfdb violations --rule <path>` for ad-hoc
/// inspection — the `violations` verb returns the same three columns.
pub(super) const T1_CONTEXT_INVENTORY_CYPHER: &str =
    include_str!("../../../examples/queries/t1-concept-unwired.cypher");

/// Primitive reads for the correlation sets. Kept inline (not as
/// separate files) because they are trivial one-line queries the
/// check verb owns — not reusable rules.
pub(super) const T1_CRATE_NAMES_CYPHER: &str =
    "MATCH (k:Crate) RETURN k.name AS name ORDER BY name ASC";
pub(super) const T1_ITEM_BOUNDED_CONTEXTS_CYPHER: &str =
    "MATCH (i:Item) RETURN i.bounded_context AS bc ORDER BY bc ASC";
pub(super) const T1_RFC_DOCS_CYPHER: &str =
    "MATCH (r:RfcDoc) RETURN r.path AS path, r.title AS title ORDER BY path ASC";

/// Embedded cypher for trigger T3 — same-name-in-≥2-crates raw
/// detection. Sibling of `hsb-by-name.cypher`: adds the `n_contexts`
/// and `bounded_contexts[]` columns needed to compute the per-row
/// `is_cross_context` boolean in Rust. See the rule file header for
/// the kind-restriction doctrine and the reason for a sibling file
/// rather than an in-place extension of hsb-by-name.
pub(super) const T3_CONCEPT_MULTI_CRATE_CYPHER: &str =
    include_str!("../../../examples/queries/t3-concept-multi-crate.cypher");

/// Primitive read for the T3 canonical-candidate correlation set:
/// every `:Context.canonical_crate` value in the keyspace. A crate
/// qualifies as the `canonical_candidate` for a T3 row when the row's
/// `crates[]` list contains at least one crate that appears in this
/// set. Lookup runs once per T3 invocation — the number of declared
/// contexts is O(dozens) in practice.
pub(super) const T3_CANONICAL_CRATES_CYPHER: &str =
    "MATCH (c:Context) RETURN c.canonical_crate AS canonical_crate ORDER BY canonical_crate ASC";

/// Cfdb editorial-drift trigger identifier (qbot-core council-4046
/// Phase 2 naming). T1 detects concept declarations that are unwired,
/// missing their canonical crate, or point at stale RFCs. T3 detects
/// same-name items across multiple crates (the raw Pattern A signal,
/// enriched with a per-row `is_cross_context` boolean).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerId {
    /// Concept-declared-in-TOML-but-missing-in-code (three sub-verdicts).
    T1,
    /// Concept-name-in-≥2-crates (raw Pattern A, restricted to
    /// struct/enum/trait; carries `is_cross_context` flag).
    T3,
}

impl TriggerId {
    /// Canonical list of every known trigger id. The `FromStr`
    /// implementation iterates this list for its error message so the
    /// set of valid values in the error string never diverges from the
    /// enum itself (global CLAUDE.md §7 MCP/CLI boundary fix AC).
    pub fn variants() -> &'static [TriggerId] {
        &[TriggerId::T1, TriggerId::T3]
    }

    /// Stable wire form. Matches the trigger ID documented in the
    /// qbot-core council-4046 Phase 2 spec (e.g. `"T1"`, `"T3"`).
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

/// Parse error for [`TriggerId`]. Carries the rejected input so the
/// `Display` impl can enumerate the valid set derived from
/// [`TriggerId::variants`].
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

/// `cfdb check --trigger <ID> --db <path> --keyspace <name>` entry.
/// Dispatches to the per-trigger runner and returns the total row
/// count so the clap dispatch arm can apply the same exit-1-on-rows
/// rule that `Command::Violations` uses.
pub fn check(db: &Path, keyspace: &str, trigger: TriggerId) -> Result<usize, crate::CfdbCliError> {
    match trigger {
        TriggerId::T1 => t1::run(db, keyspace),
        TriggerId::T3 => t3::run(db, keyspace),
    }
}

/// One T1 finding, produced by the Rust-side anti-join logic and
/// projected into a row shape compatible with
/// `cfdb_core::result::Row` so it can land in the merged
/// `QueryResult` JSON payload alongside any shared warnings.
#[derive(Clone, Debug)]
pub(super) struct Finding {
    pub(super) verdict: &'static str,
    pub(super) context_name: String,
    pub(super) canonical_crate: Option<String>,
    pub(super) owning_rfc: Option<String>,
    pub(super) evidence: String,
}

impl Finding {
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

/// One row from the `:Context` inventory cypher.
#[derive(Clone, Debug)]
pub(super) struct ContextRow {
    pub(super) name: String,
    pub(super) canonical_crate: Option<String>,
    pub(super) owning_rfc: Option<String>,
}

/// One T3 row, after the per-row `is_cross_context` + `canonical_candidate`
/// derivations. Kept as a local struct (not named `Finding`) to avoid
/// collision with the two existing `Finding` types in the codebase —
/// the pure-data `cfdb_query::Finding` (scope inventory rows) and the
/// T1-local `Finding` above (anti-join products). T3's semantic is
/// different enough to warrant its own row type.
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
