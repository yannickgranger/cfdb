//! Gate 3 spike — petgraph-baseline.
//!
//! Implements the 3 Gate 3 queries as direct Rust code against `petgraph::StableDiGraph`.
//! This spike validates the claim in the petgraph-baseline Gate 1 writeup: the 9-row grid
//! is trivially implementable as a builder API; the only real cost is the Cypher-subset
//! parser (2-3 weeks), which is explicitly OUT OF SCOPE for this spike. Agents and skills
//! can compose against the builder API directly without a query-language layer — so if
//! this spike's latencies beat a live candidate's, petgraph-baseline is a genuine option
//! for v0.1, not just a fallback.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
struct Fixture {
    nodes: Vec<FixtureNode>,
    edges: Vec<FixtureEdge>,
}

#[derive(Debug, Deserialize)]
struct FixtureNode {
    id: String,
    label: String,
    props: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct FixtureEdge {
    src: String,
    dst: String,
    label: String,
    #[serde(default)]
    props: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug)]
struct Node {
    id: String,
    label: String,
    props: BTreeMap<String, PropValue>,
}

#[derive(Clone, Debug)]
struct Edge {
    label: String,
    #[allow(dead_code)]
    props: BTreeMap<String, PropValue>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PropValue {
    S(String),
    I(i64),
    B(bool),
    Null,
}

impl PropValue {
    fn as_str(&self) -> Option<&str> {
        match self {
            PropValue::S(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl From<&serde_json::Value> for PropValue {
    fn from(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::String(s) => PropValue::S(s.clone()),
            serde_json::Value::Number(n) if n.is_i64() => PropValue::I(n.as_i64().unwrap()),
            serde_json::Value::Bool(b) => PropValue::B(*b),
            serde_json::Value::Null => PropValue::Null,
            _ => PropValue::Null,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let which = std::env::args().nth(1).unwrap_or_else(|| "small".to_string());
    let fixture_path = match which.as_str() {
        "small" => "../fixture-small.json",
        "large" => "../fixture-large.json",
        other => return Err(format!("unknown fixture: {other}").into()),
    };

    println!("== petgraph-baseline Gate 3 spike ==");
    println!("fixture: {}", fixture_path);

    let t0 = Instant::now();
    let fixture = load_fixture(fixture_path)?;
    println!(
        "load fixture: {} nodes, {} edges in {:.2?}",
        fixture.nodes.len(),
        fixture.edges.len(),
        t0.elapsed()
    );

    let t1 = Instant::now();
    let (graph, id_to_idx) = build_graph(&fixture);
    println!(
        "build graph: {} nodes, {} edges in {:.2?}",
        graph.node_count(),
        graph.edge_count(),
        t1.elapsed()
    );

    // F1a — Cartesian + regex extract
    let t4a = Instant::now();
    let f1a = query_f1a_cartesian_regex(&graph);
    println!("F1a (Cartesian + regex extract): {} results in {:.2?}", f1a, t4a.elapsed());

    // F1b — aggregation / group-by
    let t4b = Instant::now();
    let f1b = query_f1b_aggregation(&graph);
    println!("F1b (aggregation / group by base name): {} results in {:.2?}", f1b, t4b.elapsed());

    // F2 — variable-length path via BFS with depth cutoff
    let t5 = Instant::now();
    let f2 = query_f2(&graph);
    println!("F2 (variable-length path): {} results in {:.2?}", f2, t5.elapsed());

    // F3 — property regex
    let t6 = Instant::now();
    let f3 = query_f3(&graph);
    println!("F3 (regex WHERE): {} results in {:.2?}", f3, t6.elapsed());

    // Determinism — canonical sorted dump × 2
    let _ = id_to_idx; // silences unused warning if we change queries later
    let t7 = Instant::now();
    let dump1 = canonical_dump(&graph);
    let sha1 = sha256_hex(&dump1);
    let dump2 = canonical_dump(&graph);
    let sha2 = sha256_hex(&dump2);
    println!("canonical dump sha256 (run1): {}", sha1);
    println!("canonical dump sha256 (run2): {}", sha2);
    println!("determinism check: {:.2?}", t7.elapsed());
    if sha1 != sha2 {
        return Err("determinism failed".into());
    }

    println!("== SPIKE OK ==");
    Ok(())
}

fn load_fixture(path: impl AsRef<Path>) -> Result<Fixture, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn build_graph(fx: &Fixture) -> (StableDiGraph<Node, Edge>, BTreeMap<String, NodeIndex>) {
    let mut g: StableDiGraph<Node, Edge> = StableDiGraph::with_capacity(fx.nodes.len(), fx.edges.len());
    let mut id_to_idx: BTreeMap<String, NodeIndex> = BTreeMap::new();

    for n in &fx.nodes {
        let mut props: BTreeMap<String, PropValue> = BTreeMap::new();
        for (k, v) in &n.props {
            props.insert(k.clone(), PropValue::from(v));
        }
        let idx = g.add_node(Node {
            id: n.id.clone(),
            label: n.label.clone(),
            props,
        });
        id_to_idx.insert(n.id.clone(), idx);
    }

    for e in &fx.edges {
        let src_idx = match id_to_idx.get(&e.src) {
            Some(i) => *i,
            None => continue,
        };
        let dst_idx = match id_to_idx.get(&e.dst) {
            Some(i) => *i,
            None => continue,
        };
        let mut props: BTreeMap<String, PropValue> = BTreeMap::new();
        for (k, v) in &e.props {
            props.insert(k.clone(), PropValue::from(v));
        }
        g.add_edge(src_idx, dst_idx, Edge {
            label: e.label.clone(),
            props,
        });
    }

    (g, id_to_idx)
}

fn last_segment(qname: &str) -> &str {
    match qname.rfind(':') {
        Some(idx) => &qname[idx + 1..],
        None => qname,
    }
}

fn query_f1a_cartesian_regex(g: &StableDiGraph<Node, Edge>) -> usize {
    // O(n²) — deliberately the same shape as the LadybugDB F1a to compare planner cost.
    // petgraph has no planner at all; this is a literal nested loop.
    let items: Vec<&Node> = g
        .node_weights()
        .filter(|n| n.label == "Item")
        .collect();

    let mut count = 0usize;
    for i in 0..items.len() {
        for j in 0..items.len() {
            if i == j {
                continue;
            }
            let a = items[i];
            let b = items[j];
            let a_name = a.props.get("qname").and_then(PropValue::as_str).unwrap_or("");
            let b_name = b.props.get("qname").and_then(PropValue::as_str).unwrap_or("");
            let a_crate = a.props.get("crate").and_then(PropValue::as_str).unwrap_or("");
            let b_crate = b.props.get("crate").and_then(PropValue::as_str).unwrap_or("");
            if last_segment(a_name) == last_segment(b_name) && a_crate != b_crate {
                count += 1;
            }
        }
    }
    count
}

fn query_f1b_aggregation(g: &StableDiGraph<Node, Edge>) -> usize {
    // Group by base name, count groups where more than one distinct crate appears.
    // O(n) single pass with hash group-by. Idiomatic Rust of what Cypher's WITH+collect does.
    use std::collections::BTreeSet;
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for n in g.node_weights() {
        if n.label != "Item" {
            continue;
        }
        let qname = n.props.get("qname").and_then(PropValue::as_str).unwrap_or("");
        let krate = n.props.get("crate").and_then(PropValue::as_str).unwrap_or("").to_string();
        let base = last_segment(qname).to_string();
        groups.entry(base).or_default().insert(krate);
    }
    groups.values().filter(|crates| crates.len() > 1).count()
}

fn query_f2(g: &StableDiGraph<Node, Edge>) -> usize {
    // Variable-length reachability via BFS with depth cutoff. For each CallSite node,
    // walk outgoing CALLS edges up to 5 hops, collect reached Items. This is the literal
    // Pattern I / Pattern B shape. Result is the count of (CallSite, reachable Item) pairs.
    use std::collections::VecDeque;

    let call_sites: Vec<NodeIndex> = g
        .node_indices()
        .filter(|idx| g[*idx].label == "CallSite")
        .collect();

    let mut total_reached = 0usize;
    for start in call_sites {
        let mut visited: std::collections::HashSet<NodeIndex> = std::collections::HashSet::new();
        let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();
        queue.push_back((start, 0));
        while let Some((idx, depth)) = queue.pop_front() {
            if depth > 0 && g[idx].label == "Item" {
                total_reached += 1;
            }
            if depth >= 5 {
                continue;
            }
            for edge in g.edges(idx) {
                if edge.weight().label != "CALLS" {
                    continue;
                }
                let target = edge.target();
                if visited.insert(target) {
                    queue.push_back((target, depth + 1));
                }
            }
        }
    }
    total_reached
}

fn query_f3(g: &StableDiGraph<Node, Edge>) -> usize {
    // Forbidden-fn enforcement — find Items whose qname matches `now_utc`.
    let re = Regex::new(".*now_utc.*").unwrap();
    g.node_weights()
        .filter(|n| n.label == "Item")
        .filter(|n| {
            n.props
                .get("qname")
                .and_then(PropValue::as_str)
                .map(|s| re.is_match(s))
                .unwrap_or(false)
        })
        .count()
}

fn canonical_dump(g: &StableDiGraph<Node, Edge>) -> String {
    // Sorted nodes first (by id), then edges (by src id → dst id → label).
    let mut node_ids: Vec<&String> = g.node_weights().map(|n| &n.id).collect();
    node_ids.sort();

    let mut lines: Vec<String> = Vec::with_capacity(g.node_count() + g.edge_count());
    for n in g.node_weights().collect::<Vec<_>>() {
        lines.push(format!("node:{}:{}", n.label, n.id));
    }
    lines.sort();

    let mut edge_lines: Vec<String> = Vec::with_capacity(g.edge_count());
    for edge in g.edge_references() {
        let src = &g[edge.source()].id;
        let dst = &g[edge.target()].id;
        let lbl = &edge.weight().label;
        edge_lines.push(format!("edge:{}:{}->{}", lbl, src, dst));
    }
    edge_lines.sort();

    lines.extend(edge_lines);
    lines.join("\n")
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}
