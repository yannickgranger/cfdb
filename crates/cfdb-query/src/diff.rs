//! Two-keyspace delta over canonical sorted-JSONL dumps (RFC-cfdb.md §12.1).
//!
//! `compute_diff` is backend-agnostic: it consumes the `String` output of
//! `StoreBackend::canonical_dump` for two keyspaces and returns a
//! [`DiffEnvelope`]. Determinism is inherited from the caller — sorted input
//! is preserved through `BTreeMap` keying and stable iteration.
//!
//! # Envelope schema versioning
//!
//! [`DiffEnvelope::schema_version`] is pinned to [`ENVELOPE_SCHEMA_VERSION`]
//! and versions the wire shape of the envelope itself, NOT
//! `cfdb_core::SchemaVersion` (which versions on-disk keyspaces). Bump only
//! when the envelope shape changes in a breaking way.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Envelope schema version. Bumped independently of `cfdb_core::SchemaVersion`.
pub const ENVELOPE_SCHEMA_VERSION: &str = "v1";

const KIND_NODE: &str = "node";
const KIND_EDGE: &str = "edge";

/// Wire envelope for `cfdb diff` — a two-keyspace delta over the canonical
/// sorted-JSONL dump. Consumed by `cfdb classify` (#213) and qbot-core
/// #3736's per-PR drift gate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiffEnvelope {
    /// Left keyspace name (raw `--a` CLI arg).
    pub a: String,
    /// Right keyspace name (raw `--b` CLI arg).
    pub b: String,
    /// Envelope schema version — always [`ENVELOPE_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Facts present in B but not in A, BTreeMap-sorted order.
    pub added: Vec<DiffFact>,
    /// Facts present in A but not in B, BTreeMap-sorted order.
    pub removed: Vec<DiffFact>,
    /// Facts whose canonical key exists in both sides but whose envelope
    /// JSON differs (typically `props` drift).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<ChangedFact>,
    /// Informational warnings (unknown kinds filter tokens, parse skips).
    /// Elided from JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl DiffEnvelope {
    fn new(a: &str, b: &str) -> Self {
        Self {
            a: a.to_string(),
            b: b.to_string(),
            schema_version: ENVELOPE_SCHEMA_VERSION.to_string(),
            added: Vec::new(),
            removed: Vec::new(),
            changed: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// One row of `added` or `removed`. Carries the canonical-dump envelope
/// verbatim as a parsed JSON object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiffFact {
    /// `"node"` or `"edge"`.
    pub kind: String,
    /// Full canonical-dump envelope
    /// (`{id, kind:"node", label, props}` or
    /// `{dst_qname, kind:"edge", label, props, src_qname}`).
    pub envelope: Value,
}

/// One row of `changed`. Both envelope versions are carried so consumers
/// can diff at whatever granularity they want.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangedFact {
    /// `"node"` or `"edge"`.
    pub kind: String,
    /// Envelope as it appears in keyspace `a` (the "before").
    pub a: Value,
    /// Envelope as it appears in keyspace `b` (the "after").
    pub b: Value,
}

/// Filter on the `kind` discriminator — restricts the diff to `node` rows,
/// `edge` rows, or both. Parsed from the comma-separated `--kinds` CLI arg.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindsFilter {
    allowed: BTreeSet<String>,
}

impl KindsFilter {
    /// `true` when the given kind string is allowed under this filter.
    pub fn allows(&self, kind: &str) -> bool {
        self.allowed.contains(kind)
    }
}

impl FromStr for KindsFilter {
    type Err = DiffError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let mut allowed = BTreeSet::new();
        for token in raw.split(',') {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }
            match trimmed {
                KIND_NODE | KIND_EDGE => {
                    allowed.insert(trimmed.to_string());
                }
                _ => {
                    return Err(DiffError::UnknownKind {
                        token: trimmed.to_string(),
                    });
                }
            }
        }
        if allowed.is_empty() {
            return Err(DiffError::UnknownKind {
                token: raw.to_string(),
            });
        }
        Ok(Self { allowed })
    }
}

/// Failure modes for [`compute_diff`] and [`KindsFilter::from_str`].
#[derive(Debug, Error)]
pub enum DiffError {
    /// A dump line was not valid JSON.
    #[error("diff: parse error on line {line_number} of {side} dump: {source}")]
    Parse {
        /// Which side — `"a"` or `"b"`.
        side: String,
        /// 1-based line number inside the respective dump string.
        line_number: usize,
        /// Underlying serde_json failure.
        #[source]
        source: serde_json::Error,
    },
    /// A dump line was valid JSON but missing the required canonical fields
    /// (e.g. `kind` discriminator absent, or `kind` unknown, or key fields
    /// missing for its kind).
    #[error("diff: malformed envelope on line {line_number} of {side} dump: {reason}")]
    InvalidEnvelope {
        side: String,
        line_number: usize,
        reason: String,
    },
    /// The `--kinds` argument contained a token that is not `node` or `edge`.
    #[error("diff: unknown kind `{token}` — expected `node` or `edge`")]
    UnknownKind { token: String },
}

/// Canonical lookup key. Nodes key on `(kind, label, qname_or_id)`; edges
/// on `(kind, label, src_qname, dst_qname)`. Tuple ordering matches the
/// canonical_dump sort so BTreeMap iteration produces stable output.
type FactKey = (String, String, String, String);

/// Compute the [`DiffEnvelope`] between two canonical sorted-JSONL dumps.
///
/// `a_dump` and `b_dump` must be the output of `StoreBackend::canonical_dump`
/// (one JSON object per line, LF-separated, sorted per RFC §12.1). The
/// function is pure: given the same inputs it returns a byte-identical
/// envelope via `BTreeMap` stable iteration.
///
/// # `kinds` semantics
///
/// - `None` — include both node and edge rows.
/// - `Some(filter)` — include only rows whose `kind` is in the filter.
pub fn compute_diff(
    a_name: &str,
    b_name: &str,
    a_dump: &str,
    b_dump: &str,
    kinds: Option<&KindsFilter>,
) -> Result<DiffEnvelope, DiffError> {
    let a_index = index_dump("a", a_dump, kinds)?;
    let b_index = index_dump("b", b_dump, kinds)?;

    let mut envelope = DiffEnvelope::new(a_name, b_name);

    // Added: present in b, not in a.
    for (key, raw_line) in b_index.iter() {
        if !a_index.contains_key(key) {
            envelope.added.push(fact_from_line(&key.0, raw_line)?);
        }
    }
    // Removed: present in a, not in b.
    for (key, raw_line) in a_index.iter() {
        if !b_index.contains_key(key) {
            envelope.removed.push(fact_from_line(&key.0, raw_line)?);
        }
    }
    // Changed: present in both, envelope JSON differs. Collect into a
    // Vec via iterator chain to avoid `.clone()` inside a `for` body.
    // `a_index` consumed by value so `key` moves into the ChangedFact
    // construction.
    let changed_facts: Vec<ChangedFact> = a_index
        .into_iter()
        .filter_map(|(key, a_line)| b_index.get(&key).map(|b_line| (key, a_line, b_line)))
        .filter(|(_, a_line, b_line)| a_line != *b_line)
        .map(|(key, a_line, b_line)| {
            Ok::<ChangedFact, DiffError>(ChangedFact {
                kind: key.0,
                a: serde_json::from_str(&a_line).map_err(|source| DiffError::Parse {
                    side: "a".into(),
                    line_number: 0,
                    source,
                })?,
                b: serde_json::from_str(b_line).map_err(|source| DiffError::Parse {
                    side: "b".into(),
                    line_number: 0,
                    source,
                })?,
            })
        })
        .collect::<Result<_, _>>()?;
    envelope.changed = changed_facts;

    Ok(envelope)
}

fn index_dump(
    side: &str,
    dump: &str,
    kinds: Option<&KindsFilter>,
) -> Result<BTreeMap<FactKey, String>, DiffError> {
    let mut out: BTreeMap<FactKey, String> = BTreeMap::new();
    for (zero_based, line) in dump.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_number = zero_based + 1;
        let value: Value = serde_json::from_str(line).map_err(|source| DiffError::Parse {
            side: side.to_string(),
            line_number,
            source,
        })?;
        let key = canonical_key(side, line_number, &value)?;
        if let Some(filter) = kinds {
            if !filter.allows(&key.0) {
                continue;
            }
        }
        out.insert(key, line.to_string());
    }
    Ok(out)
}

fn canonical_key(side: &str, line_number: usize, value: &Value) -> Result<FactKey, DiffError> {
    let kind =
        value
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| DiffError::InvalidEnvelope {
                side: side.to_string(),
                line_number,
                reason: "missing `kind` field".into(),
            })?;
    match kind {
        KIND_NODE => {
            let label = string_field(side, line_number, value, "label")?;
            // Sort key matches canonical_dump's resolved qname: prefer
            // `props.qname`, fall back to `id` (matches the fallback at
            // `crates/cfdb-petgraph/src/canonical_dump.rs:45-54`).
            let qname_or_id = value
                .get("props")
                .and_then(|p| p.get("qname"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_string))
                .ok_or_else(|| DiffError::InvalidEnvelope {
                    side: side.to_string(),
                    line_number,
                    reason: "node envelope has neither `props.qname` nor `id`".into(),
                })?;
            Ok((kind.to_string(), label, qname_or_id, String::new()))
        }
        KIND_EDGE => {
            let label = string_field(side, line_number, value, "label")?;
            let src = string_field(side, line_number, value, "src_qname")?;
            let dst = string_field(side, line_number, value, "dst_qname")?;
            Ok((kind.to_string(), label, src, dst))
        }
        other => Err(DiffError::InvalidEnvelope {
            side: side.to_string(),
            line_number,
            reason: format!("unknown `kind`: `{other}`"),
        }),
    }
}

fn string_field(
    side: &str,
    line_number: usize,
    value: &Value,
    field: &str,
) -> Result<String, DiffError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| DiffError::InvalidEnvelope {
            side: side.to_string(),
            line_number,
            reason: format!("missing string field `{field}`"),
        })
}

fn fact_from_line(kind: &str, line: &str) -> Result<DiffFact, DiffError> {
    let envelope: Value = serde_json::from_str(line).map_err(|source| DiffError::Parse {
        side: "post-indexed".into(),
        line_number: 0,
        source,
    })?;
    Ok(DiffFact {
        kind: kind.to_string(),
        envelope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Canonical dump snippets used across shape tests. Each line is one
    // object, LF-separated, no trailing newline — matches RFC §12.1.

    const NODE_A_ID1: &str =
        r#"{"id":"item:a::X","kind":"node","label":"Item","props":{"qname":"a::X"}}"#;
    const NODE_A_ID2: &str =
        r#"{"id":"item:a::Y","kind":"node","label":"Item","props":{"qname":"a::Y"}}"#;
    const NODE_A_ID3: &str =
        r#"{"id":"item:a::Z","kind":"node","label":"Item","props":{"qname":"a::Z"}}"#;
    const NODE_A_ID2_DRIFTED: &str = r#"{"id":"item:a::Y","kind":"node","label":"Item","props":{"qname":"a::Y","is_test":true}}"#;

    const EDGE_AB: &str =
        r#"{"dst_qname":"a::Y","kind":"edge","label":"CALLS","props":{},"src_qname":"a::X"}"#;
    const EDGE_BC: &str =
        r#"{"dst_qname":"a::Z","kind":"edge","label":"CALLS","props":{},"src_qname":"a::Y"}"#;

    fn join(lines: &[&str]) -> String {
        lines.join("\n")
    }

    #[test]
    fn identical_keyspaces_produce_empty_envelope() {
        let dump = join(&[NODE_A_ID1, NODE_A_ID2, EDGE_AB]);
        let envelope = compute_diff("same", "same", &dump, &dump, None).unwrap();
        assert_eq!(envelope.a, "same");
        assert_eq!(envelope.b, "same");
        assert_eq!(envelope.schema_version, ENVELOPE_SCHEMA_VERSION);
        assert!(envelope.added.is_empty());
        assert!(envelope.removed.is_empty());
        assert!(envelope.changed.is_empty());
        assert!(envelope.warnings.is_empty());
    }

    #[test]
    fn added_only_surfaces_b_minus_a() {
        let a = join(&[NODE_A_ID1]);
        let b = join(&[NODE_A_ID1, NODE_A_ID2]);
        let envelope = compute_diff("a", "b", &a, &b, None).unwrap();
        assert_eq!(envelope.added.len(), 1);
        assert_eq!(envelope.added[0].kind, "node");
        assert_eq!(envelope.added[0].envelope["id"], "item:a::Y");
        assert!(envelope.removed.is_empty());
        assert!(envelope.changed.is_empty());
    }

    #[test]
    fn removed_only_surfaces_a_minus_b() {
        let a = join(&[NODE_A_ID1, NODE_A_ID2]);
        let b = join(&[NODE_A_ID1]);
        let envelope = compute_diff("a", "b", &a, &b, None).unwrap();
        assert_eq!(envelope.removed.len(), 1);
        assert_eq!(envelope.removed[0].envelope["id"], "item:a::Y");
        assert!(envelope.added.is_empty());
        assert!(envelope.changed.is_empty());
    }

    #[test]
    fn changed_only_surfaces_props_drift() {
        let a = join(&[NODE_A_ID1, NODE_A_ID2]);
        let b = join(&[NODE_A_ID1, NODE_A_ID2_DRIFTED]);
        let envelope = compute_diff("a", "b", &a, &b, None).unwrap();
        assert!(envelope.added.is_empty());
        assert!(envelope.removed.is_empty());
        assert_eq!(envelope.changed.len(), 1);
        let row = &envelope.changed[0];
        assert_eq!(row.kind, "node");
        assert_eq!(row.a["props"]["is_test"], Value::Null);
        assert_eq!(row.b["props"]["is_test"], true);
    }

    #[test]
    fn mixed_adds_removes_and_changes() {
        let a = join(&[NODE_A_ID1, NODE_A_ID2, EDGE_AB]);
        let b = join(&[NODE_A_ID1, NODE_A_ID2_DRIFTED, NODE_A_ID3, EDGE_BC]);
        let envelope = compute_diff("a", "b", &a, &b, None).unwrap();
        // Added: NODE_A_ID3, EDGE_BC (b has them, a doesn't)
        assert_eq!(envelope.added.len(), 2);
        // Removed: EDGE_AB (a has it, b doesn't)
        assert_eq!(envelope.removed.len(), 1);
        assert_eq!(envelope.removed[0].kind, "edge");
        // Changed: NODE_A_ID2 → NODE_A_ID2_DRIFTED
        assert_eq!(envelope.changed.len(), 1);
    }

    #[test]
    fn kinds_filter_node_hides_edge_changes() {
        let a = join(&[NODE_A_ID1, EDGE_AB]);
        let b = join(&[NODE_A_ID1, NODE_A_ID2, EDGE_AB, EDGE_BC]);
        let filter = KindsFilter::from_str("node").unwrap();
        let envelope = compute_diff("a", "b", &a, &b, Some(&filter)).unwrap();
        assert_eq!(envelope.added.len(), 1);
        assert_eq!(envelope.added[0].kind, "node");
        // EDGE_BC is filtered out despite being b-only.
        assert!(envelope.removed.is_empty());
    }

    #[test]
    fn serde_round_trip_preserves_envelope() {
        let a = join(&[NODE_A_ID1]);
        let b = join(&[NODE_A_ID1, NODE_A_ID2]);
        let envelope = compute_diff("a", "b", &a, &b, None).unwrap();
        let serialised = serde_json::to_string(&envelope).unwrap();
        let back: DiffEnvelope = serde_json::from_str(&serialised).unwrap();
        assert_eq!(envelope, back);
    }

    #[test]
    fn kinds_filter_accepts_node_edge_and_both() {
        assert!(KindsFilter::from_str("node").is_ok());
        assert!(KindsFilter::from_str("edge").is_ok());
        assert!(KindsFilter::from_str("node,edge").is_ok());
        assert!(KindsFilter::from_str(" node , edge ").is_ok());
    }

    #[test]
    fn kinds_filter_rejects_unknown_tokens() {
        let err = KindsFilter::from_str("Item").unwrap_err();
        assert!(matches!(err, DiffError::UnknownKind { .. }));
        let err = KindsFilter::from_str("node,banana").unwrap_err();
        assert!(matches!(err, DiffError::UnknownKind { .. }));
        let err = KindsFilter::from_str("").unwrap_err();
        assert!(matches!(err, DiffError::UnknownKind { .. }));
    }

    #[test]
    fn compute_diff_is_deterministic_across_calls() {
        let a = join(&[NODE_A_ID1, NODE_A_ID2, EDGE_AB]);
        let b = join(&[NODE_A_ID1, NODE_A_ID3, EDGE_BC]);
        let one = compute_diff("a", "b", &a, &b, None).unwrap();
        let two = compute_diff("a", "b", &a, &b, None).unwrap();
        assert_eq!(
            serde_json::to_string(&one).unwrap(),
            serde_json::to_string(&two).unwrap()
        );
    }

    #[test]
    fn malformed_json_reports_line_number() {
        let a = join(&[NODE_A_ID1, "not json"]);
        let err = compute_diff("a", "b", &a, "", None).unwrap_err();
        match err {
            DiffError::Parse {
                side, line_number, ..
            } => {
                assert_eq!(side, "a");
                assert_eq!(line_number, 2);
            }
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn envelope_missing_kind_is_invalid() {
        let bad = r#"{"id":"x","label":"Item","props":{}}"#;
        let err = compute_diff("a", "b", bad, "", None).unwrap_err();
        assert!(matches!(err, DiffError::InvalidEnvelope { .. }));
    }
}
