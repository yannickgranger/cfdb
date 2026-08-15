//! Build-pass helpers for `KeyspaceState::by_prop`.
//!
//! Pure functions that turn an [`IndexEntry`] + a [`Node`] into the
//! `(tag, value)` pair that gets inserted into `by_prop`. Keeping these
//! pure and free of `KeyspaceState` state lets the build pass and the
//! re-ingest maintenance path share one code path — and lets unit tests
//! exercise edge cases without wiring a full graph.
//!
//! # Computed-key dispatch
//!
//! Computed keys (`IndexEntry::Computed { computed, ... }`) read the
//! key's `source_prop` (`qname` for `LastSegment`, `name` for
//! `ConversionPrefix`) and dispatch through
//! [`ComputedKey::evaluate`](crate::index::spec::ComputedKey::evaluate),
//! the single canonical formula surface. `LastSegment` routes to
//! [`cfdb_core::qname::last_segment`] for qname structure;
//! `ConversionPrefix` routes to the vetted `CONVERSION_PREFIX_PATTERN`
//! regex. `evaluate` is PARTIAL: `ConversionPrefix` over a `name` with no
//! conversion prefix returns `None`, and that node contributes NO posting
//! (SQL NULL-index semantics) — a scatter fork only buckets items whose
//! name actually carries a conversion prefix.

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;

use crate::index::spec::IndexEntry;

/// The inner-key type stored in `by_prop`. v0.1 indexes only
/// round-trip-stable scalar values — [`PropValue::Str`], [`PropValue::Int`],
/// [`PropValue::Bool`]. `Float` and `Null` produce `None` from
/// [`index_key_of`] and are therefore not indexed; this matches
/// standard SQL semantics (NULL excluded from non-null indexes) and
/// sidesteps the non-`Ord` / non-`Eq` nature of `f64`.
pub(crate) type IndexValue = String;

/// Tag distinguishing a prop entry from a computed-key entry inside a
/// `(Label, …)` pair. Stored as a `String` because the v0.1 computed-key
/// allowlist canonical strings (`"last_segment(qname)"`) are disjoint
/// from the real prop names currently in use (`qname`, `bounded_context`,
/// `name`, …). A future RFC that closes that gap — if one ever opens —
/// ships with a real enum tag.
pub(crate) type IndexTag = String;

/// Canonical string for a [`PropValue`] when used as a posting-list key.
/// Returns `None` for `Float` / `Null` / other unsupported shapes —
/// callers treat a missing value the same as "prop absent on node":
/// no entry is added to `by_prop`.
pub(crate) fn index_key_of(pv: &PropValue) -> Option<IndexValue> {
    match pv {
        PropValue::Str(s) => Some(s.clone()),
        PropValue::Int(n) => Some(n.to_string()),
        PropValue::Bool(b) => Some(b.to_string()),
        PropValue::Float(_) | PropValue::Null => None,
        // Future unknown variants are treated as not-indexable (same as
        // Float/Null) so the posting list stays correct under variant
        // addition; the variant's existence in fact data will still appear
        // via canonical_dump.
        _ => None,
    }
}

/// Compute the `(tag, value)` to insert into `by_prop` for a single
/// `(IndexEntry, Node)` pair. Returns `None` when:
///
/// - The entry's `label` does not match the node's label.
/// - The named prop is absent on the node.
/// - The prop value is not indexable (`Float`, `Null`).
///
/// The caller uses a `None` result to mean "no index entry for this
/// (spec entry, node) pair" — it is not an error.
pub(crate) fn entry_value_for_node(
    entry: &IndexEntry,
    node: &Node,
) -> Option<(Label, IndexTag, IndexValue)> {
    match entry {
        IndexEntry::Prop { label, prop, .. } => {
            let label = Label::new(label.as_str());
            if node.label != label {
                return None;
            }
            let value = index_key_of(node.props.get(prop)?)?;
            Some((label, prop.clone(), value))
        }
        IndexEntry::Computed {
            label, computed, ..
        } => {
            let label = Label::new(label.as_str());
            if node.label != label {
                return None;
            }
            // Read the key's canonical source prop (`qname` for
            // LastSegment, `name` for ConversionPrefix), then apply the
            // formula. `evaluate` is PARTIAL — a `None` result (a name
            // with no conversion prefix) means this node contributes no
            // posting under the computed tag.
            let raw = node.props.get(computed.source_prop())?;
            let source = raw.as_str()?;
            let derived = computed.evaluate(source)?.to_string();
            Some((label, computed.as_str().to_string(), derived))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::spec::{ComputedKey, IndexEntry};
    use cfdb_core::fact::Node;
    use cfdb_core::schema::Label;

    fn item(id: &str) -> Node {
        Node::new(id, Label::new("Item"))
    }

    #[test]
    fn index_key_of_accepts_scalar_shapes() {
        assert_eq!(
            index_key_of(&PropValue::from("foo")).as_deref(),
            Some("foo")
        );
        assert_eq!(index_key_of(&PropValue::from(42i64)).as_deref(), Some("42"));
        assert_eq!(
            index_key_of(&PropValue::from(true)).as_deref(),
            Some("true")
        );
    }

    #[test]
    fn index_key_of_rejects_float_and_null() {
        // Value chosen to avoid clippy::approx_constant (not 3.14 / 2.71).
        assert_eq!(index_key_of(&PropValue::Float(1.5_f64)), None);
        assert_eq!(index_key_of(&PropValue::Null), None);
    }

    #[test]
    fn entry_value_for_node_skips_label_mismatch() {
        let entry = IndexEntry::Prop {
            label: "Item".into(),
            prop: "qname".into(),
            notes: "test".into(),
        };
        let n = Node::new("a", Label::new("CallSite")).with_prop("qname", "foo");
        assert_eq!(entry_value_for_node(&entry, &n), None);
    }

    #[test]
    fn entry_value_for_node_prop_returns_tag_and_value() {
        let entry = IndexEntry::Prop {
            label: "Item".into(),
            prop: "qname".into(),
            notes: "test".into(),
        };
        let n = item("a").with_prop("qname", "foo::bar");
        let (label, tag, value) = entry_value_for_node(&entry, &n).expect("matched");
        assert_eq!(label.as_str(), "Item");
        assert_eq!(tag, "qname");
        assert_eq!(value, "foo::bar");
    }

    #[test]
    fn entry_value_for_node_computed_evaluates_last_segment() {
        let entry = IndexEntry::Computed {
            label: "Item".into(),
            computed: ComputedKey::LastSegment,
            notes: "test".into(),
        };
        let n = item("a").with_prop("qname", "foo::bar::baz");
        let (label, tag, value) = entry_value_for_node(&entry, &n).expect("matched");
        assert_eq!(label.as_str(), "Item");
        assert_eq!(tag, "last_segment(qname)");
        assert_eq!(value, "baz");
    }

    #[test]
    fn entry_value_for_node_computed_evaluates_conversion_prefix_from_name() {
        // ConversionPrefix reads `name` (its source_prop), NOT `qname`,
        // and buckets under the whole regex match.
        let entry = IndexEntry::Computed {
            label: "Item".into(),
            computed: ComputedKey::ConversionPrefix,
            notes: "test".into(),
        };
        let n = item("a")
            .with_prop("qname", "crate_a::infra::compute_0_from_bps")
            .with_prop("name", "compute_0_from_bps");
        let (label, tag, value) = entry_value_for_node(&entry, &n).expect("matched");
        assert_eq!(label.as_str(), "Item");
        assert_eq!(tag, "conversion_prefix(name)");
        assert_eq!(value, "compute_0_from_");
    }

    #[test]
    fn entry_value_for_node_computed_conversion_prefix_non_match_contributes_nothing() {
        // A name with no conversion prefix → evaluate None → no posting.
        let entry = IndexEntry::Computed {
            label: "Item".into(),
            computed: ComputedKey::ConversionPrefix,
            notes: "test".into(),
        };
        let n = item("a")
            .with_prop("qname", "crate_a::infra::uniq_5")
            .with_prop("name", "uniq_5");
        assert_eq!(entry_value_for_node(&entry, &n), None);
    }

    #[test]
    fn entry_value_for_node_computed_conversion_prefix_absent_name_contributes_nothing() {
        // No `name` prop at all → None (source prop absent).
        let entry = IndexEntry::Computed {
            label: "Item".into(),
            computed: ComputedKey::ConversionPrefix,
            notes: "test".into(),
        };
        let n = item("a").with_prop("qname", "crate_a::infra::thing");
        assert_eq!(entry_value_for_node(&entry, &n), None);
    }

    #[test]
    fn entry_value_for_node_returns_none_when_prop_absent() {
        let entry = IndexEntry::Prop {
            label: "Item".into(),
            prop: "qname".into(),
            notes: "test".into(),
        };
        let n = item("a"); // no qname
        assert_eq!(entry_value_for_node(&entry, &n), None);
    }
}
