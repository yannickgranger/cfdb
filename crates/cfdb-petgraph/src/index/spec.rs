use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexSpec {
    #[serde(rename = "index", default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<IndexEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexEntry {
    Prop {
        label: String,
        prop: String,
        notes: String,
    },
    Computed {
        label: String,
        computed: ComputedKey,
        notes: String,
    },
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedKey {
    LastSegment,
    ConversionPrefix,
}

pub const CONVERSION_PREFIX_PATTERN: &str = r"^(\w+)_(?:from|to|for|as)_";

static CONVERSION_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(CONVERSION_PREFIX_PATTERN).expect("vetted ConversionPrefix regex compiles")
});

impl ComputedKey {
    pub fn as_str(self) -> &'static str {
        match self {
            ComputedKey::LastSegment => "last_segment(qname)",
            ComputedKey::ConversionPrefix => "conversion_prefix(name)",
        }
    }

    pub fn source_prop(self) -> &'static str {
        match self {
            ComputedKey::LastSegment => "qname",
            ComputedKey::ConversionPrefix => "name",
        }
    }

    #[must_use]
    pub fn evaluate(self, source: &str) -> Option<&str> {
        match self {
            ComputedKey::LastSegment => Some(cfdb_core::qname::last_segment(source)),
            ComputedKey::ConversionPrefix => CONVERSION_PREFIX_RE.find(source).map(|m| m.as_str()),
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
            "conversion_prefix(name)" => Ok(ComputedKey::ConversionPrefix),
            other => Err(UnknownComputedKey(other.to_string())),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnknownComputedKey(pub String);

impl std::fmt::Display for UnknownComputedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown computed key `{}` — allowed: last_segment(qname), conversion_prefix(name)",
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

#[derive(Debug)]
pub enum IndexSpecLoadError {
    Io(std::io::Error),
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
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn from_path(path: &Path) -> Result<Self, IndexSpecLoadError> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::from_toml_str(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty()),
            Err(e) => Err(IndexSpecLoadError::Io(e)),
        }
    }

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

    #[test]
    fn evaluate_dispatches_last_segment_to_cfdb_core_helper() {
        let inputs = ["foo::bar::baz", "foo", "", "cfdb_core::qname::last_segment"];
        for q in inputs {
            assert_eq!(
                ComputedKey::LastSegment.evaluate(q),
                Some(cfdb_core::qname::last_segment(q)),
                "dispatch ≠ canonical helper for input {q:?}",
            );
        }
    }

    #[test]
    fn conversion_prefix_round_trips_as_canonical_string() {
        assert_eq!(
            ComputedKey::ConversionPrefix.as_str(),
            "conversion_prefix(name)"
        );
        assert_eq!(
            "conversion_prefix(name)"
                .parse::<ComputedKey>()
                .expect("parse"),
            ComputedKey::ConversionPrefix
        );
    }

    #[test]
    fn source_prop_names_the_read_property_per_key() {
        assert_eq!(ComputedKey::LastSegment.source_prop(), "qname");
        assert_eq!(ComputedKey::ConversionPrefix.source_prop(), "name");
    }

    #[test]
    fn conversion_prefix_pattern_matches_cypher_literal_decoded_form() {
        assert_eq!(CONVERSION_PREFIX_PATTERN, r"^(\w+)_(?:from|to|for|as)_");
    }

    #[test]
    fn conversion_prefix_evaluate_returns_whole_match_or_none() {
        assert_eq!(
            ComputedKey::ConversionPrefix.evaluate("compute_0_from_bps"),
            Some("compute_0_from_")
        );
        assert_eq!(
            ComputedKey::ConversionPrefix.evaluate("compute_0_from_pct"),
            Some("compute_0_from_"),
            "both fork partners must bucket under the same prefix"
        );
        assert_eq!(
            ComputedKey::ConversionPrefix.evaluate("qty_to_notional"),
            Some("qty_to_")
        );
        assert_eq!(ComputedKey::ConversionPrefix.evaluate("uniq_42"), None);
        assert_eq!(ComputedKey::ConversionPrefix.evaluate("DupStruct"), None);
        assert_eq!(ComputedKey::ConversionPrefix.evaluate(""), None);
    }
}
