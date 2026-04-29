//! `cfdb-extractor-php` — PHP reference implementation of
//! `cfdb_lang::LanguageProducer` (RFC-041 Phase 2 / issue #264).
//!
//! # Bounded context
//!
//! Sits in the **language production** outer ring next to
//! `cfdb-extractor` (Rust producer). Depends on `cfdb-core` for the
//! `Node` / `Edge` / `Label` / `EdgeLabel` schema vocabulary and on
//! `cfdb-lang` for the `LanguageProducer` trait surface. Does NOT
//! depend on `cfdb-extractor` — the two producers are sibling crates
//! that share the trait but no implementation.
//!
//! # Schema mapping decision
//!
//! See `Cargo.toml` for the full rationale. The MVP maps PHP concepts
//! to the existing closed-set `:Item.kind` values per RFC-041 §4
//! Published Language invariant:
//!
//! - PHP `namespace` → `:Module` node (structural-context, not `:Item`)
//! - PHP `class` / `interface` / `trait` → `:Item { kind: "trait" }`
//! - PHP `method` / `function` → `:Item { kind: "fn" }`
//!
//! New `:Item.kind` values for PHP are deferred to a follow-up schema
//! RFC + `cfdb-core::SchemaVersion` patch + lockstep graph-specs PR.
//!
//! # Walker shape
//!
//! tree-sitter-php parses each `.php` file into a syntax tree. At the
//! top level a `program` node holds a flat sequence of children:
//! `namespace_definition`, `class_declaration`, `interface_declaration`,
//! `trait_declaration`, `function_definition`, etc. The PHP grammar
//! does NOT nest classes inside the namespace AST node — instead, a
//! `namespace_definition;` statement establishes the "current
//! namespace" for everything that follows it (until the next
//! namespace_definition or end of file).
//!
//! Therefore the walker tracks `current_namespace: Option<String>` as
//! it visits top-level children left-to-right and qualifies every
//! emitted `:Item.qname` with the active namespace.
//!
//! # Determinism
//!
//! Files are walked in sorted-path order; emitted nodes are sorted by
//! `(label, id)` and edges by `(src, dst, label)` before return.
//! Matches the canonical-dump shape `cfdb-extractor` already produces.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_lang::{LanguageError, LanguageProducer};

/// Stable producer name reported by [`LanguageProducer::name`] and
/// embedded in [`LanguageError::Parse::producer`]. Matches the
/// `lang-php` Cargo feature flag the CLI dispatcher will gate on once
/// integration wires this crate into `cfdb-cli`.
const PRODUCER_NAME: &str = "php";

/// Synthetic crate id used for every emitted `:Item` and `:Module`.
/// PHP has no Cargo-equivalent root manifest; the workspace root
/// directory name is the closest analogue. The MVP uses a fixed
/// `crate:php-workspace` id so the AC fixture has a stable
/// `IN_CRATE` target. A follow-up slice can derive this from
/// `composer.json`'s `"name"` field.
const CRATE_ID: &str = "crate:php-workspace";

/// PHP reference implementation of [`cfdb_lang::LanguageProducer`].
///
/// Detects a PHP workspace by the presence of `composer.json` at the
/// workspace root (the same shape `cfdb-extractor`'s `RustProducer`
/// uses for `Cargo.toml`). Parses every `.php` under the workspace
/// (excluding `vendor/`) via tree-sitter-php and emits structural
/// facts onto the closed-set schema vocabulary.
pub struct PhpProducer;

impl LanguageProducer for PhpProducer {
    fn name(&self) -> &'static str {
        PRODUCER_NAME
    }

    fn detect(&self, workspace_root: &Path) -> bool {
        workspace_root.join("composer.json").is_file()
    }

    fn produce(&self, workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), LanguageError> {
        produce_facts(workspace_root)
    }
}

// ---------------------------------------------------------------------------
// Fact production pipeline
// ---------------------------------------------------------------------------

/// Top-level pipeline: discover `.php` files, parse each, walk the
/// trees, accumulate nodes + edges, sort, return.
fn produce_facts(workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), LanguageError> {
    let mut emitter = Emitter::new();

    // Emit the synthetic :Crate node once. Every :Item carries an
    // IN_CRATE edge to it so cypher queries match the established
    // structural-context shape (`(:Item)-[:IN_CRATE]->(:Crate)`).
    emitter.emit_node(
        Node::new(CRATE_ID, Label::new(Label::CRATE))
            .with_prop("name", "php-workspace")
            .with_prop("is_workspace_member", true),
    );

    let php_files = collect_php_files(workspace_root)?;
    for path in php_files {
        walk_file(&path, &mut emitter)?;
    }

    let (mut nodes, mut edges) = emitter.finish();
    nodes.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    Ok((nodes, edges))
}

/// Walk `workspace_root` recursively and collect every `*.php` file,
/// skipping `vendor/` per the MVP scope. Returned paths are sorted so
/// the producer's output is byte-stable across reruns regardless of
/// platform-specific directory iteration order.
fn collect_php_files(workspace_root: &Path) -> Result<Vec<PathBuf>, LanguageError> {
    let mut out = Vec::new();
    walk_dir(workspace_root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), LanguageError> {
    let read = std::fs::read_dir(dir).map_err(LanguageError::Io)?;
    for entry in read {
        let entry = entry.map_err(LanguageError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            // Skip vendor/ — Composer-installed third-party code is
            // not part of the workspace's own fact set.
            if path.file_name().is_some_and(|n| n == "vendor") {
                continue;
            }
            walk_dir(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "php") {
            out.push(path);
        }
    }
    Ok(())
}

/// Parse one `.php` file and walk its syntax tree, emitting nodes +
/// edges into the shared `Emitter`.
fn walk_file(path: &Path, emitter: &mut Emitter) -> Result<(), LanguageError> {
    let source = std::fs::read_to_string(path).map_err(LanguageError::Io)?;

    let mut parser = tree_sitter::Parser::new();
    // tree-sitter-php 0.23+ exposes `LANGUAGE_PHP` as a `LanguageFn`
    // constant instead of the legacy `language_php()` fn (the workspace
    // pinned 0.23 to share the tree-sitter ABI with `cfdb-extractor-ts`
    // — see this crate's Cargo.toml). `.into()` performs the
    // `From<LanguageFn> for Language` conversion that 0.22 lacked.
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .map_err(|e| LanguageError::Parse {
            producer: PRODUCER_NAME,
            message: format!("set_language: {e}"),
        })?;
    let tree = parser.parse(&source, None).ok_or(LanguageError::Parse {
        producer: PRODUCER_NAME,
        message: format!("tree-sitter-php returned None for {}", path.display()),
    })?;

    let root = tree.root_node();
    walk_top_level(root, source.as_bytes(), emitter);
    Ok(())
}

/// Walk a `program` node's top-level children, tracking the current
/// namespace as `namespace_definition` siblings establish it. PHP's
/// AST does not nest declarations inside the namespace node — they
/// follow it as siblings until the next `namespace_definition` (or
/// EOF).
fn walk_top_level(program: tree_sitter::Node, src: &[u8], emitter: &mut Emitter) {
    let mut current_ns: Option<String> = None;
    let mut cursor = program.walk();
    for child in program.children(&mut cursor) {
        match child.kind() {
            "namespace_definition" => {
                let ns_name = extract_namespace_name(child, src);
                if let Some(name) = &ns_name {
                    emit_module(emitter, name);
                }
                current_ns = ns_name;
            }
            "class_declaration" | "interface_declaration" | "trait_declaration" => {
                emit_class_like(child, src, current_ns.as_deref(), emitter);
            }
            "function_definition" => {
                emit_function(child, src, current_ns.as_deref(), emitter);
            }
            _ => {}
        }
    }
}

/// Pull the dotted namespace name out of a `namespace_definition` node.
/// Tree-sitter-php exposes it as a `namespace_name` child whose `name`
/// children carry the path components (joined by `\\` in source).
fn extract_namespace_name(ns_node: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let mut cursor = ns_node.walk();
    for child in ns_node.children(&mut cursor) {
        if child.kind() == "namespace_name" {
            return text(child, src).map(|s| s.to_string());
        }
    }
    None
}

/// Emit a `:Module` node for a PHP namespace. The id is
/// `module:<namespace>` so the `:Item.IN_MODULE` edge target matches
/// the established schema.
fn emit_module(emitter: &mut Emitter, namespace: &str) {
    let id = module_id(namespace);
    if emitter.has_node(&id) {
        return;
    }
    emitter.emit_node(
        Node::new(&id, Label::new(Label::MODULE))
            .with_prop("name", namespace)
            .with_prop("path", namespace.replace('\\', "::")),
    );
}

fn module_id(namespace: &str) -> String {
    format!("module:{namespace}")
}

/// Emit a `:Item { kind: "trait" }` for a PHP `class_declaration`,
/// `interface_declaration`, or `trait_declaration` plus the corresponding
/// `IN_CRATE` and (when available) `IN_MODULE` edges. Recurses into the
/// declaration's `declaration_list` to emit method-level `:Item`s.
fn emit_class_like(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    emitter: &mut Emitter,
) {
    let Some(name) = find_named_child(node, "name", src) else {
        return;
    };
    let qname = qualify(current_ns, &name);
    let id = item_id(&qname);

    let line = (node.start_position().row + 1) as i64;
    emitter.emit_node(
        Node::new(&id, Label::new(Label::ITEM))
            .with_prop("kind", "trait")
            .with_prop("name", name.as_str())
            .with_prop("qname", qname.as_str())
            .with_prop("line", line)
            // Provenance for the schema-mapping decision (see
            // crate-root docs). Keeps the squashing visible to
            // downstream queries: a cypher rule looking only for
            // genuine PHP `trait` constructs can disambiguate by
            // filtering on this prop.
            .with_prop("php_construct", node.kind()),
    );
    emitter.emit_edge(Edge::new(
        &id,
        CRATE_ID,
        EdgeLabel::new(EdgeLabel::IN_CRATE),
    ));
    if let Some(ns) = current_ns {
        emitter.emit_edge(Edge::new(
            &id,
            module_id(ns),
            EdgeLabel::new(EdgeLabel::IN_MODULE),
        ));
    }

    // Walk the declaration_list for methods.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "declaration_list" {
            walk_declaration_list(child, src, current_ns, &qname, emitter);
        }
    }
}

/// Walk a class/interface/trait body, emitting `:Item { kind: "fn" }`
/// for each `method_declaration`.
fn walk_declaration_list(
    list: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    parent_qname: &str,
    emitter: &mut Emitter,
) {
    let mut cursor = list.walk();
    for child in list.children(&mut cursor) {
        if child.kind() == "method_declaration" {
            emit_method(child, src, current_ns, parent_qname, emitter);
        }
    }
}

/// Emit one `:Item { kind: "fn" }` for a `method_declaration` plus
/// `IN_CRATE` and (when available) `IN_MODULE` edges.
fn emit_method(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    parent_qname: &str,
    emitter: &mut Emitter,
) {
    let Some(name) = find_named_child(node, "name", src) else {
        return;
    };
    let qname = format!("{parent_qname}::{name}");
    let id = item_id(&qname);
    let line = (node.start_position().row + 1) as i64;
    emitter.emit_node(
        Node::new(&id, Label::new(Label::ITEM))
            .with_prop("kind", "fn")
            .with_prop("name", name.as_str())
            .with_prop("qname", qname.as_str())
            .with_prop("line", line)
            .with_prop("php_construct", "method_declaration"),
    );
    emitter.emit_edge(Edge::new(
        &id,
        CRATE_ID,
        EdgeLabel::new(EdgeLabel::IN_CRATE),
    ));
    if let Some(ns) = current_ns {
        emitter.emit_edge(Edge::new(
            &id,
            module_id(ns),
            EdgeLabel::new(EdgeLabel::IN_MODULE),
        ));
    }
}

/// Emit one `:Item { kind: "fn" }` for a top-level `function_definition`.
fn emit_function(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    emitter: &mut Emitter,
) {
    let Some(name) = find_named_child(node, "name", src) else {
        return;
    };
    let qname = qualify(current_ns, &name);
    let id = item_id(&qname);
    let line = (node.start_position().row + 1) as i64;
    emitter.emit_node(
        Node::new(&id, Label::new(Label::ITEM))
            .with_prop("kind", "fn")
            .with_prop("name", name.as_str())
            .with_prop("qname", qname.as_str())
            .with_prop("line", line)
            .with_prop("php_construct", "function_definition"),
    );
    emitter.emit_edge(Edge::new(
        &id,
        CRATE_ID,
        EdgeLabel::new(EdgeLabel::IN_CRATE),
    ));
    if let Some(ns) = current_ns {
        emitter.emit_edge(Edge::new(
            &id,
            module_id(ns),
            EdgeLabel::new(EdgeLabel::IN_MODULE),
        ));
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find the first direct child with a given `kind` and return its
/// source text. Used to locate the `name` child of class/method/fn
/// declarations.
fn find_named_child(node: tree_sitter::Node, kind: &str, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return text(child, src).map(|s| s.to_string());
        }
    }
    None
}

/// UTF-8 substring of source at `node`'s byte range. Returns `None`
/// only if the range crosses a non-UTF-8 boundary, which shouldn't
/// happen for valid PHP source.
fn text<'s>(node: tree_sitter::Node, src: &'s [u8]) -> Option<&'s str> {
    std::str::from_utf8(&src[node.byte_range()]).ok()
}

/// Compose a fully-qualified PHP name. `App` + `User` → `App\\User`
/// (PHP separator); rendered into the `qname` prop verbatim. Cypher
/// queries can normalise `\\` → `::` if they want to match Rust-style
/// qualified names.
fn qualify(ns: Option<&str>, name: &str) -> String {
    match ns {
        Some(ns) if !ns.is_empty() => format!("{ns}\\{name}"),
        _ => name.to_string(),
    }
}

fn item_id(qname: &str) -> String {
    format!("item:{qname}")
}

// ---------------------------------------------------------------------------
// Emitter — small helper that dedups :Module / :Crate node ids and
// otherwise just collects everything for sorting at finish().
// ---------------------------------------------------------------------------

struct Emitter {
    nodes: BTreeMap<String, Node>,
    edges: Vec<Edge>,
}

impl Emitter {
    fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: Vec::new(),
        }
    }

    fn emit_node(&mut self, node: Node) {
        // dedup on id — if the same module/crate id is emitted twice
        // (multi-file namespace), keep the first.
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    fn has_node(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    fn emit_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    fn finish(self) -> (Vec<Node>, Vec<Edge>) {
        (self.nodes.into_values().collect(), self.edges)
    }
}

// Suppress dead-code warning for the rare `PropValue` import path —
// kept around for future extensions that emit numeric / bool props
// the helpers don't yet construct.
#[allow(dead_code)]
fn _ensure_prop_value_in_use(v: PropValue) -> PropValue {
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PhpProducer` must be object-safe under `&dyn LanguageProducer`.
    /// Catches accidental supertrait drift if the trait surface ever
    /// gains a generic method or associated type.
    #[test]
    fn php_producer_is_object_safe() {
        fn _accept(_: &dyn LanguageProducer) {}
        _accept(&PhpProducer);
    }
}
