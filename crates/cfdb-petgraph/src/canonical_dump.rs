use std::collections::BTreeMap;

use cfdb_core::fact::{is_g1_excluded, Edge, Node, PropValue};
use petgraph::visit::IntoEdgeReferences;
use serde_json::Value;

use crate::graph::KeyspaceState;

pub(crate) fn canonical_dump(state: &KeyspaceState) -> String {
    let mut id_to_qname: BTreeMap<&str, &str> = BTreeMap::new();
    for node in state.graph.node_weights() {
        let qname = node
            .props
            .get("qname")
            .and_then(PropValue::as_str)
            .unwrap_or(node.id.as_str());
        id_to_qname.insert(node.id.as_str(), qname);
    }

    let mut node_lines: Vec<((String, String), String)> =
        Vec::with_capacity(state.graph.node_count());
    for node in state.graph.node_weights() {
        let qname = id_to_qname
            .get(node.id.as_str())
            .copied()
            .unwrap_or(node.id.as_str())
            .to_string();
        let label = node.label.as_str().to_string();
        let json = node_envelope_json(node);
        node_lines.push(((label, qname), json));
    }
    node_lines.sort_by(|a, b| a.0.cmp(&b.0));

    let mut edge_lines: Vec<((String, String, String), String)> =
        Vec::with_capacity(state.graph.edge_count());
    for edge_ref in IntoEdgeReferences::edge_references(&state.graph) {
        let edge: &Edge = edge_ref.weight();
        let src_qname = id_to_qname
            .get(edge.src.as_str())
            .copied()
            .unwrap_or(edge.src.as_str())
            .to_string();
        let dst_qname = id_to_qname
            .get(edge.dst.as_str())
            .copied()
            .unwrap_or(edge.dst.as_str())
            .to_string();
        let label = edge.label.as_str().to_string();
        let json = edge_envelope_json(edge, &src_qname, &dst_qname);
        edge_lines.push(((label, src_qname, dst_qname), json));
    }
    edge_lines.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::new();
    for (_, json) in node_lines {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&json);
    }
    for (_, json) in edge_lines {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&json);
    }
    out
}

fn prop_value_to_json(p: &PropValue) -> Value {
    match p {
        PropValue::Str(s) => Value::String(s.clone()),
        PropValue::Int(n) => Value::Number((*n).into()),
        PropValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        PropValue::Bool(b) => Value::Bool(*b),
        PropValue::Null => Value::Null,
        _ => Value::String(format!("unsupported_propvalue:{:?}", p)),
    }
}

fn has_dumpable_props(props: &cfdb_core::fact::Props) -> bool {
    props.keys().any(|k| !is_g1_excluded(k))
}

fn props_to_json(props: &cfdb_core::fact::Props) -> Value {
    let map: BTreeMap<String, Value> = props
        .iter()
        .filter(|(k, _)| !is_g1_excluded(k))
        .map(|(k, v)| (k.clone(), prop_value_to_json(v)))
        .collect();
    serde_json::to_value(map).expect("props envelope serializes")
}

fn node_envelope_json(node: &Node) -> String {
    let mut env: BTreeMap<String, Value> = BTreeMap::new();
    env.insert("id".to_string(), Value::String(node.id.clone()));
    env.insert("kind".to_string(), Value::String("node".to_string()));
    env.insert(
        "label".to_string(),
        Value::String(node.label.as_str().to_string()),
    );
    if has_dumpable_props(&node.props) {
        env.insert("props".to_string(), props_to_json(&node.props));
    }
    serde_json::to_string(&env).expect("node envelope serializes")
}

fn edge_envelope_json(edge: &Edge, src_qname: &str, dst_qname: &str) -> String {
    let mut env: BTreeMap<String, Value> = BTreeMap::new();
    env.insert(
        "dst_qname".to_string(),
        Value::String(dst_qname.to_string()),
    );
    env.insert("kind".to_string(), Value::String("edge".to_string()));
    env.insert(
        "label".to_string(),
        Value::String(edge.label.as_str().to_string()),
    );
    if has_dumpable_props(&edge.props) {
        env.insert("props".to_string(), props_to_json(&edge.props));
    }
    env.insert(
        "src_qname".to_string(),
        Value::String(src_qname.to_string()),
    );
    serde_json::to_string(&env).expect("edge envelope serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::Node;
    use cfdb_core::schema::Label;

    #[test]
    fn g1_excluded_attr_absent_from_node_dump() {
        let node = Node::new("item:demo::f", Label::new(Label::ITEM))
            .with_prop("qname", "demo::f")
            .with_prop("test_coverage", "{\"lines\":42}")
            .with_prop("visibility", "pub");
        let json = node_envelope_json(&node);
        assert!(
            !json.contains("test_coverage"),
            "G1-excluded attr `test_coverage` must NOT appear in the canonical dump: {json}"
        );
        assert!(
            json.contains("visibility"),
            "non-excluded attr `visibility` MUST still appear in the canonical dump: {json}"
        );
    }

    #[test]
    fn populating_g1_excluded_attr_does_not_change_node_dump() {
        let base = Node::new("item:demo::f", Label::new(Label::ITEM)).with_prop("qname", "demo::f");
        let with_cov = base.clone().with_prop("test_coverage", "{\"lines\":42}");
        assert_eq!(
            node_envelope_json(&base),
            node_envelope_json(&with_cov),
            "populating a G1-excluded attr must not change the canonical (G1) dump bytes"
        );
    }

    #[test]
    fn node_with_only_g1_excluded_props_elides_props_key() {
        let node = Node::new("item:demo::g", Label::new(Label::ITEM))
            .with_prop("test_coverage", "{\"lines\":1}");
        let json = node_envelope_json(&node);
        assert!(
            !json.contains("\"props\""),
            "a node whose only attr is G1-excluded must omit the props key: {json}"
        );
    }
}
