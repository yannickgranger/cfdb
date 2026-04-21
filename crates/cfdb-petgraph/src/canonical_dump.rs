//! Canonical-dump helpers for `PetgraphStore::canonical_dump` (RFC §12.1).
//!
//! The sort-then-join machinery that produces a byte-stable JSONL dump of a
//! `KeyspaceState` lives here so the `impl StoreBackend for PetgraphStore`
//! block in `lib.rs` can stay small. The `pub(crate)` entry point is
//! `canonical_dump`; the four envelope/serde helpers are module-private.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use petgraph::visit::IntoEdgeReferences;
use serde_json::Value;

use crate::graph::KeyspaceState;

/// Produce the canonical sorted-JSONL dump of a keyspace per RFC §12.1.
///
/// **Format contract (issue #3630):**
/// - One pure JSON object per line, LF-separated, NO trailing newline
/// - Every key at every nesting level is emitted in alphabetical order
///   (achieved by building `BTreeMap<String, Value>` envelopes — direct
///   `serde_json::to_string(&Node)` would preserve struct declaration order
///   instead, which is NOT alphabetical for `id`/`label`/`props`)
/// - **Node lines** carry `{id, kind:"node", label, props}` — props elided
///   when empty
/// - **Edge lines** carry `{dst_qname, kind:"edge", label, props, src_qname}`
///   — props elided when empty. Raw src/dst node ids are NOT in the envelope;
///   `src_qname`/`dst_qname` ARE the canonical cross-reference form
/// - **Node sort key:** `(node.label, qname)` where `qname` is read from
///   `node.props["qname"]`; falls back to `node.id` when absent (CallSites,
///   Crates, Modules, Files have no qname prop)
/// - **Edge sort key:** `(edge.label, src_qname, dst_qname)` — both qnames
///   resolved via the id→qname lookup table (also id-fallback)
///
/// **Determinism (G1):** two consecutive calls on unchanged state return
/// byte-identical output. The id→qname lookup uses `BTreeMap` (not `HashMap`),
/// the sort uses stable `sort_by` (not `sort_unstable`), and there is no
/// wall-clock or system-time read.
///
/// **`println!` consumers (e.g. `cfdb dump` CLI):** `println!` appends a
/// trailing `\n` to the function output. Two-run determinism is preserved
/// because the trailing newline is consistent across runs — but downstream
/// consumers comparing raw `canonical_dump` output to file bytes must
/// account for the difference.
pub(crate) fn canonical_dump(state: &KeyspaceState) -> String {
    // Step 1: build id → qname-or-id lookup. BTreeMap (not HashMap) for G1.
    let mut id_to_qname: BTreeMap<&str, &str> = BTreeMap::new();
    for node in state.graph.node_weights() {
        let qname = node
            .props
            .get("qname")
            .and_then(PropValue::as_str)
            .unwrap_or(node.id.as_str());
        id_to_qname.insert(node.id.as_str(), qname);
    }

    // Step 2: collect node lines paired with their sort key.
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
    // Stable sort by (label, qname) — never sort_unstable per §12.1 G1 rule.
    node_lines.sort_by(|a, b| a.0.cmp(&b.0));

    // Step 3: collect edge lines paired with their sort key.
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

    // Step 4: join. Nodes first, then edges. LF separator. NO trailing LF.
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

/// Convert a `PropValue` to its canonical `serde_json::Value` form. The
/// `PropValue` enum is `#[serde(untagged)]` so each variant maps 1:1 to
/// the corresponding bare JSON value.
fn prop_value_to_json(p: &PropValue) -> Value {
    match p {
        PropValue::Str(s) => Value::String(s.clone()),
        PropValue::Int(n) => Value::Number((*n).into()),
        PropValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        PropValue::Bool(b) => Value::Bool(*b),
        PropValue::Null => Value::Null,
    }
}

/// Build the props sub-object — alphabetical via `BTreeMap` iteration.
fn props_to_json(props: &cfdb_core::fact::Props) -> Value {
    let map: BTreeMap<String, Value> = props
        .iter()
        .map(|(k, v)| (k.clone(), prop_value_to_json(v)))
        .collect();
    serde_json::to_value(map).expect("props envelope serializes")
}

/// Serialize a node as a single-line canonical JSON object.
/// Field order is alphabetical at every level via `BTreeMap`.
fn node_envelope_json(node: &Node) -> String {
    let mut env: BTreeMap<String, Value> = BTreeMap::new();
    env.insert("id".to_string(), Value::String(node.id.clone()));
    env.insert("kind".to_string(), Value::String("node".to_string()));
    env.insert(
        "label".to_string(),
        Value::String(node.label.as_str().to_string()),
    );
    if !node.props.is_empty() {
        env.insert("props".to_string(), props_to_json(&node.props));
    }
    serde_json::to_string(&env).expect("node envelope serializes")
}

/// Serialize an edge as a single-line canonical JSON object.
/// `src_qname`/`dst_qname` are the resolved cross-reference form
/// (with id-fallback applied by the caller). Raw src/dst ids are NOT
/// emitted — qnames are the canonical cross-reference form per §12.1.
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
    if !edge.props.is_empty() {
        env.insert("props".to_string(), props_to_json(&edge.props));
    }
    env.insert(
        "src_qname".to_string(),
        Value::String(src_qname.to_string()),
    );
    serde_json::to_string(&env).expect("edge envelope serializes")
}
