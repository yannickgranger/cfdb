use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{EdgeLabel, Label};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum PropValue {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

impl PropValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PropValue::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, PropValue::Null)
    }

    pub fn from_json(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::String(s) => PropValue::Str(s.clone()),
            serde_json::Value::Number(n) if n.is_i64() => {
                PropValue::Int(n.as_i64().expect("match arm guard proved n.is_i64()"))
            }
            serde_json::Value::Number(n) => match n.as_f64() {
                Some(f) => PropValue::Float(f),
                None => PropValue::Null,
            },
            serde_json::Value::Bool(b) => PropValue::Bool(*b),
            serde_json::Value::Null => PropValue::Null,
            _ => PropValue::Null,
        }
    }
}

impl From<&str> for PropValue {
    fn from(s: &str) -> Self {
        PropValue::Str(s.to_string())
    }
}

impl From<String> for PropValue {
    fn from(s: String) -> Self {
        PropValue::Str(s)
    }
}

impl From<i64> for PropValue {
    fn from(n: i64) -> Self {
        PropValue::Int(n)
    }
}

impl From<bool> for PropValue {
    fn from(b: bool) -> Self {
        PropValue::Bool(b)
    }
}

pub type Props = BTreeMap<String, PropValue>;

pub const G1_EXCLUDED_ATTRS: &[&str] = &["test_coverage"];

pub fn is_g1_excluded(attr: &str) -> bool {
    G1_EXCLUDED_ATTRS.contains(&attr)
}

pub fn build_item_props_common(qname: &str, name: &str, kind: &str, crate_name: &str) -> Props {
    let mut props: Props = BTreeMap::new();
    props.insert("qname".to_string(), PropValue::Str(qname.to_string()));
    props.insert("name".to_string(), PropValue::Str(name.to_string()));
    props.insert("kind".to_string(), PropValue::Str(kind.to_string()));
    props.insert("crate".to_string(), PropValue::Str(crate_name.to_string()));
    props
}

pub fn build_item_props(qname: &str, kind: &str, crate_name: &str, bounded_context: &str) -> Props {
    let name = crate::qname::last_segment(qname);
    let mut props = build_item_props_common(qname, name, kind, crate_name);
    props.insert(
        "bounded_context".to_string(),
        PropValue::Str(bounded_context.to_string()),
    );
    props
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub label: Label,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: Props,
}

impl Node {
    pub fn new(id: impl Into<String>, label: Label) -> Self {
        Self {
            id: id.into(),
            label,
            props: Props::new(),
        }
    }

    pub fn with_prop(mut self, key: impl Into<String>, value: impl Into<PropValue>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }

    pub fn sort_key(&self) -> (&str, &str) {
        (self.label.as_str(), self.id.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub src: String,
    pub dst: String,
    pub label: EdgeLabel,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: Props,
}

impl Edge {
    pub fn new(src: impl Into<String>, dst: impl Into<String>, label: EdgeLabel) -> Self {
        Self {
            src: src.into(),
            dst: dst.into(),
            label,
            props: Props::new(),
        }
    }

    pub fn with_prop(mut self, key: impl Into<String>, value: impl Into<PropValue>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }

    pub fn sort_key(&self) -> (&str, &str, &str) {
        (self.src.as_str(), self.dst.as_str(), self.label.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_builder() {
        let n = Node::new("item:foo::bar", Label::new(Label::ITEM))
            .with_prop("qname", "foo::bar")
            .with_prop("line", 42i64);
        assert_eq!(n.id, "item:foo::bar");
        assert_eq!(
            n.props.get("qname").and_then(PropValue::as_str),
            Some("foo::bar")
        );
        assert_eq!(n.props.get("line").and_then(PropValue::as_i64), Some(42));
    }

    #[test]
    fn edge_bag_semantics_via_props() {
        let e1 = Edge::new("cs:1", "item:foo", EdgeLabel::new(EdgeLabel::CALLS))
            .with_prop("line", 10i64);
        let e2 = Edge::new("cs:2", "item:foo", EdgeLabel::new(EdgeLabel::CALLS))
            .with_prop("line", 20i64);
        assert_ne!(e1, e2);
    }

    #[test]
    fn prop_value_round_trips_every_variant() {
        for v in [
            PropValue::Str("hello".into()),
            PropValue::Int(42),
            PropValue::Float(0.75),
            PropValue::Bool(true),
            PropValue::Null,
        ] {
            let json = serde_json::to_string(&v)
                .expect("PropValue is an untagged enum of primitive JSON types");
            let back: PropValue =
                serde_json::from_str(&json).expect("round-trip of just-serialized PropValue");
            assert_eq!(v, back);
        }
    }

    #[test]
    fn prop_value_untagged_serializes_as_bare_json() {
        assert_eq!(
            serde_json::to_string(&PropValue::Str("x".into()))
                .expect("PropValue::Str wraps String, infallible to JSON"),
            "\"x\""
        );
        assert_eq!(
            serde_json::to_string(&PropValue::Int(7))
                .expect("PropValue::Int wraps i64, infallible to JSON"),
            "7"
        );
        assert_eq!(
            serde_json::to_string(&PropValue::Bool(false))
                .expect("PropValue::Bool wraps bool, infallible to JSON"),
            "false"
        );
        assert_eq!(
            serde_json::to_string(&PropValue::Null)
                .expect("PropValue::Null serializes as JSON null"),
            "null"
        );
    }

    #[test]
    fn node_round_trips_with_props() {
        let n = Node::new("item:foo::bar", Label::new(Label::ITEM))
            .with_prop("qname", "foo::bar")
            .with_prop("line", 42i64)
            .with_prop("is_test", false);
        let json = serde_json::to_string(&n)
            .expect("Node has derived Serialize over String/Label/BTreeMap");
        let back: Node = serde_json::from_str(&json).expect("round-trip of just-serialized Node");
        assert_eq!(n, back);
    }

    #[test]
    fn node_round_trips_without_props() {
        let n = Node::new("crate:qbot-core", Label::new(Label::CRATE));
        let json = serde_json::to_string(&n)
            .expect("Node has derived Serialize over String/Label/BTreeMap");
        assert!(!json.contains("props"), "empty props should be elided");
        let back: Node = serde_json::from_str(&json).expect("round-trip of just-serialized Node");
        assert_eq!(n, back);
    }

    #[test]
    fn edge_round_trips_with_props() {
        let e = Edge::new(
            "cs:abcdef",
            "item:foo::bar",
            EdgeLabel::new(EdgeLabel::INVOKES_AT),
        )
        .with_prop("arg_index", 2i64);
        let json = serde_json::to_string(&e)
            .expect("Edge has derived Serialize over String/EdgeLabel/BTreeMap");
        let back: Edge = serde_json::from_str(&json).expect("round-trip of just-serialized Edge");
        assert_eq!(e, back);
    }

    #[test]
    fn build_item_props_common_is_exactly_the_four_subset() {
        let props = build_item_props_common("a::b::Foo", "Foo", "struct", "mycrate");
        assert_eq!(
            props.get("qname").and_then(PropValue::as_str),
            Some("a::b::Foo")
        );
        assert_eq!(props.get("name").and_then(PropValue::as_str), Some("Foo"));
        assert_eq!(
            props.get("kind").and_then(PropValue::as_str),
            Some("struct")
        );
        assert_eq!(
            props.get("crate").and_then(PropValue::as_str),
            Some("mycrate")
        );
        let mut keys: Vec<&str> = props.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["crate", "kind", "name", "qname"]);
    }

    #[test]
    fn build_item_props_common_takes_name_verbatim_not_derived_from_qname() {
        let props = build_item_props_common(
            "m::Foo::impl_Display",
            "impl Display for Foo",
            "impl_block",
            "mycrate",
        );
        assert_eq!(
            props.get("name").and_then(PropValue::as_str),
            Some("impl Display for Foo")
        );
    }

    #[test]
    fn build_item_props_extends_common_with_bounded_context() {
        let props = build_item_props("a::b::Foo", "struct", "mycrate", "b_context");
        let common = build_item_props_common("a::b::Foo", "Foo", "struct", "mycrate");
        for (k, v) in &common {
            assert_eq!(props.get(k), Some(v), "5-key must contain 4-subset key {k}");
        }
        assert_eq!(
            props.get("bounded_context").and_then(PropValue::as_str),
            Some("b_context")
        );
        assert_eq!(props.len(), 5);
    }

    #[test]
    fn from_json_number_never_fabricates_a_zero() {
        use serde_json::Value;
        assert_eq!(
            PropValue::from_json(&Value::from(42_i64)),
            PropValue::Int(42)
        );
        assert_eq!(
            PropValue::from_json(&Value::from(-5_i64)),
            PropValue::Int(-5)
        );
        let big = PropValue::from_json(&Value::from(u64::MAX));
        assert_eq!(big, PropValue::Float(u64::MAX as f64));
        assert_ne!(big, PropValue::Float(0.0));
        assert_eq!(
            PropValue::from_json(&Value::from(2.5_f64)),
            PropValue::Float(2.5)
        );
        assert_eq!(
            PropValue::from_json(&Value::from(true)),
            PropValue::Bool(true)
        );
        assert_eq!(
            PropValue::from_json(&Value::from("s")),
            PropValue::Str("s".to_string())
        );
        assert!(PropValue::from_json(&Value::Null).is_null());
        assert!(PropValue::from_json(&Value::Array(vec![])).is_null());
        assert!(PropValue::from_json(&Value::Object(Default::default())).is_null());
    }
}
