use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::graph::GraphView;
use cfdb_core::schema::{EdgeLabel, Label};

pub(crate) const VERB: &str = "enrich_rfc_docs";

const SCAN_ROOTS: &[&str] = &["docs", ".concept-graph"];

pub(crate) fn run(view: &mut dyn GraphView, workspace_root: &Path) -> EnrichReport {
    let mut warnings: Vec<String> = Vec::new();

    let rfc_files = discover_rfc_files(workspace_root, &mut warnings);
    let scanned = scan_files(&rfc_files, workspace_root, &mut warnings);

    let item_label = Label::new(Label::ITEM);
    let items = collect_items(view, &item_label);

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

    view.ingest_nodes(rfc_nodes);
    view.ingest_edges(edges);

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

struct ScannedFile {
    path: String,
    title: Option<String>,
    content: String,
}

struct ItemRow {
    node_id: String,
    qname: String,
    name: String,
    file: String,
}

type References<'a> = BTreeMap<&'a str, std::collections::BTreeSet<usize>>;

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

fn collect_items(view: &dyn GraphView, label: &Label) -> Vec<ItemRow> {
    view.nodes_with_label(label)
        .into_iter()
        .filter_map(|id| view.node_by_id(&id).map(project_item_row))
        .filter(|row| !row.qname.is_empty() || !row.name.is_empty())
        .collect()
}

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

fn find_references<'a>(items: &'a [ItemRow], scanned: &'a [ScannedFile]) -> References<'a> {
    items
        .iter()
        .flat_map(|item| item_matches(item, scanned))
        .fold(BTreeMap::new(), |mut acc, (node_id, idx)| {
            acc.entry(node_id).or_default().insert(idx);
            acc
        })
}

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

fn emit_graph(scanned: &[ScannedFile], references: &References<'_>) -> (Vec<Node>, Vec<Edge>) {
    let referenced_idx: std::collections::BTreeSet<usize> = references
        .values()
        .flat_map(|set| set.iter().copied())
        .collect();

    let rfc_doc_label = Label::new(Label::RFC_DOC);
    let rfc_nodes: Vec<Node> = referenced_idx
        .iter()
        .map(|idx| build_rfc_doc_node(&scanned[*idx], &rfc_doc_label))
        .collect();

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

#[cfg(test)]
mod tests;
