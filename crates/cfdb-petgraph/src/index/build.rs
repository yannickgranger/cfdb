use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;

use crate::index::spec::IndexEntry;

pub(crate) type IndexValue = String;

pub(crate) type IndexTag = String;

pub(crate) fn index_key_of(pv: &PropValue) -> Option<IndexValue> {
    match pv {
        PropValue::Str(s) => Some(s.clone()),
        PropValue::Int(n) => Some(n.to_string()),
        PropValue::Bool(b) => Some(b.to_string()),
        PropValue::Float(_) | PropValue::Null => None,
        _ => None,
    }
}

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
        let n = item("a");
        assert_eq!(entry_value_for_node(&entry, &n), None);
    }
}
