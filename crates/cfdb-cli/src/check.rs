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

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use cfdb_core::fact::PropValue;
use cfdb_core::result::{QueryResult, Row, RowValue, Warning, WarningKind};

use crate::commands::parse_and_execute;

/// Embedded cypher that dumps the `:Context` inventory for T1. The
/// file lives under `examples/queries/` so operators can also run it
/// standalone via `cfdb violations --rule <path>` for ad-hoc
/// inspection — the `violations` verb returns the same three columns.
const T1_CONTEXT_INVENTORY_CYPHER: &str =
    include_str!("../../../examples/queries/t1-concept-unwired.cypher");

/// Primitive reads for the correlation sets. Kept inline (not as
/// separate files) because they are trivial one-line queries the
/// check verb owns — not reusable rules.
const T1_CRATE_NAMES_CYPHER: &str = "MATCH (k:Crate) RETURN k.name AS name ORDER BY name ASC";
const T1_ITEM_BOUNDED_CONTEXTS_CYPHER: &str =
    "MATCH (i:Item) RETURN i.bounded_context AS bc ORDER BY bc ASC";
const T1_RFC_DOCS_CYPHER: &str =
    "MATCH (r:RfcDoc) RETURN r.path AS path, r.title AS title ORDER BY path ASC";

/// Embedded cypher for trigger T3 — same-name-in-≥2-crates raw
/// detection. Sibling of `hsb-by-name.cypher`: adds the `n_contexts`
/// and `bounded_contexts[]` columns needed to compute the per-row
/// `is_cross_context` boolean in Rust. See the rule file header for
/// the kind-restriction doctrine and the reason for a sibling file
/// rather than an in-place extension of hsb-by-name.
const T3_CONCEPT_MULTI_CRATE_CYPHER: &str =
    include_str!("../../../examples/queries/t3-concept-multi-crate.cypher");

/// Primitive read for the T3 canonical-candidate correlation set:
/// every `:Context.canonical_crate` value in the keyspace. A crate
/// qualifies as the `canonical_candidate` for a T3 row when the row's
/// `crates[]` list contains at least one crate that appears in this
/// set. Lookup runs once per T3 invocation — the number of declared
/// contexts is O(dozens) in practice.
const T3_CANONICAL_CRATES_CYPHER: &str =
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
        TriggerId::T1 => run_trigger_t1(db, keyspace),
        TriggerId::T3 => run_trigger_t3(db, keyspace),
    }
}

/// One T1 finding, produced by the Rust-side anti-join logic and
/// projected into a row shape compatible with
/// `cfdb_core::result::Row` so it can land in the merged
/// `QueryResult` JSON payload alongside any shared warnings.
#[derive(Clone, Debug)]
struct Finding {
    verdict: &'static str,
    context_name: String,
    canonical_crate: Option<String>,
    owning_rfc: Option<String>,
    evidence: String,
}

impl Finding {
    fn into_row(self) -> Row {
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
struct ContextRow {
    name: String,
    canonical_crate: Option<String>,
    owning_rfc: Option<String>,
}

/// Run the T1 trigger: fetch the four correlation sets, compute the
/// three anti-join sub-verdicts in Rust, emit the merged payload.
fn run_trigger_t1(db: &Path, keyspace: &str) -> Result<usize, crate::CfdbCliError> {
    let contexts = fetch_contexts(db, keyspace)?;
    let crate_names = fetch_scalar_set(db, keyspace, T1_CRATE_NAMES_CYPHER, "name")?;
    let item_contexts = fetch_scalar_set(db, keyspace, T1_ITEM_BOUNDED_CONTEXTS_CYPHER, "bc")?;
    let rfc_haystack = fetch_rfc_haystack(db, keyspace)?;

    let mut findings: Vec<Finding> = Vec::new();
    for ctx in &contexts {
        if !item_contexts.contains(&ctx.name) {
            findings.push(Finding {
                verdict: "CONCEPT_UNWIRED",
                context_name: ctx.name.clone(),
                canonical_crate: ctx.canonical_crate.clone(),
                owning_rfc: ctx.owning_rfc.clone(),
                evidence: ctx.name.clone(),
            });
        }
        if let Some(canonical) = ctx.canonical_crate.as_deref() {
            if !canonical.is_empty() && !crate_names.contains(canonical) {
                findings.push(Finding {
                    verdict: "MISSING_CANONICAL_CRATE",
                    context_name: ctx.name.clone(),
                    canonical_crate: ctx.canonical_crate.clone(),
                    owning_rfc: ctx.owning_rfc.clone(),
                    evidence: canonical.to_string(),
                });
            }
        }
        if let Some(rfc) = ctx.owning_rfc.as_deref() {
            if !rfc.is_empty() && !rfc_haystack.iter().any(|hay| hay.contains(rfc)) {
                findings.push(Finding {
                    verdict: "STALE_RFC_REFERENCE",
                    context_name: ctx.name.clone(),
                    canonical_crate: ctx.canonical_crate.clone(),
                    owning_rfc: ctx.owning_rfc.clone(),
                    evidence: rfc.to_string(),
                });
            }
        }
    }

    // Determinism: stable order regardless of the per-context
    // verdict-check order. `(context_name, verdict)` is the canonical
    // sort key — same shape as the cypher file's `ORDER BY`.
    findings.sort_by(|a, b| {
        a.context_name
            .cmp(&b.context_name)
            .then_with(|| a.verdict.cmp(b.verdict))
    });

    let mut merged = QueryResult::empty();
    for f in findings {
        merged.rows.push(f.into_row());
    }

    if rfc_haystack.is_empty() {
        merged.warnings.push(Warning {
            kind: WarningKind::EmptyResult,
            message: "no :RfcDoc nodes in keyspace — STALE_RFC_REFERENCE sub-verdict is \
                      evaluated against an empty RFC document set. Any `owning_rfc` tag will \
                      surface as stale. Run `cfdb enrich-rfc-docs --db <db> --keyspace <ks> \
                      --workspace <path>` to populate the doc inventory before checking T1."
                .to_string(),
            suggestion: Some(
                "cfdb enrich-rfc-docs --db <db> --keyspace <ks> --workspace <path>".to_string(),
            ),
        });
    }

    let row_count = merged.rows.len();
    eprintln!("violations: {row_count} (rule: trigger T1)");

    let as_json = serde_json::to_string_pretty(&merged)?;
    println!("{as_json}");

    Ok(row_count)
}

/// Execute the embedded `:Context` inventory cypher and project each
/// row into a `ContextRow`. Non-string props in the returned rows are
/// treated as null (defensive — the extractor only emits string
/// values for these keys, but the cypher layer is untyped).
fn fetch_contexts(db: &Path, keyspace: &str) -> Result<Vec<ContextRow>, crate::CfdbCliError> {
    let result = parse_and_execute(
        db,
        keyspace,
        T1_CONTEXT_INVENTORY_CYPHER,
        "trigger T1 / :Context inventory",
    )?;
    let contexts = result
        .rows
        .into_iter()
        .filter_map(|row| {
            let name = scalar_str_owned(&row, "context_name")?;
            Some(ContextRow {
                name,
                canonical_crate: scalar_str_owned(&row, "canonical_crate"),
                owning_rfc: scalar_str_owned(&row, "owning_rfc"),
            })
        })
        .collect();
    Ok(contexts)
}

/// Execute a simple `MATCH … RETURN col` cypher and collect the
/// requested column's scalar-string values into a deduplicating set.
/// Missing rows / non-string values are skipped.
fn fetch_scalar_set(
    db: &Path,
    keyspace: &str,
    cypher: &str,
    col: &str,
) -> Result<BTreeSet<String>, crate::CfdbCliError> {
    let rule_tag = format!("trigger T1 / {col} probe");
    let result = parse_and_execute(db, keyspace, cypher, &rule_tag)?;
    Ok(result
        .rows
        .into_iter()
        .filter_map(|row| scalar_str_owned(&row, col))
        .collect())
}

/// Pull every `:RfcDoc.path` and `:RfcDoc.title` into a single vector
/// of strings. STALE_RFC_REFERENCE tests whether any element of the
/// vector contains the `owning_rfc` tag as a substring — same
/// semantics the cypher's `r.path =~ tag OR r.title =~ tag` would have
/// if the evaluator supported outer-bound regex in OPTIONAL MATCH
/// (it does not, per the cypher file header).
fn fetch_rfc_haystack(db: &Path, keyspace: &str) -> Result<Vec<String>, crate::CfdbCliError> {
    let result = parse_and_execute(
        db,
        keyspace,
        T1_RFC_DOCS_CYPHER,
        "trigger T1 / :RfcDoc probe",
    )?;
    let mut out = Vec::with_capacity(result.rows.len() * 2);
    for row in &result.rows {
        if let Some(path) = scalar_str_owned(row, "path") {
            out.push(path);
        }
        if let Some(title) = scalar_str_owned(row, "title") {
            out.push(title);
        }
    }
    Ok(out)
}

/// Extract a `RowValue::Scalar(PropValue::Str)` into an owned `String`.
/// Returns `None` for missing keys, null values, or non-string values.
fn scalar_str_owned(row: &Row, key: &str) -> Option<String> {
    match row.get(key)? {
        RowValue::Scalar(PropValue::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

/// One T3 row, after the per-row `is_cross_context` + `canonical_candidate`
/// derivations. Kept as a local struct (not named `Finding`) to avoid
/// collision with the two existing `Finding` types in the codebase —
/// the pure-data `cfdb_query::Finding` (scope inventory rows) and the
/// T1-local `Finding` above (anti-join products). T3's semantic is
/// different enough to warrant its own row type.
#[derive(Clone, Debug)]
struct T3Row {
    name: String,
    kind: String,
    n: i64,
    n_crates: i64,
    n_contexts: i64,
    crates: Vec<String>,
    bounded_contexts: Vec<String>,
    qnames: Vec<String>,
    files: Vec<String>,
    is_cross_context: bool,
    canonical_candidate: Option<String>,
}

impl T3Row {
    fn into_row(self) -> Row {
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

/// Run the T3 trigger: execute the embedded multi-crate cypher, fetch
/// the canonical-crate set once, then per-row derive
/// `is_cross_context` (`n_contexts > 1`) and `canonical_candidate`
/// (the first element of `crates[]` that appears in any
/// `:Context.canonical_crate`, or `null` if none). Emit the merged
/// JSON payload with stable ordering.
fn run_trigger_t3(db: &Path, keyspace: &str) -> Result<usize, crate::CfdbCliError> {
    let raw = parse_and_execute(
        db,
        keyspace,
        T3_CONCEPT_MULTI_CRATE_CYPHER,
        "trigger T3 / Pattern A multi-crate",
    )?;
    let canonical_crates =
        fetch_scalar_set(db, keyspace, T3_CANONICAL_CRATES_CYPHER, "canonical_crate")?;

    let mut rows_out: Vec<T3Row> = Vec::with_capacity(raw.rows.len());
    for row in raw.rows {
        if let Some(t3) = project_t3_row(&row, &canonical_crates) {
            rows_out.push(t3);
        }
    }

    // Stable order — same sort key as the cypher's ORDER BY: n DESC,
    // name ASC. The cypher's projection already sorted by this key,
    // but we re-sort defensively since the Rust-side projection
    // iterates in receive order. Ties on `n` resolve by `name`.
    rows_out.sort_by(|a, b| b.n.cmp(&a.n).then_with(|| a.name.cmp(&b.name)));

    let mut merged = QueryResult::empty();
    for r in rows_out {
        merged.rows.push(r.into_row());
    }

    let row_count = merged.rows.len();
    eprintln!("violations: {row_count} (rule: trigger T3)");

    let as_json = serde_json::to_string_pretty(&merged)?;
    println!("{as_json}");

    Ok(row_count)
}

/// Project one raw cypher row into a [`T3Row`], computing
/// `is_cross_context` + `canonical_candidate` in Rust because the
/// v0.1 cypher subset RETURN clause does not evaluate boolean or
/// outer-bound OPTIONAL MATCH correlations reliably (see the T3 cypher
/// header for the limitations).
fn project_t3_row(row: &Row, canonical_crates: &BTreeSet<String>) -> Option<T3Row> {
    let name = scalar_str_owned(row, "name")?;
    let kind = scalar_str_owned(row, "kind")?;
    let n = scalar_int(row, "n")?;
    let n_crates = scalar_int(row, "n_crates")?;
    let n_contexts = scalar_int(row, "n_contexts")?;
    let crates = list_str_owned(row, "crates");
    let bounded_contexts = list_str_owned(row, "bounded_contexts");
    let qnames = list_str_owned(row, "qnames");
    let files = list_str_owned(row, "files");

    let is_cross_context = n_contexts > 1;
    // Pick the first crate in this row's `crates[]` list that is
    // declared as some `:Context.canonical_crate` — this is the
    // "canonical candidate" in the T3 row shape per issue AC-4.
    // `BTreeSet::contains` is O(log n); `crates[]` is small in
    // practice (workspace-scale, not ecosystem-scale).
    let canonical_candidate = crates
        .iter()
        .find(|c| canonical_crates.contains(*c))
        .cloned();

    Some(T3Row {
        name,
        kind,
        n,
        n_crates,
        n_contexts,
        crates,
        bounded_contexts,
        qnames,
        files,
        is_cross_context,
        canonical_candidate,
    })
}

/// Extract an integer value from a row column. Returns `None` for
/// missing keys, null values, or non-integer values. Matches the
/// `RowValue::Scalar(PropValue::Int(_))` shape produced by
/// `count(...)` aggregations in the v0.1 evaluator.
fn scalar_int(row: &Row, key: &str) -> Option<i64> {
    match row.get(key)? {
        RowValue::Scalar(PropValue::Int(n)) => Some(*n),
        _ => None,
    }
}

/// Extract a list column as owned `Vec<String>`, filtering to
/// scalar-string elements. Matches the `RowValue::List(Vec<PropValue>)`
/// shape produced by `collect(...)` aggregations.
fn list_str_owned(row: &Row, key: &str) -> Vec<String> {
    match row.get(key) {
        Some(RowValue::List(items)) => items
            .iter()
            .filter_map(|p| match p {
                PropValue::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_id_display_is_uppercase_tag() {
        assert_eq!(TriggerId::T1.to_string(), "T1");
    }

    #[test]
    fn trigger_id_from_str_round_trips_every_variant() {
        // Anti-regression for issue #102 (T3): adding a variant to
        // `variants()` must automatically let it parse without a
        // hardcoded edit anywhere else.
        for v in TriggerId::variants() {
            let spelled = v.to_string();
            let parsed: TriggerId = spelled.parse().expect("round-trip");
            assert_eq!(&parsed, v);
        }
    }

    #[test]
    fn trigger_id_from_str_rejects_unknown_with_derived_valid_values() {
        let err = TriggerId::from_str("T999").expect_err("unknown should fail");
        // Rejected input is carried verbatim.
        assert_eq!(err.0, "T999");
        // Error message enumerates every known variant, derived from
        // the enum — NEVER hardcoded. Adding a new variant updates
        // this error surface automatically.
        let msg = err.to_string();
        for v in TriggerId::variants() {
            assert!(
                msg.contains(v.as_str()),
                "error message missing {}: {msg}",
                v.as_str()
            );
        }
        assert!(
            msg.contains("valid values:"),
            "error message missing preamble: {msg}"
        );
    }

    #[test]
    fn trigger_id_from_str_is_case_sensitive() {
        // Stable wire form is uppercase — lowercase and mixed-case
        // must not silently parse to the same variant. Downstream
        // tooling reads the tag off argv and compares by equality.
        assert!(TriggerId::from_str("t1").is_err());
        assert!(TriggerId::from_str("T1").is_ok());
        assert!(TriggerId::from_str("t3").is_err());
        assert!(TriggerId::from_str("T3").is_ok());
    }

    #[test]
    fn trigger_id_variants_enumerates_t1_and_t3_in_stable_order() {
        // The variant order is load-bearing: consumer skills
        // (`/operate-module`, etc.) may iterate `variants()` and
        // depend on the numeric-suffix ordering. Pin it explicitly.
        assert_eq!(
            TriggerId::variants(),
            &[TriggerId::T1, TriggerId::T3],
            "TriggerId::variants() order must be T1, T3 — \
             consumer skills rely on stable iteration order"
        );
    }
}
