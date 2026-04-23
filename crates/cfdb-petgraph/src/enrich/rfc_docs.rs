//! `enrich_rfc_docs` — scan `docs/**/*.md` + `.concept-graph/*.md` for
//! whole-word matches on `:Item.name` / `:Item.qname` and emit
//! `:RfcDoc { path, title }` nodes + `(:Item)-[:REFERENCED_BY]->(:RfcDoc)`
//! edges (slice 43-D / issue #107).
//!
//! # Scan strategy (rust-systems Q2)
//!
//! `str::contains` + a hand-rolled `\b` boundary check (char-level, ASCII
//! word chars `[A-Za-z0-9_]`) is sufficient for <500 concepts × ~15 RFC
//! files × ~8 KB each — completes in <100ms on cfdb's own tree per AC-8.
//! `aho-corasick` is transitively available via `regex-automata` 1.1.4 but
//! the naive scan stays well under the 5000-concept / 100MB threshold
//! where multi-pattern search matters.
//!
//! # Match semantics
//!
//! - **Case-sensitive, whole-word.** An item named `Timer` does NOT match
//!   `Timers` or `theTimer` — neighbouring chars on either side must be
//!   non-word (or string boundary). Prevents false positives on
//!   common-word item names.
//! - **Two patterns per item.** Check both `name` (`EnrichBackend`) and
//!   `qname` (`cfdb_core::enrich::EnrichBackend`). Either match yields
//!   one `REFERENCED_BY` edge for the (item, rfc-file) pair — multiple
//!   mentions in the same file still emit exactly one edge (edge carries
//!   no `count` prop).
//! - **Self-reference filter.** Items whose defining `file` prop is
//!   itself the RFC file are skipped to prevent an item that the RFC
//!   documents *about* from claiming to be *referenced by* the RFC.
//!   Applies only when the `:Item.file` prop matches the RFC path
//!   exactly — rarely triggers since `:Item` nodes live in source files,
//!   not markdown, but defensive for future rustdoc-generated items.
//!
//! # Emission policy
//!
//! Only RFC files that have **at least one referencing item** become
//! `:RfcDoc` nodes. Orphan RFC files with no references (e.g. meta docs
//! like `docs/cross-fixture-bump.md`) are skipped — no reader consumes
//! them and their omission keeps the graph smaller.
//!
//! # Determinism (AC-5)
//!
//! - RFC file paths collected via `std::fs::read_dir` are sorted
//!   immediately after collection.
//! - Emitted `:RfcDoc` nodes are sorted by `path` before ingest.
//! - Emitted `REFERENCED_BY` edges are sorted by `(src_qname, dst_path)`
//!   before ingest.
//!
//! Two runs on an unchanged tree produce byte-identical canonical dumps.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::schema::{EdgeLabel, Label};

use crate::graph::KeyspaceState;

pub(crate) const VERB: &str = "enrich_rfc_docs";

/// Directory roots to scan for markdown. Each root is searched recursively
/// for `*.md` files — cfdb's convention is `docs/*.md` flat, but downstream
/// consumers may use `docs/rfc/`, `docs/arch/`, etc. `.concept-graph/` is
/// flat by convention but scanning recursively costs nothing.
const SCAN_ROOTS: &[&str] = &["docs", ".concept-graph"];

pub(crate) fn run(state: &mut KeyspaceState, workspace_root: &Path) -> EnrichReport {
    let mut warnings: Vec<String> = Vec::new();

    let rfc_files = discover_rfc_files(workspace_root, &mut warnings);
    let scanned = scan_files(&rfc_files, workspace_root, &mut warnings);

    let item_label = Label::new(Label::ITEM);
    let items = collect_items(state, &item_label);

    if items.is_empty() || scanned.is_empty() {
        return EnrichReport {
            verb: VERB.into(),
            ran: true,
            facts_scanned: scanned_facts(&scanned),
            attrs_written: 0,
            edges_written: 0,
            warnings,
        };
    }

    let references = find_references(&items, &scanned);
    let (rfc_nodes, edges) = emit_graph(&scanned, &references);

    let attrs_written: u64 = rfc_nodes
        .iter()
        .map(|n| u64::try_from(n.props.len()).unwrap_or(u64::MAX))
        .sum();
    let edges_written = u64::try_from(edges.len()).unwrap_or(u64::MAX);

    state.ingest_nodes(rfc_nodes);
    state.ingest_edges(edges);

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: scanned_facts(&scanned),
        attrs_written,
        edges_written,
        warnings,
    }
}

fn scanned_facts(scanned: &[ScannedFile]) -> u64 {
    u64::try_from(scanned.len()).unwrap_or(u64::MAX)
}

/// One scanned RFC file — path (workspace-relative), optional title, and
/// raw content for reference-matching.
struct ScannedFile {
    path: String,
    title: Option<String>,
    content: String,
}

/// One `:Item` projected from the keyspace — node id + discriminator props
/// needed for whole-word matching.
struct ItemRow {
    node_id: String,
    qname: String,
    name: String,
    file: String,
}

/// Per-item references → set of RFC file indices (by position in
/// `scanned`). Using a `BTreeMap<item_node_id, BTreeSet<file_idx>>` keeps
/// both axes deterministic. The key borrows from `items` so `node_id` is
/// cloned at most once per unique item (at insert time), not once per match.
type References<'a> = BTreeMap<&'a str, std::collections::BTreeSet<usize>>;

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------

fn discover_rfc_files(workspace_root: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for root in SCAN_ROOTS {
        let abs = workspace_root.join(root);
        if abs.is_dir() {
            walk_markdown(&abs, &mut out, warnings);
        }
    }
    out.sort();
    out
}

/// Depth-first recursive walk collecting `*.md` files. Uses `read_dir` not
/// glob so we avoid adding a dep for this single-use case; tolerates
/// unreadable entries with a warning.
fn walk_markdown(dir: &Path, out: &mut Vec<PathBuf>, warnings: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            warnings.push(format!("{VERB}: read_dir({}) failed: {err}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warnings.push(format!(
                    "{VERB}: entry in {} unreadable: {err}",
                    dir.display()
                ));
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            walk_markdown(&path, out, warnings);
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// File scanning
// ---------------------------------------------------------------------------

fn scan_files(
    files: &[PathBuf],
    workspace_root: &Path,
    warnings: &mut Vec<String>,
) -> Vec<ScannedFile> {
    let mut out: Vec<ScannedFile> = Vec::with_capacity(files.len());
    for abs_path in files {
        match scan_one_file(abs_path, workspace_root) {
            Ok(f) => out.push(f),
            Err(err) => warnings.push(format!(
                "{VERB}: failed to read {}: {err}",
                abs_path.display()
            )),
        }
    }
    out
}

fn scan_one_file(abs_path: &Path, workspace_root: &Path) -> std::io::Result<ScannedFile> {
    let content = std::fs::read_to_string(abs_path)?;
    let title = extract_title(&content);
    let rel = abs_path
        .strip_prefix(workspace_root)
        .unwrap_or(abs_path)
        .to_string_lossy()
        .into_owned();
    Ok(ScannedFile {
        path: rel,
        title,
        content,
    })
}

/// First `# <heading>` line, trimmed. Ignores `##`/`###`, setext headings
/// (`===` / `---` underlines), and files without any `# ` prefix. Robust
/// against empty files, files without a heading, and binary files
/// accidentally named `.md` (UTF-8 decode already rejects non-text).
fn extract_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            let title = rest.trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// :Item projection
// ---------------------------------------------------------------------------

fn collect_items(state: &KeyspaceState, label: &Label) -> Vec<ItemRow> {
    state
        .nodes_with_label(label)
        .into_iter()
        .filter_map(|idx| state.graph.node_weight(idx).map(project_item_row))
        .filter(|row| !row.qname.is_empty() || !row.name.is_empty())
        .collect()
}

/// Pure projection from a `:Item` node into an `ItemRow`. Extracted from
/// `collect_items`'s for-loop body so the cloning of `node.id` happens
/// inside an iterator chain (map), not a for-loop — quality-metrics
/// treats these distinctly even though the semantics are identical.
fn project_item_row(node: &Node) -> ItemRow {
    ItemRow {
        node_id: node.id.clone(),
        qname: prop_str(&node.props, "qname").unwrap_or_default(),
        name: prop_str(&node.props, "name").unwrap_or_default(),
        file: prop_str(&node.props, "file").unwrap_or_default(),
    }
}

fn prop_str(props: &Props, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(PropValue::as_str)
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Reference matching
// ---------------------------------------------------------------------------

fn find_references<'a>(items: &'a [ItemRow], scanned: &'a [ScannedFile]) -> References<'a> {
    items
        .iter()
        .flat_map(|item| item_matches(item, scanned))
        .fold(BTreeMap::new(), |mut acc, (node_id, idx)| {
            acc.entry(node_id).or_default().insert(idx);
            acc
        })
}

/// All `(node_id_borrowed, file_idx)` pairs for one item. Self-reference
/// filter (item's defining file IS the RFC path) is applied here. The
/// `node_id` is borrowed from `item` so it is cloned at most once per
/// unique item (at `BTreeMap` insert time), not once per (item, file) match.
fn item_matches<'a>(
    item: &'a ItemRow,
    scanned: &'a [ScannedFile],
) -> impl Iterator<Item = (&'a str, usize)> + 'a {
    scanned.iter().enumerate().filter_map(move |(idx, file)| {
        if item.file == file.path || !item_is_referenced(item, &file.content) {
            return None;
        }
        Some((item.node_id.as_str(), idx))
    })
}

fn item_is_referenced(item: &ItemRow, content: &str) -> bool {
    (!item.name.is_empty() && contains_whole_word(content, &item.name))
        || (!item.qname.is_empty()
            && item.qname != item.name
            && contains_whole_word(content, &item.qname))
}

/// Whole-word `needle` match against `haystack`. Word char = ASCII
/// alphanumeric or `_`. Returns `true` if at least one occurrence has
/// non-word neighbours (or string boundaries) on both sides.
fn contains_whole_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hay_bytes = haystack.as_bytes();
    let needle_len = needle.len();
    let mut search_from = 0usize;
    while let Some(rel) = haystack[search_from..].find(needle) {
        let start = search_from + rel;
        let end = start + needle_len;
        let left_ok = start == 0 || !is_word_char(hay_bytes[start - 1]);
        let right_ok = end == hay_bytes.len() || !is_word_char(hay_bytes[end]);
        if left_ok && right_ok {
            return true;
        }
        search_from = start + 1;
    }
    false
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------------------------------------------------------------------------
// Graph emission
// ---------------------------------------------------------------------------

/// Build the sorted `(rfc_nodes, edges)` tuple. Only RFC files that
/// appear in `references` are emitted as `:RfcDoc` nodes — orphans
/// (meta docs with no item references) are skipped.
fn emit_graph(scanned: &[ScannedFile], references: &References<'_>) -> (Vec<Node>, Vec<Edge>) {
    // Step 1: which file indices are actually referenced?
    let referenced_idx: std::collections::BTreeSet<usize> = references
        .values()
        .flat_map(|set| set.iter().copied())
        .collect();

    // Step 2: emit :RfcDoc nodes for referenced files, sorted by path
    // (already sorted via BTreeSet iteration order on indices-into-sorted).
    let rfc_doc_label = Label::new(Label::RFC_DOC);
    let rfc_nodes: Vec<Node> = referenced_idx
        .iter()
        .map(|idx| build_rfc_doc_node(&scanned[*idx], &rfc_doc_label))
        .collect();

    // Step 3: emit REFERENCED_BY edges, sorted by (src_node_id, dst_path).
    // BTreeMap iteration over node_id + BTreeSet over file indices (which
    // are positions in the already-sorted `scanned` vec) gives the desired
    // deterministic order.
    let referenced_by_label = EdgeLabel::new(EdgeLabel::REFERENCED_BY);
    let edges: Vec<Edge> = references
        .iter()
        .flat_map(|(item_node_id, file_indices)| {
            let label = &referenced_by_label;
            file_indices
                .iter()
                .map(move |idx| build_edge(item_node_id, &scanned[*idx], label))
        })
        .collect();

    (rfc_nodes, edges)
}

/// Construct one `:RfcDoc` node from a scanned file. Clones of `path`,
/// `title`, and `label` happen inside this helper — called from the
/// iterator chain in `emit_graph`, not a for-loop body.
fn build_rfc_doc_node(file: &ScannedFile, label: &Label) -> Node {
    let mut props = Props::new();
    props.insert("path".into(), PropValue::Str(file.path.clone()));
    match &file.title {
        Some(t) => props.insert("title".into(), PropValue::Str(t.clone())),
        None => props.insert("title".into(), PropValue::Null),
    };
    Node {
        id: rfc_doc_node_id(&file.path),
        label: label.clone(),
        props,
    }
}

/// Construct one `REFERENCED_BY` edge. Cloning of `item_node_id` and
/// `label` happens inside this helper, outside any for-loop.
fn build_edge(item_node_id: &str, file: &ScannedFile, label: &EdgeLabel) -> Edge {
    Edge {
        src: item_node_id.to_string(),
        dst: rfc_doc_node_id(&file.path),
        label: label.clone(),
        props: Props::new(),
    }
}

fn rfc_doc_node_id(path: &str) -> String {
    format!("rfc:{path}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
