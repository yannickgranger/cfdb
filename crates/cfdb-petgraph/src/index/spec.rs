//! `IndexSpec` — parsed `.cfdb/indexes.toml` (RFC-035 §3.2, slice 1 #180).
//!
//! The spec names which `(Label, prop)` pairs are indexed at ingest time.
//! Each entry carries a required `notes` string documenting the
//! rationale — who consumes the index, why it is indexed. Pattern-match
//! on `.cfdb/skill-routing.toml` where every row carries the same kind
//! of rationale (R1 R2 resolution — DDD lens).
//!
//! The loader mirrors `cfdb_query::SkillRoutingTable`:
//! [`IndexSpec::from_path`] does I/O, [`IndexSpec::from_toml_str`] is a
//! pure parser usable from unit tests without touching the filesystem.
//!
//! # v0.1 scope
//!
//! Slice 1 ships only the spec + TOML loader. The build pass,
//! evaluator fast paths, and composition-root wiring land in later
//! slices (2, 5, 6, 7). `ComputedKey::LastSegment` is parseable here
//! but has no `evaluate` method until slice 3 wires the
//! `cfdb_core::qname::last_segment` helper (RFC-035 §3.3).
//!
//! # File shape
//!
//! ```toml
//! [[index]]
//! label = "Item"
//! prop = "qname"
//! notes = "Join key for list-callers and find-canonical verbs; high-cardinality, always indexed."
//!
//! [[index]]
//! label = "Item"
//! computed = "last_segment(qname)"
//! notes = "Homonym-pair join key for context_homonym classifier rule."
//! ```
//!
//! Each `[[index]]` entry has exactly one of `prop` or `computed` (the
//! `IndexEntry` enum is serde-`untagged`) plus `label` and `notes`.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Parsed `.cfdb/indexes.toml` content. Owns a `Vec<IndexEntry>` in
/// TOML document order — the build pass (slice 2) iterates the vector,
/// so order is stable across runs on identical inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexSpec {
    /// One `[[index]]` array entry per indexed `(Label, key)` pair.
    /// Renamed to `index` in the TOML surface to match the `[[index]]`
    /// array-of-tables convention from RFC-035 §3.2.
    #[serde(rename = "index", default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<IndexEntry>,
}

/// A single `[[index]]` TOML entry. Two shapes, distinguished by
/// whether the entry names a plain `prop` or a `computed` key. Both
/// shapes carry:
/// - `label` — the node label the index is built on (e.g. `"Item"`).
/// - `notes` — **required** free-form rationale. An entry missing
///   `notes` is rejected by the parser.
///
/// Deserialisation routes through a flat intermediate representation
/// (see `RawIndexEntry` below) rather than serde's `untagged` enum
/// dispatch — `untagged` collapses every variant-mismatch into
/// "data did not match any variant", losing the actual cause (missing
/// `notes`, unknown computed key, both fields set). Routing through
/// a flat struct preserves the specific error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexEntry {
    /// Index on a literal prop value (e.g. `:Item.qname`).
    Prop {
        label: String,
        prop: String,
        notes: String,
    },
    /// Index on a computed key (pure function of a node's props, e.g.
    /// `last_segment(qname)`). The `ComputedKey` enum is a closed
    /// `const`-sized allowlist per RFC-035 §3.4 — extending the
    /// allowlist is an RFC-gated change.
    Computed {
        label: String,
        computed: ComputedKey,
        notes: String,
    },
}

/// Flat intermediate shape used for (de)serialisation. `prop` and
/// `computed` are both optional at the TOML level; exactly one must
/// be present — enforced by [`IndexEntry`]'s custom `Deserialize`.
#[derive(Serialize, Deserialize)]
struct RawIndexEntry {
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prop: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    computed: Option<String>,
    notes: String,
}

impl Serialize for IndexEntry {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let raw = match self {
            IndexEntry::Prop { label, prop, notes } => RawIndexEntry {
                label: label.clone(),
                prop: Some(prop.clone()),
                computed: None,
                notes: notes.clone(),
            },
            IndexEntry::Computed {
                label,
                computed,
                notes,
            } => RawIndexEntry {
                label: label.clone(),
                prop: None,
                computed: Some(computed.as_str().to_string()),
                notes: notes.clone(),
            },
        };
        raw.serialize(s)
    }
}

impl<'de> Deserialize<'de> for IndexEntry {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = RawIndexEntry::deserialize(d)?;
        match (raw.prop, raw.computed) {
            (Some(_), Some(_)) => Err(serde::de::Error::custom(
                "index entry has both `prop` and `computed` set — pick one",
            )),
            (None, None) => Err(serde::de::Error::custom(
                "index entry missing both `prop` and `computed` — exactly one required",
            )),
            (Some(prop), None) => Ok(IndexEntry::Prop {
                label: raw.label,
                prop,
                notes: raw.notes,
            }),
            (None, Some(name)) => {
                let computed = name
                    .parse::<ComputedKey>()
                    .map_err(serde::de::Error::custom)?;
                Ok(IndexEntry::Computed {
                    label: raw.label,
                    computed,
                    notes: raw.notes,
                })
            }
        }
    }
}

/// Allowlisted computed-key functions. Closed `const`-sized enum —
/// NOT a trait registry (RFC-035 §3.4 OCP decision / R1 B4
/// resolution). Each variant is a wrapper around a canonical
/// `cfdb_core::qname::*` helper that serves as its invariant owner
/// (RFC-035 §3.3 / R1 B3 resolution).
///
/// v0.1 ships only `LastSegment`. Extending the allowlist ships with
/// its own follow-up RFC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedKey {
    /// `last_segment(qname)` — splits the qname at the last `::` and
    /// returns the trailing segment. Semantic anchor:
    /// `cfdb_core::qname::last_segment` (helper lands in slice 3).
    LastSegment,
}

impl ComputedKey {
    /// Canonical string form used on the TOML surface and as the
    /// virtual-prop tag for build-pass entries.
    pub fn as_str(self) -> &'static str {
        match self {
            ComputedKey::LastSegment => "last_segment(qname)",
        }
    }
}

impl std::fmt::Display for ComputedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ComputedKey {
    type Err = UnknownComputedKey;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "last_segment(qname)" => Ok(ComputedKey::LastSegment),
            other => Err(UnknownComputedKey(other.to_string())),
        }
    }
}

/// Error raised when a TOML `computed = "..."` string is not in the
/// allowlist (RFC-035 §3.4). Carries the offending string for error
/// messages.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownComputedKey(pub String);

impl std::fmt::Display for UnknownComputedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown computed key `{}` — allowed: last_segment(qname)",
            self.0
        )
    }
}

impl std::error::Error for UnknownComputedKey {}

impl Serialize for ComputedKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ComputedKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Errors that can arise loading an [`IndexSpec`] from disk.
#[derive(Debug)]
pub enum IndexSpecLoadError {
    /// Filesystem-level error reading the TOML file.
    Io(std::io::Error),
    /// TOML parse error — malformed file contents, missing `notes`,
    /// unknown computed key, etc.
    Toml(String),
}

impl std::fmt::Display for IndexSpecLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexSpecLoadError::Io(e) => write!(f, "read indexes.toml: {e}"),
            IndexSpecLoadError::Toml(msg) => write!(f, "parse indexes.toml: {msg}"),
        }
    }
}

impl std::error::Error for IndexSpecLoadError {}

impl IndexSpec {
    /// An empty spec — no indexes declared. Returned by
    /// [`Self::from_path`] when the TOML file is absent (which is not
    /// an error: keyspaces without an `indexes.toml` run with the
    /// existing full-scan paths).
    pub fn empty() -> Self {
        Self::default()
    }

    /// True when the spec declares no indexes. Callers can skip the
    /// build pass entirely when this is true.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Load the spec from a TOML file on disk. Missing file is not an
    /// error — returns [`IndexSpec::empty()`]. This matches the
    /// composition-root contract: `.cfdb/indexes.toml` is optional, and
    /// its absence means "no indexes, full-scan paths only".
    pub fn from_path(path: &Path) -> Result<Self, IndexSpecLoadError> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::from_toml_str(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty()),
            Err(e) => Err(IndexSpecLoadError::Io(e)),
        }
    }

    /// Parse a spec from a TOML string. Factored out of [`Self::from_path`]
    /// so unit tests can exercise parsing without touching the filesystem.
    pub fn from_toml_str(s: &str) -> Result<Self, IndexSpecLoadError> {
        toml::from_str(s).map_err(|e| IndexSpecLoadError::Toml(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREE_ENTRY_TOML: &str = r#"
[[index]]
label = "Item"
prop = "qname"
notes = "Join key for list-callers and find-canonical verbs."

[[index]]
label = "Item"
prop = "bounded_context"
notes = "Scope-verb filter predicate (#169 / RFC-035); low-cardinality."

[[index]]
label = "Item"
computed = "last_segment(qname)"
notes = "Homonym-pair join key for context_homonym classifier rule."
"#;

    #[test]
    fn parses_three_entry_fixture() {
        let spec = IndexSpec::from_toml_str(THREE_ENTRY_TOML).expect("parse");
        assert_eq!(spec.entries.len(), 3);

        match &spec.entries[0] {
            IndexEntry::Prop { label, prop, notes } => {
                assert_eq!(label, "Item");
                assert_eq!(prop, "qname");
                assert!(notes.starts_with("Join key"));
            }
            other => panic!("expected Prop variant, got {other:?}"),
        }
        match &spec.entries[2] {
            IndexEntry::Computed {
                label,
                computed,
                notes,
            } => {
                assert_eq!(label, "Item");
                assert_eq!(*computed, ComputedKey::LastSegment);
                assert!(notes.starts_with("Homonym"));
            }
            other => panic!("expected Computed variant, got {other:?}"),
        }
    }

    #[test]
    fn serde_round_trip_via_json_preserves_notes() {
        let spec = IndexSpec::from_toml_str(THREE_ENTRY_TOML).expect("parse");
        let json = serde_json::to_string(&spec).expect("serialize");
        let reparsed: IndexSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(spec, reparsed);
        for (idx, entry) in reparsed.entries.iter().enumerate() {
            let notes = match entry {
                IndexEntry::Prop { notes, .. } | IndexEntry::Computed { notes, .. } => notes,
            };
            assert!(
                !notes.is_empty(),
                "entry {idx} lost its notes across the round-trip"
            );
        }
    }

    #[test]
    fn rejects_entry_missing_notes() {
        let without_notes = r#"
[[index]]
label = "Item"
prop = "qname"
"#;
        let err = IndexSpec::from_toml_str(without_notes).expect_err("must reject");
        let msg = err.to_string();
        assert!(
            msg.contains("parse indexes.toml"),
            "expected wrapped parse error, got: {msg}"
        );
        // The serde error surface mentions the missing field — assert on
        // the presence of `notes` in the message so a future serde upgrade
        // that changes phrasing still flags this test if `notes` drops out
        // of the error.
        assert!(
            msg.contains("notes"),
            "error must reference the `notes` field, got: {msg}"
        );
    }

    #[test]
    fn rejects_unknown_computed_key() {
        let bogus = r#"
[[index]]
label = "Item"
computed = "parent_qpath(qname)"
notes = "not in allowlist"
"#;
        let err = IndexSpec::from_toml_str(bogus).expect_err("must reject");
        let msg = err.to_string();
        assert!(
            msg.contains("parent_qpath"),
            "error must reference the rejected key, got: {msg}"
        );
    }

    #[test]
    fn empty_toml_produces_empty_spec() {
        let spec = IndexSpec::from_toml_str("").expect("parse empty");
        assert!(spec.is_empty());
        assert_eq!(spec, IndexSpec::empty());
    }

    #[test]
    fn missing_file_returns_empty_spec() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let missing = tmp.path().join("does-not-exist.toml");
        let spec = IndexSpec::from_path(&missing).expect("missing file is Ok");
        assert!(spec.is_empty());
    }

    #[test]
    fn from_path_reads_a_real_file() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let path = tmp.path().join("indexes.toml");
        std::fs::write(&path, THREE_ENTRY_TOML).expect("write fixture");
        let spec = IndexSpec::from_path(&path).expect("load");
        assert_eq!(spec.entries.len(), 3);
    }

    #[test]
    fn parse_is_deterministic() {
        let a = IndexSpec::from_toml_str(THREE_ENTRY_TOML).expect("parse a");
        let b = IndexSpec::from_toml_str(THREE_ENTRY_TOML).expect("parse b");
        assert_eq!(a, b);
    }

    #[test]
    fn computed_key_round_trips_as_canonical_string() {
        assert_eq!(ComputedKey::LastSegment.as_str(), "last_segment(qname)");
        assert_eq!(
            "last_segment(qname)".parse::<ComputedKey>().expect("parse"),
            ComputedKey::LastSegment
        );
        assert!("bogus".parse::<ComputedKey>().is_err());
    }
}
