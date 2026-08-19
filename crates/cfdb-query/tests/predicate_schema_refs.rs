use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::query::{Pattern, Predicate, Query};
use cfdb_core::schema::{EdgeLabel, Label};

const KNOWN_NODE_LABELS: &[&str] = &[
    Label::CRATE,
    Label::MODULE,
    Label::FILE,
    Label::ITEM,
    Label::FIELD,
    Label::VARIANT,
    Label::PARAM,
    Label::CALL_SITE,
    Label::ENTRY_POINT,
    Label::CONCEPT,
    Label::CONTEXT,
    Label::RFC_DOC,
    Label::CONST_TABLE,
];

const KNOWN_EDGE_LABELS: &[&str] = &[
    EdgeLabel::IN_CRATE,
    EdgeLabel::IN_MODULE,
    EdgeLabel::HAS_FIELD,
    EdgeLabel::HAS_VARIANT,
    EdgeLabel::HAS_PARAM,
    EdgeLabel::HAS_CONST_TABLE,
    EdgeLabel::TYPE_OF,
    EdgeLabel::IMPLEMENTS,
    EdgeLabel::IMPLEMENTS_FOR,
    EdgeLabel::RETURNS,
    EdgeLabel::BELONGS_TO,
    EdgeLabel::CALLS,
    EdgeLabel::INVOKES_AT,
    EdgeLabel::EXPOSES,
    EdgeLabel::REGISTERS_PARAM,
    EdgeLabel::LABELED_AS,
    EdgeLabel::CANONICAL_FOR,
    EdgeLabel::EQUIVALENT_TO,
    EdgeLabel::REFERENCED_BY,
];

#[test]
fn every_seed_predicate_parses_cleanly() {
    for (path, source) in seed_predicate_files() {
        if let Err(e) = cfdb_query::parse(&source) {
            panic!("{} failed to parse: {e}", path.display());
        }
    }
}

#[test]
fn every_seed_predicate_references_only_known_node_labels() {
    let known: BTreeSet<&str> = KNOWN_NODE_LABELS.iter().copied().collect();
    for (path, source) in seed_predicate_files() {
        let query =
            cfdb_query::parse(&source).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        let mut found = BTreeSet::new();
        for pattern in &query.match_clauses {
            collect_node_labels(pattern, &mut found);
        }
        if let Some(pred) = &query.where_clause {
            collect_predicate_node_labels(pred, &mut found);
        }
        for label in &found {
            assert!(
                known.contains(label.as_str()),
                "{}: references node label :{} which is not in cfdb_core::schema::Label \
                 constants. If this is a new schema label, extend KNOWN_NODE_LABELS in this \
                 test AND the cfdb_core::schema::Label impl consts together (via a schema RFC).",
                path.display(),
                label
            );
        }
    }
}

#[test]
fn every_seed_predicate_references_only_known_edge_labels() {
    let known: BTreeSet<&str> = KNOWN_EDGE_LABELS.iter().copied().collect();
    for (path, source) in seed_predicate_files() {
        let query =
            cfdb_query::parse(&source).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        let mut found = BTreeSet::new();
        for pattern in &query.match_clauses {
            collect_edge_labels(pattern, &mut found);
        }
        if let Some(pred) = &query.where_clause {
            collect_predicate_edge_labels(pred, &mut found);
        }
        for label in &found {
            assert!(
                known.contains(label.as_str()),
                "{}: references edge label [:{}] which is not in cfdb_core::schema::EdgeLabel \
                 constants. If this is a new schema edge, extend KNOWN_EDGE_LABELS in this \
                 test AND the cfdb_core::schema::EdgeLabel impl consts together (via a \
                 schema RFC). Deferred-on-develop edges (e.g. RE_EXPORTS, which requires \
                 HIR Phase B per RFC-034 §6) MUST NOT appear in a shipped seed.",
                path.display(),
                label
            );
        }
    }
}

#[test]
fn seed_directory_is_not_empty() {
    let files = seed_predicate_files();
    assert!(
        !files.is_empty(),
        ".cfdb/predicates/ is empty; Slice 2 (#146) ships three seed predicates. If \
         this test fails, a seed file was accidentally deleted."
    );
}

fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .expect("crates/ parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn seed_predicate_files() -> Vec<(PathBuf, String)> {
    let dir = workspace_root().join(".cfdb").join("predicates");
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut entries: Vec<PathBuf> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("cypher"))
        .collect();
    entries.sort();
    entries
        .into_iter()
        .map(|p| {
            let src =
                fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            (p, src)
        })
        .collect()
}

fn collect_node_labels(pattern: &Pattern, out: &mut BTreeSet<Label>) {
    match pattern {
        Pattern::Node(n) => {
            if let Some(l) = &n.label {
                out.insert(l.clone());
            }
        }
        Pattern::Path(p) => {
            if let Some(l) = &p.from.label {
                out.insert(l.clone());
            }
            if let Some(l) = &p.to.label {
                out.insert(l.clone());
            }
        }
        Pattern::Optional(inner) => collect_node_labels(inner, out),
        Pattern::Unwind { .. } => {}
    }
}

fn collect_edge_labels(pattern: &Pattern, out: &mut BTreeSet<EdgeLabel>) {
    match pattern {
        Pattern::Node(_) => {}
        Pattern::Path(p) => {
            if let Some(l) = &p.edge.label {
                out.insert(l.clone());
            }
        }
        Pattern::Optional(inner) => collect_edge_labels(inner, out),
        Pattern::Unwind { .. } => {}
    }
}

fn collect_predicate_node_labels(pred: &Predicate, out: &mut BTreeSet<Label>) {
    match pred {
        Predicate::NotExists { inner } => {
            let q: &Query = inner;
            for pattern in &q.match_clauses {
                collect_node_labels(pattern, out);
            }
            if let Some(inner_pred) = &q.where_clause {
                collect_predicate_node_labels(inner_pred, out);
            }
        }
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            collect_predicate_node_labels(a, out);
            collect_predicate_node_labels(b, out);
        }
        Predicate::Not(inner) => collect_predicate_node_labels(inner, out),
        Predicate::Compare { .. }
        | Predicate::Ne { .. }
        | Predicate::In { .. }
        | Predicate::Regex { .. } => {}
    }
}

fn collect_predicate_edge_labels(pred: &Predicate, out: &mut BTreeSet<EdgeLabel>) {
    match pred {
        Predicate::NotExists { inner } => {
            let q: &Query = inner;
            for pattern in &q.match_clauses {
                collect_edge_labels(pattern, out);
            }
            if let Some(inner_pred) = &q.where_clause {
                collect_predicate_edge_labels(inner_pred, out);
            }
        }
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            collect_predicate_edge_labels(a, out);
            collect_predicate_edge_labels(b, out);
        }
        Predicate::Not(inner) => collect_predicate_edge_labels(inner, out),
        Predicate::Compare { .. }
        | Predicate::Ne { .. }
        | Predicate::In { .. }
        | Predicate::Regex { .. } => {}
    }
}
