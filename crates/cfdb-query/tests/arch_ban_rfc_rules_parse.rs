//! Static-check for the frozen RFC §4 invariant catalog
//! (`examples/queries/arch-ban-rfc-*.cypher`) — RFC-044 §3.8 slice 044-H
//! (issue #427).
//!
//! Slice 044-H converts reviewer-only RFC §4 invariants (RFC-042 :EntryPoint
//! emission contract, RFC-043 / Label::CALL_SITE discriminator contract) into
//! zero-tolerance `cfdb violations --rule` Cypher ban rules. The rules live in
//! `examples/queries/` (NOT the `.cfdb/queries/` of RFC §3.8's literal wording)
//! because the self-dogfood loop (`.gitea/workflows/ci.yml`) and the
//! cross-dogfood loop (`ci/cross-dogfood.sh`) already glob
//! `examples/queries/arch-ban-*.cypher` — placing the rules there auto-enforces
//! them with zero CI-workflow edits. Each rule's header comment documents the
//! deviation.
//!
//! This test is the slice's "Unit" surface: it parses every shipped
//! `arch-ban-rfc-*.cypher` via the cfdb-query parser and asserts that
//!
//!   1. each file is a single, parseable Cypher statement;
//!   2. every `:Label` and `[:EdgeLabel]` it references resolves to a known
//!      `cfdb_core::schema::{Label, EdgeLabel}` constant (catching a typo'd or
//!      out-of-schema vocabulary item before it ships); and
//!   3. the glob is non-empty AND found at least the catalogued rule count —
//!      so an accidental file deletion (which would silently make the
//!      self/cross-dogfood loops a no-op for these invariants) fails the build.
//!
//! Mirrors `predicate_schema_refs.rs`'s helper shape (RFC-035 §3.3
//! single-resolution-point: the label/edge collectors are the same walk).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::query::{Pattern, Predicate, Query};
use cfdb_core::schema::{EdgeLabel, Label};

/// Lower bound on the number of `arch-ban-rfc-*.cypher` rules slice 044-H
/// ships. Six genuinely static-expressible RFC §4 invariants were encoded
/// (see each file's header). New RFC §4 catalog rules (the continuous-catalog
/// policy, RFC-044 §3.8) only RAISE this floor — they never lower it. If a
/// rule is removed, this constant must be lowered in the same reviewed PR with
/// a rationale, exactly like a metric threshold (`const`, no baseline file —
/// project CLAUDE.md §6.8).
const MIN_RFC_CATALOG_RULES: usize = 6;

/// Node-`:Label` vocabulary known to the schema. Kept in lockstep with
/// `cfdb_core::schema::Label` constants; a new schema label extends BOTH here
/// and the impl consts (via a schema RFC).
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
    Label::LITERAL,
];

/// Edge-`[:EdgeLabel]` vocabulary known to the schema.
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
fn every_rfc_catalog_rule_parses_as_single_statement() {
    let rules = rfc_catalog_rule_files();
    for (path, source) in &rules {
        if let Err(e) = cfdb_query::parse(source) {
            panic!("{} failed to parse: {e}", path.display());
        }
    }
}

#[test]
fn every_rfc_catalog_rule_references_only_known_node_labels() {
    let known: BTreeSet<&str> = KNOWN_NODE_LABELS.iter().copied().collect();
    for (path, source) in rfc_catalog_rule_files() {
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
                "{}: references node label :{} which is not a cfdb_core::schema::Label \
                 constant. A typo'd or out-of-schema label would silently never match. If \
                 this is a new schema label, extend KNOWN_NODE_LABELS here AND the \
                 cfdb_core::schema::Label consts together (via a schema RFC).",
                path.display(),
                label
            );
        }
    }
}

#[test]
fn every_rfc_catalog_rule_references_only_known_edge_labels() {
    let known: BTreeSet<&str> = KNOWN_EDGE_LABELS.iter().copied().collect();
    for (path, source) in rfc_catalog_rule_files() {
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
                "{}: references edge label [:{}] which is not a cfdb_core::schema::EdgeLabel \
                 constant. If this is a new schema edge, extend KNOWN_EDGE_LABELS here AND \
                 the cfdb_core::schema::EdgeLabel consts together (via a schema RFC).",
                path.display(),
                label
            );
        }
    }
}

#[test]
fn rfc_catalog_is_not_below_floor() {
    let rules = rfc_catalog_rule_files();
    assert!(
        rules.len() >= MIN_RFC_CATALOG_RULES,
        "found {} examples/queries/arch-ban-rfc-*.cypher rules; slice 044-H ships at least \
         {MIN_RFC_CATALOG_RULES}. A rule file was deleted or the glob is broken — either \
         would silently turn the self/cross-dogfood RFC §4 invariant enforcement into a \
         no-op. Restore the rule, or lower MIN_RFC_CATALOG_RULES in the same reviewed PR \
         with a rationale (no baseline file — project CLAUDE.md §6.8).",
        rules.len()
    );
}

// --- helpers ---

/// `crates/cfdb-query/` is two levels below the workspace root.
fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .expect("crates/ parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Every `examples/queries/arch-ban-rfc-*.cypher` file, read and sorted by
/// path for deterministic iteration. Empty vec if the directory is missing —
/// `rfc_catalog_is_not_below_floor` catches that.
fn rfc_catalog_rule_files() -> Vec<(PathBuf, String)> {
    let dir = workspace_root().join("examples").join("queries");
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut entries: Vec<PathBuf> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let is_cypher = p.extension().and_then(|e| e.to_str()) == Some("cypher");
            let is_rfc_rule = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("arch-ban-rfc-"));
            is_cypher && is_rfc_rule
        })
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
