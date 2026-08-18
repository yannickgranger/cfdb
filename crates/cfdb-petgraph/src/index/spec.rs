//! `IndexSpec` — parsed `.cfdb/indexes.toml`.
//!
//! The spec names which `(Label, prop)` pairs are indexed at ingest time.
//! Each entry carries a required `notes` string documenting the rationale.
//!
//! The loader: [`IndexSpec::from_path`] does I/O, [`IndexSpec::from_toml_str`]
//! is a pure parser usable from unit tests without touching the filesystem.
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
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Parsed `.cfdb/indexes.toml` content. Owns a `Vec<IndexEntry>` in
/// TOML document order so order is stable across runs on identical inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexSpec {
    /// One `[[index]]` array entry per indexed `(Label, key)` pair.
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
/// resolution). Each variant is anchored to a single canonical
/// definition of its formula so the computed key cannot drift from the
/// contract it realises (RFC-035 §3.3 / R1 B3 resolution): `LastSegment`
/// anchors to `cfdb_core::qname::last_segment`; `ConversionPrefix`
/// anchors to the vetted [`CONVERSION_PREFIX_PATTERN`] regex, which the
/// Cypher `regexp_extract` UDF and this index build both evaluate.
///
/// Adding a key is a reviewed PR referencing RFC-035 (§3.2 / §4
/// no-ratchet); the const-vs-trait decision (§3.4) is re-opened only if
/// the allowlist exceeds five keys. At two keys it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedKey {
    /// `last_segment(qname)` — splits the qname at the last `::` and
    /// returns the trailing segment. Semantic anchor:
    /// `cfdb_core::qname::last_segment`. Total: every qname has a last
    /// segment, so [`Self::evaluate`] always returns `Some`.
    LastSegment,
    /// `conversion_prefix(name)` — the leading `<stem>_<kw>_` conversion
    /// prefix of an item's `name`, extracted by the vetted
    /// [`CONVERSION_PREFIX_PATTERN`] regex. Join key for the
    /// RandomScattering classifier's fork detector
    /// (`examples/queries/classifier-random-scattering.cypher`), which
    /// equi-joins `regexp_extract(a.name, '<pattern>') =
    /// regexp_extract(b.name, '<pattern>')`. Unlike [`Self::LastSegment`]
    /// it reads `name` (not `qname`) and is PARTIAL — a name that does
    /// not carry a conversion prefix yields `None` from
    /// [`Self::evaluate`], contributing no posting (SQL NULL-index
    /// semantics).
    ConversionPrefix,
}

/// Vetted regex for [`ComputedKey::ConversionPrefix`]. It MUST equal the
/// `regexp_extract` pattern literal in
/// `examples/queries/classifier-random-scattering.cypher` BYTE-FOR-BYTE:
/// the cross-MATCH fast path (`index::lookup`) recognises the computed
/// join only when the Cypher call's pattern argument is exactly this
/// string. The Cypher lexer decodes the source
/// `'^(\\w+)_(?:from|to|for|as)_'` (double-backslash) to this
/// single-backslash form, so this raw literal is the decoded pattern the
/// AST carries. `pub` so the evaluator crate's test suite can pin the
/// equality against the parsed rule file.
pub const CONVERSION_PREFIX_PATTERN: &str = r"^(\w+)_(?:from|to|for|as)_";

/// Compiled [`CONVERSION_PREFIX_PATTERN`], built once. The extractor is
/// `regex::Regex::find` — the SAME engine + pattern the Cypher
/// `regexp_extract` UDF uses — so an index bucket key is byte-identical
/// to the query-time join value the WHERE clause compares.
static CONVERSION_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(CONVERSION_PREFIX_PATTERN).expect("vetted ConversionPrefix regex compiles")
});

impl ComputedKey {
    /// Canonical string form used on the TOML surface and as the
    /// virtual-prop tag for build-pass entries.
    pub fn as_str(self) -> &'static str {
        match self {
            ComputedKey::LastSegment => "last_segment(qname)",
            ComputedKey::ConversionPrefix => "conversion_prefix(name)",
        }
    }

    /// The `:Item` property this computed key reads. `LastSegment` reads
    /// `qname`; `ConversionPrefix` reads `name`. The build pass
    /// ([`crate::index::build::entry_value_for_node`]) fetches the raw
    /// source value through this, and the evaluator cross-MATCH fast
    /// path (`index::lookup`) uses it to confirm the recognised call
    /// reads the key's canonical prop — a call over any other prop is
    /// not this key and must not narrow through its posting list.
    pub fn source_prop(self) -> &'static str {
        match self {
            ComputedKey::LastSegment => "qname",
            ComputedKey::ConversionPrefix => "name",
        }
    }

    /// Compile-time dispatch from a computed key to its canonical
    /// formula (RFC-035 §3.3 invariant ownership + §3.4 closed-enum
    /// compile-time dispatch / R1 B3 + B4). `source` is the raw value of
    /// the [`Self::source_prop`] property.
    ///
    /// This is the single dispatch surface for index-build consumers
    /// and the evaluator fast-path consumer (slice 5/6). Inline
    /// switching on `ComputedKey` outside this method re-introduces the
    /// split-brain that §3.3's invariant-owner rule is designed to
    /// prevent — always route through `evaluate`.
    ///
    /// Returns `None` for a PARTIAL key whose formula does not apply to
    /// the input: `ConversionPrefix` over a `name` with no conversion
    /// prefix yields `None`, so the node contributes no posting and (at
    /// query time) the equi-join conjunct comparing it is unsatisfiable.
    /// `LastSegment` is total and always returns `Some`.
    ///
    /// `LastSegment` delegates to [`cfdb_core::qname::last_segment`], the
    /// canonical owner of the `last_segment` formula. `ConversionPrefix`
    /// returns the whole first match of [`CONVERSION_PREFIX_PATTERN`] —
    /// byte-identical to what the Cypher `regexp_extract(name,
    /// '<pattern>')` UDF returns for the same pattern, because both use
    /// the same `regex` engine (see [`CONVERSION_PREFIX_RE`]).
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

/// Error raised when a TOML `computed = "..."` string is not in the
/// allowlist (RFC-035 §3.4). Carries the offending string for error
/// messages.
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

    #[test]
    fn evaluate_dispatches_last_segment_to_cfdb_core_helper() {
        // The dispatch surface MUST agree byte-for-byte with the
        // canonical `cfdb_core::qname::last_segment` (RFC-035 §3.3).
        // This unit test pins the agreement on a representative input
        // set; the self-dogfood integration test extends it to every
        // `:Item` in cfdb's own extracted keyspace. `LastSegment` is
        // total, so the dispatch is `Some(<helper output>)`.
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
        // The Cypher lexer decodes `'^(\\w+)_(?:from|to|for|as)_'` (the
        // literal in classifier-random-scattering.cypher) to this
        // single-backslash form. Recognition of the fork equi-join is
        // byte-for-byte, so the const MUST equal it. If this fails, the
        // const drifted from the .cypher — re-vet both together.
        assert_eq!(CONVERSION_PREFIX_PATTERN, r"^(\w+)_(?:from|to|for|as)_");
    }

    #[test]
    fn conversion_prefix_evaluate_returns_whole_match_or_none() {
        // Whole first match, exactly as `regexp_extract` returns it —
        // includes the trailing conversion keyword + `_` (see the
        // pattern_b_vertical_split_brain scar: `stop_from_bps` →
        // `stop_from_`).
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
        // Names with no conversion prefix are PARTIAL → None → no
        // posting (SQL NULL-index semantics).
        assert_eq!(ComputedKey::ConversionPrefix.evaluate("uniq_42"), None);
        assert_eq!(ComputedKey::ConversionPrefix.evaluate("DupStruct"), None);
        assert_eq!(ComputedKey::ConversionPrefix.evaluate(""), None);
    }
}
