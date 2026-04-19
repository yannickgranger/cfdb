//! Fact types — Node, Edge, PropValue.
//!
//! These are the units the extractor emits and the store persists. The JSONL
//! canonical-dump format (RFC §12.1) is obtained by serializing these types
//! with `serde_json` and sorting by the deterministic key `(label, id)` for
//! nodes, `(src, dst, label)` for edges.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{EdgeLabel, Label};

/// A typed property value. Intentionally a closed set — string / int / float /
/// bool / null — so that property equality has canonical semantics and so that
/// the JSONL dump is byte-stable across implementations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
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

    /// Canonical JSON → scalar PropValue coercion. Arrays and objects are
    /// NOT valid scalar params — callers feeding untrusted input (e.g. the
    /// `cfdb query --params <json>` CLI) must reject non-scalar shapes at
    /// the boundary BEFORE calling this, so they can emit a clear error.
    /// For trusted scalar input (test fixtures, already-validated CLI
    /// params), this is a total function: non-scalar JSON collapses to
    /// `PropValue::Null` rather than panicking.
    pub fn from_json(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::String(s) => PropValue::Str(s.clone()),
            serde_json::Value::Number(n) if n.is_i64() => {
                PropValue::Int(n.as_i64().expect("match arm guard proved n.is_i64()"))
            }
            serde_json::Value::Number(n) => PropValue::Float(n.as_f64().unwrap_or(0.0)),
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

/// A map of property key → value. BTreeMap is intentional — it gives us a
/// canonical iteration order, which is load-bearing for G1 (byte-identical
/// canonical dumps across runs).
pub type Props = BTreeMap<String, PropValue>;

/// A single node fact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Stable identifier — globally unique within a keyspace. For Items this
    /// is typically the fully-qualified name plus a disambiguator; for
    /// CallSites it is a hash of `(file, line, col, in_fn)`.
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

    /// Sort key for canonical dump ordering (G1).
    pub fn sort_key(&self) -> (&str, &str) {
        (self.label.as_str(), self.id.as_str())
    }
}

/// A single edge fact. Edge identity is not stored — two edges with identical
/// (src, dst, label, props) are distinct by construction (bag semantics, S5).
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

    /// Sort key for canonical dump ordering (G1). Includes label so two edges
    /// between the same pair with different labels sort stably.
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
        // Different src → different facts even though dst/label match.
        assert_ne!(e1, e2);
    }

    // ---- Serde round-trip tests (#3625 AC) ---------------------------------

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
        // #[serde(untagged)] writes bare JSON values, not tagged enum variants.
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
        // Empty props must be elided from the wire form (skip_serializing_if)
        // AND must parse back to an empty BTreeMap, not a missing field error.
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
}
