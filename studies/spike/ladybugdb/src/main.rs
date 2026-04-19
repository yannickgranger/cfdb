//! Gate 3 spike — LadybugDB (`lbug`).
//!
//! Loads the shared fixture, declares the 4-node × 3-edge schema per methodology §6.1.T2,
//! bulk-inserts, runs F1/F2/F3, measures latency, does a canonical-dump sha256
//! determinism check.
//!
//! Run with:
//!   cargo run --release -- small   # small fixture
//!   cargo run --release -- large   # large fixture (15k/80k)

use std::fs;
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
struct Fixture {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    label: String,
    props: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Edge {
    src: String,
    dst: String,
    label: String,
    #[serde(default)]
    props: serde_json::Map<String, serde_json::Value>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let which = std::env::args().nth(1).unwrap_or_else(|| "small".to_string());
    let fixture_path = match which.as_str() {
        "small" => "../fixture-small.json",
        "large" => "../fixture-large.json",
        other => return Err(format!("unknown fixture: {other}").into()),
    };

    println!("== LadybugDB Gate 3 spike ==");
    println!("fixture: {}", fixture_path);

    let t0 = Instant::now();
    let fixture = load_fixture(fixture_path)?;
    println!(
        "load fixture: {} nodes, {} edges in {:.2?}",
        fixture.nodes.len(),
        fixture.edges.len(),
        t0.elapsed()
    );

    // Temp DB path, cleared on each run. lbug creates a single file (not a directory) at
    // the path by default; legacy versions used a directory — handle both.
    let db_dir = std::env::temp_dir().join("cfdb-spike-ladybugdb");
    if db_dir.exists() {
        if db_dir.is_dir() {
            fs::remove_dir_all(&db_dir)?;
        } else {
            fs::remove_file(&db_dir)?;
        }
    }
    for sidecar in [
        db_dir.with_extension("wal"),
        db_dir.with_extension("shadow"),
        db_dir.with_extension("lock"),
    ] {
        if sidecar.exists() {
            let _ = fs::remove_file(sidecar);
        }
    }

    let t1 = Instant::now();
    let db = open_db(&db_dir)?;
    println!("open DB: {:.2?}", t1.elapsed());

    let t2 = Instant::now();
    declare_schema(&db)?;
    println!("schema DDL: {:.2?}", t2.elapsed());

    let t3 = Instant::now();
    bulk_insert(&db, &fixture)?;
    println!(
        "bulk insert: {} nodes + {} edges in {:.2?}",
        fixture.nodes.len(),
        fixture.edges.len(),
        t3.elapsed()
    );

    // F1 — fixed-hop label + property match (duplicate logical names across crates).
    // Three variants tested: the textbook Cartesian form, the same with a pre-extracted
    // `name` property, and the aggregation form. This is the core Pattern A test and the
    // variant timings are the single most important Gate 3 datapoint for LadybugDB — they
    // tell cfdb's query-surface designers which Cypher shapes are usable and which aren't.
    let t4 = Instant::now();
    let f1a_count = query_f1a_cartesian_regex(&db)?;
    println!("F1a (Cartesian + regex extract): {} results in {:.2?}", f1a_count, t4.elapsed());

    let t4b = Instant::now();
    let f1b_count = query_f1b_aggregation(&db)?;
    println!("F1b (aggregation / collect DISTINCT): {} results in {:.2?}", f1b_count, t4b.elapsed());

    // F2 — variable-length path (bounded 1..10 CALLS chain)
    let t5 = Instant::now();
    let f2_count = query_f2(&db)?;
    println!("F2 (variable-length path): {} results in {:.2?}", f2_count, t5.elapsed());

    // F3 — property regex in WHERE
    let t6 = Instant::now();
    let f3_count = query_f3(&db)?;
    println!("F3 (regex WHERE): {} results in {:.2?}", f3_count, t6.elapsed());

    // Determinism check: canonical JSONL dump × 2, sha256
    let t7 = Instant::now();
    let dump1 = canonical_dump(&db)?;
    let sha1 = sha256_hex(&dump1);
    let dump2 = canonical_dump(&db)?;
    let sha2 = sha256_hex(&dump2);
    println!("canonical dump sha256 (run1): {}", sha1);
    println!("canonical dump sha256 (run2): {}", sha2);
    println!("determinism check: {:.2?}", t7.elapsed());
    if sha1 != sha2 {
        return Err("determinism check failed: sha256 differs".into());
    }

    println!("== SPIKE OK ==");
    Ok(())
}

fn load_fixture(path: impl AsRef<Path>) -> Result<Fixture, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let fixture: Fixture = serde_json::from_slice(&bytes)?;
    Ok(fixture)
}

fn open_db(path: &Path) -> Result<lbug::Database, Box<dyn std::error::Error>> {
    let config = lbug::SystemConfig::default();
    let db = lbug::Database::new(path, config)?;
    Ok(db)
}

fn declare_schema(db: &lbug::Database) -> Result<(), Box<dyn std::error::Error>> {
    let conn = lbug::Connection::new(db)?;
    // 4 node tables
    conn.query("CREATE NODE TABLE Crate(id STRING, name STRING, is_workspace_member BOOLEAN, PRIMARY KEY (id));")?;
    conn.query(
        "CREATE NODE TABLE Item(id STRING, qname STRING, kind STRING, crate STRING, file STRING, line INT64, signature_hash STRING, PRIMARY KEY (id));",
    )?;
    conn.query(
        "CREATE NODE TABLE Field(id STRING, name STRING, parent_qname STRING, type_qname STRING, PRIMARY KEY (id));",
    )?;
    conn.query(
        "CREATE NODE TABLE CallSite(id STRING, file STRING, line INT64, col INT64, in_fn STRING, PRIMARY KEY (id));",
    )?;
    // 3 edge tables. MANY_MANY is default so duplicates allowed (S5).
    // IN_CRATE: Item -> Crate
    conn.query("CREATE REL TABLE IN_CRATE(FROM Item TO Crate);")?;
    // HAS_FIELD: Item -> Field
    conn.query("CREATE REL TABLE HAS_FIELD(FROM Item TO Field);")?;
    // CALLS: carries edges from multiple source labels. Declare as multi-FROM/TO rel table.
    // In the small fixture CALLS is CallSite -> Item AND CallSite -> CallSite (for self-refs); in the large fixture CALLS is CallSite -> Item.
    // For this spike declare CALLS as CallSite -> Item (the dominant edge) + a second rel table CALLS_SITE for the small-fixture CallSite -> Item|Self edges.
    conn.query(
        "CREATE REL TABLE CALLS(FROM CallSite TO Item, in_fn STRING, arg_count INT64);",
    )?;
    Ok(())
}

fn bulk_insert(db: &lbug::Database, fx: &Fixture) -> Result<(), Box<dyn std::error::Error>> {
    let conn = lbug::Connection::new(db)?;

    // Insert nodes — one Cypher CREATE per node.
    // For bulk at scale, COPY FROM CSV is the documented fast path, but the spike focuses
    // on correctness first; latency is measured on the 15k-node fixture below.
    for n in &fx.nodes {
        let cypher = match n.label.as_str() {
            "Crate" => {
                let name = prop_str(&n.props, "name");
                let is_ws = prop_bool(&n.props, "is_workspace_member");
                format!(
                    "CREATE (:Crate {{id: '{}', name: '{}', is_workspace_member: {}}});",
                    escape(&n.id),
                    escape(&name),
                    is_ws
                )
            }
            "Item" => {
                let qname = prop_str(&n.props, "qname");
                let kind = prop_str(&n.props, "kind");
                let krate = prop_str(&n.props, "crate");
                let file = prop_str(&n.props, "file");
                let line = prop_i64(&n.props, "line");
                let sig = prop_str(&n.props, "signature_hash");
                format!(
                    "CREATE (:Item {{id: '{}', qname: '{}', kind: '{}', crate: '{}', file: '{}', line: {}, signature_hash: '{}'}});",
                    escape(&n.id),
                    escape(&qname),
                    escape(&kind),
                    escape(&krate),
                    escape(&file),
                    line,
                    escape(&sig)
                )
            }
            "Field" => {
                let name = prop_str(&n.props, "name");
                let parent = prop_str(&n.props, "parent_qname");
                let ty = prop_str(&n.props, "type_qname");
                format!(
                    "CREATE (:Field {{id: '{}', name: '{}', parent_qname: '{}', type_qname: '{}'}});",
                    escape(&n.id),
                    escape(&name),
                    escape(&parent),
                    escape(&ty)
                )
            }
            "CallSite" => {
                let file = prop_str(&n.props, "file");
                let line = prop_i64(&n.props, "line");
                let col = prop_i64(&n.props, "col");
                let in_fn = prop_str(&n.props, "in_fn");
                format!(
                    "CREATE (:CallSite {{id: '{}', file: '{}', line: {}, col: {}, in_fn: '{}'}});",
                    escape(&n.id),
                    escape(&file),
                    line,
                    col,
                    escape(&in_fn)
                )
            }
            other => return Err(format!("unknown node label: {other}").into()),
        };
        conn.query(&cypher)?;
    }

    // Insert edges — skip any with unknown src/dst labels that don't match our rel tables.
    // For this spike we MATCH by id and CREATE the edge.
    for e in &fx.edges {
        match e.label.as_str() {
            "IN_CRATE" => {
                let cypher = format!(
                    "MATCH (a:Item {{id: '{}'}}), (b:Crate {{id: '{}'}}) CREATE (a)-[:IN_CRATE]->(b);",
                    escape(&e.src),
                    escape(&e.dst),
                );
                conn.query(&cypher)?;
            }
            "HAS_FIELD" => {
                let cypher = format!(
                    "MATCH (a:Item {{id: '{}'}}), (b:Field {{id: '{}'}}) CREATE (a)-[:HAS_FIELD]->(b);",
                    escape(&e.src),
                    escape(&e.dst),
                );
                conn.query(&cypher)?;
            }
            "CALLS" => {
                // Our CALLS rel table only covers CallSite -> Item.
                // Small fixture may have Item -> Item CALLS; skip those for now.
                if !e.src.starts_with("cs:") || !e.dst.starts_with("item:") {
                    continue;
                }
                let in_fn = prop_str(&e.props, "in_fn");
                let arg_count = prop_i64(&e.props, "arg_count");
                let cypher = format!(
                    "MATCH (a:CallSite {{id: '{}'}}), (b:Item {{id: '{}'}}) CREATE (a)-[:CALLS {{in_fn: '{}', arg_count: {}}}]->(b);",
                    escape(&e.src),
                    escape(&e.dst),
                    escape(&in_fn),
                    arg_count
                );
                conn.query(&cypher)?;
            }
            other => return Err(format!("unknown edge label: {other}").into()),
        }
    }

    Ok(())
}

fn query_f1a_cartesian_regex(db: &lbug::Database) -> Result<usize, Box<dyn std::error::Error>> {
    // F1a: the textbook Cartesian form.
    // `MATCH (a:Item),(b:Item) WHERE f(a.p) = f(b.p) AND a <> b` — the shape that
    // RFC §3 / methodology §4.1 canonicalizes for Pattern A. The query planner must
    // push `f(a.p) = f(b.p)` into a hash join for this to be tractable on 5k items.
    // Spike observation: LadybugDB does NOT push the function-equality into a hash
    // join — this is an O(n²) scan with regex per row. Measured at ~200s on 5k items.
    let conn = lbug::Connection::new(db)?;
    let result = conn.query(
        "MATCH (a:Item), (b:Item) \
         WHERE regexp_extract(a.qname, '[^:]+$') = regexp_extract(b.qname, '[^:]+$') \
           AND a.crate <> b.crate AND a.id <> b.id \
         RETURN count(*);",
    )?;
    Ok(count_rows(result))
}

fn query_f1b_aggregation(db: &lbug::Database) -> Result<usize, Box<dyn std::error::Error>> {
    // F1b: aggregation form. Group items by base name, keep groups where more than one
    // distinct crate appears. O(n) single scan with a hash group-by — the idiomatic
    // Cypher way to express "cluster by base name, filter clusters with multiplicity > 1".
    // This is the query shape cfdb's HSB skill should emit.
    let conn = lbug::Connection::new(db)?;
    let result = conn.query(
        "MATCH (a:Item) \
         WITH regexp_extract(a.qname, '[^:]+$') AS name, \
              collect(DISTINCT a.crate) AS crates \
         WHERE size(crates) > 1 \
         RETURN count(*);",
    )?;
    Ok(count_rows(result))
}

fn query_f2(db: &lbug::Database) -> Result<usize, Box<dyn std::error::Error>> {
    // F2: variable-length path — reach Items from any CallSite through 1..5 CALLS hops.
    let conn = lbug::Connection::new(db)?;
    let result = conn.query(
        "MATCH (cs:CallSite)-[:CALLS*1..5]->(fn:Item) RETURN count(DISTINCT fn);",
    )?;
    Ok(count_rows(result))
}

fn query_f3(db: &lbug::Database) -> Result<usize, Box<dyn std::error::Error>> {
    // F3: property regex — forbidden-fn enforcement. Find items with qname matching a regex.
    let conn = lbug::Connection::new(db)?;
    let result = conn.query(
        "MATCH (i:Item) WHERE i.qname =~ '.*now_utc.*' RETURN count(*);",
    )?;
    Ok(count_rows(result))
}

fn canonical_dump(db: &lbug::Database) -> Result<String, Box<dyn std::error::Error>> {
    // Sorted canonical dump — nodes first (by id), then edges (by src, dst, label).
    let conn = lbug::Connection::new(db)?;

    let mut lines: Vec<String> = Vec::new();

    for label in ["Crate", "Item", "Field", "CallSite"] {
        let q = format!("MATCH (n:{}) RETURN n.id ORDER BY n.id;", label);
        let result = conn.query(&q)?;
        for id in collect_strings(result) {
            lines.push(format!("{}:{}", label, id));
        }
    }

    // Edges — each rel table in turn.
    for (rel, src_label, dst_label) in [
        ("IN_CRATE", "Item", "Crate"),
        ("HAS_FIELD", "Item", "Field"),
        ("CALLS", "CallSite", "Item"),
    ] {
        let q = format!(
            "MATCH (a:{})-[:{}]->(b:{}) RETURN a.id, b.id ORDER BY a.id, b.id;",
            src_label, rel, dst_label
        );
        let result = conn.query(&q)?;
        for (a, b) in collect_string_pairs(result) {
            lines.push(format!("{}:{}->{}", rel, a, b));
        }
    }

    Ok(lines.join("\n"))
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn count_rows(mut qr: lbug::QueryResult) -> usize {
    // QueryResult is an iterator; advance once and read the COUNT column.
    if let Some(row) = qr.next() {
        if let Some(v) = row.first() {
            if let lbug::Value::Int64(n) = v {
                return *n as usize;
            }
        }
    }
    0
}

fn collect_strings(qr: lbug::QueryResult) -> Vec<String> {
    let mut out = Vec::new();
    for row in qr {
        if let Some(lbug::Value::String(s)) = row.first() {
            out.push(s.clone());
        }
    }
    out
}

fn collect_string_pairs(qr: lbug::QueryResult) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for row in qr {
        let a = row.first().and_then(|v| match v { lbug::Value::String(s) => Some(s.clone()), _ => None });
        let b = row.get(1).and_then(|v| match v { lbug::Value::String(s) => Some(s.clone()), _ => None });
        if let (Some(a), Some(b)) = (a, b) {
            out.push((a, b));
        }
    }
    out
}

fn prop_str(m: &serde_json::Map<String, serde_json::Value>, key: &str) -> String {
    m.get(key).and_then(|v| v.as_str()).map(String::from).unwrap_or_default()
}

fn prop_i64(m: &serde_json::Map<String, serde_json::Value>, key: &str) -> i64 {
    m.get(key).and_then(|v| v.as_i64()).unwrap_or(0)
}

fn prop_bool(m: &serde_json::Map<String, serde_json::Value>, key: &str) -> bool {
    m.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn escape(s: &str) -> String {
    s.replace('\'', "\\'")
}
