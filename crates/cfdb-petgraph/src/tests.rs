use cfdb_core::fact::{Edge, Node};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::PetgraphStore;

fn ks() -> Keyspace {
    Keyspace::new("test")
}

fn item(id: &str, qname: &str, krate: &str) -> Node {
    Node::new(id, Label::new(Label::ITEM))
        .with_prop("qname", qname)
        .with_prop("crate", krate)
}

fn call_site(id: &str) -> Node {
    Node::new(id, Label::new(Label::CALL_SITE))
}

#[test]
fn ingest_round_trip_via_canonical_dump() {
    let mut store = PetgraphStore::new();
    let nodes = vec![
        item("item:a", "foo::bar", "c1"),
        item("item:b", "baz::bar", "c2"),
        call_site("cs:1"),
    ];
    let edges = vec![Edge::new(
        "cs:1",
        "item:a",
        EdgeLabel::new(EdgeLabel::CALLS),
    )];
    store
        .ingest_nodes(&ks(), nodes)
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(&ks(), edges)
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    assert!(dump.contains("item:a"));
    assert!(dump.contains("item:b"));
    assert!(dump.contains("cs:1"));
    assert!(dump.contains("CALLS"));
}

#[test]
fn canonical_dump_is_deterministic() {
    let mut store = PetgraphStore::new();
    let nodes: Vec<Node> = (0..20)
        .map(|i| item(&format!("item:n{}", i), &format!("mod::f{}", i), "c1"))
        .collect();
    let edges: Vec<Edge> = (0..19)
        .map(|i| {
            Edge::new(
                format!("item:n{}", i),
                format!("item:n{}", i + 1),
                EdgeLabel::new(EdgeLabel::CALLS),
            )
        })
        .collect();
    store
        .ingest_nodes(&ks(), nodes)
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(&ks(), edges)
        .expect("ingest into fresh in-memory store never fails");

    let d1 = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let d2 = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    assert_eq!(
        d1.as_bytes(),
        d2.as_bytes(),
        "G1: canonical dump must be byte-identical across calls"
    );
}

fn parse_dump_lines(dump: &str) -> Vec<(String, serde_json::Value)> {
    dump.lines()
        .map(|line| {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
                panic!("dump line is not pure JSON: {line:?}: {e}");
            });
            assert!(
                matches!(parsed, serde_json::Value::Object(_)),
                "dump line must be a JSON object, got: {line}"
            );
            (line.to_string(), parsed)
        })
        .collect()
}

#[test]
fn canonical_dump_lines_are_pure_jsonl() {
    let mut store = PetgraphStore::new();
    let nodes = vec![
        item("item:a", "foo::bar", "c1"),
        item("item:b", "baz::qux", "c2"),
    ];
    let edges = vec![Edge::new(
        "item:a",
        "item:b",
        EdgeLabel::new(EdgeLabel::CALLS),
    )];
    store
        .ingest_nodes(&ks(), nodes)
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(&ks(), edges)
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    for line in dump.lines() {
        assert!(
            !line.starts_with("N\t"),
            "line uses old tab-prefix format: {line}"
        );
        assert!(
            !line.starts_with("E\t"),
            "line uses old tab-prefix format: {line}"
        );
    }
    let parsed = parse_dump_lines(&dump);
    assert_eq!(parsed.len(), 3, "2 nodes + 1 edge = 3 lines");
}

#[test]
fn canonical_dump_kind_discriminator_present() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![item("item:a", "foo::a", "c1"), call_site("cs:1")],
        )
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(
            &ks(),
            vec![Edge::new(
                "cs:1",
                "item:a",
                EdgeLabel::new(EdgeLabel::CALLS),
            )],
        )
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let parsed = parse_dump_lines(&dump);

    let kinds: Vec<&str> = parsed
        .iter()
        .map(|(_, v)| v.get("kind").and_then(|k| k.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "node").count(),
        2,
        "expected 2 node lines: {kinds:?}"
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "edge").count(),
        1,
        "expected 1 edge line: {kinds:?}"
    );
}

#[test]
fn canonical_dump_field_order_is_alphabetical() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::bar", "c1")])
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let line = dump.lines().next().expect("at least one line");
    let keys = top_level_json_keys(line);
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "top-level JSON keys are not alphabetical: {keys:?} (line: {line})"
    );
}

fn top_level_json_keys(line: &str) -> Vec<String> {
    let parsed: serde_json::Value = serde_json::from_str(line)
        .expect("caller has already validated line is pure JSON via parse_dump_lines");
    let bytes = serde_json::to_string(&parsed)
        .expect("re-serializing a just-parsed serde_json::Value is infallible");
    let mut keys = Vec::new();
    let mut chars = bytes.chars().peekable();
    let mut depth = 0i32;
    while let Some(c) = chars.next() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            '"' if depth == 1 => {
                let mut k = String::new();
                while let Some(&ch) = chars.peek() {
                    chars.next();
                    if ch == '"' {
                        break;
                    }
                    k.push(ch);
                }
                while let Some(&ch) = chars.peek() {
                    if ch == ':' {
                        keys.push(k);
                        break;
                    } else if !ch.is_whitespace() {
                        break;
                    }
                    chars.next();
                }
            }
            _ => {}
        }
    }
    keys
}

#[test]
fn canonical_dump_node_sort_uses_label_then_qname() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![
                item("item:a", "foo::bar", "c1"),
                item("item:b", "baz::qux", "c2"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let parsed = parse_dump_lines(&dump);
    assert_eq!(parsed.len(), 2);
    let qnames: Vec<&str> = parsed
        .iter()
        .map(|(_, v)| {
            v.get("props")
                .and_then(|p| p.get("qname"))
                .and_then(|q| q.as_str())
                .unwrap_or("")
        })
        .collect();
    assert_eq!(
        qnames,
        vec!["baz::qux", "foo::bar"],
        "nodes must sort by (label, qname), not by id"
    );
}

#[test]
fn canonical_dump_edge_sort_uses_label_then_src_qname_then_dst_qname() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![
                item("item:a", "alpha", "c1"),
                item("item:b", "bravo", "c1"),
                item("item:c", "charlie", "c1"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(
            &ks(),
            vec![
                Edge::new("item:b", "item:c", EdgeLabel::new(EdgeLabel::CALLS)),
                Edge::new("item:a", "item:c", EdgeLabel::new(EdgeLabel::CALLS)),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let parsed = parse_dump_lines(&dump);
    let edge_lines: Vec<&serde_json::Value> = parsed
        .iter()
        .filter_map(|(_, v)| {
            if v.get("kind").and_then(|k| k.as_str()) == Some("edge") {
                Some(v)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(edge_lines.len(), 2);

    let src_qnames: Vec<&str> = edge_lines
        .iter()
        .map(|v| v.get("src_qname").and_then(|s| s.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(
        src_qnames,
        vec!["alpha", "bravo"],
        "edges must sort by (label, src_qname, dst_qname)"
    );
}

#[test]
fn canonical_dump_edge_sort_fallback_when_src_qname_absent() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![
                call_site("cs:zebra"),
                call_site("cs:apple"),
                item("item:target", "target_qname", "c1"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(
            &ks(),
            vec![
                Edge::new("cs:zebra", "item:target", EdgeLabel::new(EdgeLabel::CALLS)),
                Edge::new("cs:apple", "item:target", EdgeLabel::new(EdgeLabel::CALLS)),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let parsed = parse_dump_lines(&dump);
    let edge_lines: Vec<&serde_json::Value> = parsed
        .iter()
        .filter_map(|(_, v)| {
            if v.get("kind").and_then(|k| k.as_str()) == Some("edge") {
                Some(v)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(edge_lines.len(), 2);

    let src_qnames: Vec<&str> = edge_lines
        .iter()
        .map(|v| v.get("src_qname").and_then(|s| s.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(
        src_qnames,
        vec!["cs:apple", "cs:zebra"],
        "fallback to id when qname prop is absent"
    );
}

#[test]
fn canonical_dump_lf_separated_no_trailing_newline() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::a", "c1")])
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    assert!(!dump.is_empty());
    assert!(
        !dump.ends_with('\n'),
        "canonical_dump output must NOT have a trailing newline (sha256sum reproducibility)"
    );
    assert!(!dump.contains('\r'), "canonical_dump must use LF, not CRLF");
}

#[test]
fn canonical_dump_byte_identity_across_two_calls() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![
                item("item:a", "foo::a", "c1"),
                item("item:b", "foo::b", "c1"),
                item("item:c", "foo::c", "c2"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(
            &ks(),
            vec![
                Edge::new("item:a", "item:b", EdgeLabel::new(EdgeLabel::CALLS)),
                Edge::new("item:b", "item:c", EdgeLabel::new(EdgeLabel::CALLS)),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let d1 = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let d2 = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    assert_eq!(d1.as_bytes(), d2.as_bytes(), "G1 byte identity must hold");
}
