//! cfdb-petgraph — `StoreBackend` implementation on `petgraph::StableDiGraph`.
//!
//! One `KeyspaceState` per keyspace; each holds a `StableDiGraph<Node, Edge>`
//! plus an insertion-ordered id → `NodeIndex` map (`indexmap::IndexMap`) and a
//! `BTreeMap`-based label index.
//!
//! Evaluation is routed through `eval::Evaluator` which ports the Gate 3 spike
//! (`studies/spike/petgraph/src/main.rs`) onto the real
//! `cfdb_core::Query` AST. Canonical dumping is a single sorted `Vec<String>`
//! join so two consecutive calls are byte-identical (RFC §12 G1).
//!
//! NOTE on pathological-shape lint (study 001 §4.2): v0.1 delegates that check
//! to `cfdb-query::shape_lint` — callers run the lint at parse time and
//! decide whether to call `execute`. The evaluator does not re-run the lint.

mod enrich;
mod eval;
mod graph;
pub mod persist;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::query::Query;
use cfdb_core::result::QueryResult;
use cfdb_core::schema::{Keyspace, SchemaVersion};
use cfdb_core::store::{StoreBackend, StoreError};
use petgraph::visit::IntoEdgeReferences;
use serde_json::Value;

use crate::eval::Evaluator;
use crate::graph::KeyspaceState;

/// In-memory petgraph-backed store. One `StableDiGraph` per keyspace.
///
/// The store is `Send + Sync` by virtue of its contents; concurrent readers
/// are not yet supported — the trait takes `&mut self` for writes and `&self`
/// for reads, so callers wrap the store in an external `RwLock` if they need
/// parallel evaluation.
pub struct PetgraphStore {
    keyspaces: BTreeMap<Keyspace, KeyspaceState>,
    schema_version: SchemaVersion,
    /// Optional workspace root for enrichment passes that read files
    /// (`enrich_rfc_docs`, `enrich_concepts`). `None` when the store was
    /// constructed for tests or for non-enrichment workflows. Wired by
    /// [`crate::PetgraphStore::with_workspace`]; [`crate::PetgraphStore::new`]
    /// remains argument-less so existing callers (30+ test sites, persist
    /// round-trips) compile unchanged. Slices 43-D (issue #107) and 43-F
    /// (issue #109) will consume this field via
    /// [`crate::PetgraphStore::workspace_root`] without changing the
    /// `EnrichBackend` port signature — clean-arch B4 resolution
    /// (`council/43/clean-arch.md`).
    workspace_root: Option<PathBuf>,
}

impl Default for PetgraphStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PetgraphStore {
    /// Create an empty store at `SchemaVersion::CURRENT`. New keyspaces
    /// are tagged with the current build's schema version; any legacy file
    /// ingested via `persist::load` retains its own version unless it is
    /// rewritten through `persist::save` (which stamps CURRENT).
    pub fn new() -> Self {
        Self {
            keyspaces: BTreeMap::new(),
            schema_version: SchemaVersion::CURRENT,
            workspace_root: None,
        }
    }

    /// Attach a workspace root for enrichment passes that read files.
    /// Builder-style — returns `self` so a caller can chain
    /// `PetgraphStore::new().with_workspace(path)` without changing the
    /// zero-arg `::new()` signature that 30+ call sites depend on. The
    /// composition root (`cfdb-cli::compose::load_store`) will wire this
    /// when slices 43-D / 43-F actually need a workspace path; until then
    /// every existing construction path returns `workspace_root = None`.
    pub fn with_workspace(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(root.into());
        self
    }

    /// Return the attached workspace root, if any. Slices 43-D and 43-F
    /// will consume this to locate `docs/rfc/*.md` and
    /// `.cfdb/concepts/*.toml` without modifying the `EnrichBackend` port
    /// signature.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Return a reference to a keyspace, creating it if missing.
    fn keyspace_mut(&mut self, keyspace: &Keyspace) -> &mut KeyspaceState {
        if !self.keyspaces.contains_key(keyspace) {
            self.keyspaces
                .insert(keyspace.clone(), KeyspaceState::new());
        }
        self.keyspaces
            .get_mut(keyspace)
            .expect("keyspace just inserted must be present")
    }

    /// Export the raw nodes and edges of a keyspace. Used by
    /// [`crate::persist::save`] to serialize the keyspace to disk. Returns
    /// facts in insertion order; the caller sorts for canonical output.
    pub fn export(&self, keyspace: &Keyspace) -> Result<(Vec<Node>, Vec<Edge>), StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;

        let nodes: Vec<Node> = state.graph.node_weights().cloned().collect();
        let edges: Vec<Edge> = IntoEdgeReferences::edge_references(&state.graph)
            .map(|e| e.weight().clone())
            .collect();
        Ok((nodes, edges))
    }
}

impl StoreBackend for PetgraphStore {
    fn ingest_nodes(&mut self, keyspace: &Keyspace, nodes: Vec<Node>) -> Result<(), StoreError> {
        self.keyspace_mut(keyspace).ingest_nodes(nodes);
        Ok(())
    }

    fn ingest_edges(&mut self, keyspace: &Keyspace, edges: Vec<Edge>) -> Result<(), StoreError> {
        self.keyspace_mut(keyspace).ingest_edges(edges);
        Ok(())
    }

    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;
        let mut result = Evaluator::new(state, &query.params).run(query);
        let mut prepended = state.ingest_warnings.clone();
        prepended.append(&mut result.warnings);
        result.warnings = prepended;
        Ok(result)
    }

    fn schema_version(&self, keyspace: &Keyspace) -> Result<SchemaVersion, StoreError> {
        if !self.keyspaces.contains_key(keyspace) {
            return Err(StoreError::UnknownKeyspace(keyspace.clone()));
        }
        Ok(self.schema_version)
    }

    fn list_keyspaces(&self) -> Vec<Keyspace> {
        self.keyspaces.keys().cloned().collect()
    }

    fn drop_keyspace(&mut self, keyspace: &Keyspace) -> Result<(), StoreError> {
        self.keyspaces.remove(keyspace);
        Ok(())
    }

    fn canonical_dump(&self, keyspace: &Keyspace) -> Result<String, StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;
        Ok(canonical_dump(state))
    }
}

// RFC-031 §2 — enrichment is a sibling trait. PetgraphStore inherits the
// seven Phase A stubs (`EnrichReport::not_implemented`); concrete enrichment
// passes override individual methods as #43 slices land.
//
// `enrich_deprecation` overridden in slice 43-C (#106) to report the real
// source as the extractor rather than deflecting to `not_implemented`. The
// deprecation facts (`is_deprecated`, `deprecation_since`) are populated at
// extraction time by `cfdb-extractor` via `extract_deprecated_attr`, so the
// `EnrichBackend::enrich_deprecation` method is a runtime no-op but must
// advertise its non-stub status — `ran: true, attrs_written: 0` with a
// warning naming the extractor so callers can distinguish "done upstream"
// from "deferred".
impl EnrichBackend for PetgraphStore {
    fn enrich_deprecation(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        // Keyspace existence check mirrors other enrichment verbs — a
        // caller targeting an unknown keyspace gets the same error shape
        // as `schema_version`/`drop_keyspace`.
        if !self.keyspaces.contains_key(keyspace) {
            return Err(StoreError::UnknownKeyspace(keyspace.clone()));
        }
        Ok(cfdb_core::enrich::EnrichReport {
            verb: "enrich_deprecation".into(),
            ran: true,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![
                "enrich_deprecation: facts populated at extraction time by cfdb-extractor::extract_deprecated_attr (#43-C / RFC addendum §A2.2 row 3); no enrichment work to do"
                    .into(),
            ],
        })
    }

    fn enrich_git_history(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        if !self.keyspaces.contains_key(keyspace) {
            return Err(StoreError::UnknownKeyspace(keyspace.clone()));
        }
        Ok(enrich_git_history_dispatch(self, keyspace))
    }
}

/// Feature-off path — the real pass is gated on `git-enrich` to keep libgit2
/// out of default builds (rust-systems Q1 / Q6). Without the feature the verb
/// still exists and dispatches here, returning a `ran: false` report whose
/// warning names the feature flag (AC-1 / issue #105).
#[cfg(not(feature = "git-enrich"))]
fn enrich_git_history_dispatch(
    _store: &mut PetgraphStore,
    _keyspace: &cfdb_core::schema::Keyspace,
) -> cfdb_core::enrich::EnrichReport {
    cfdb_core::enrich::EnrichReport {
        verb: "enrich_git_history".into(),
        ran: false,
        facts_scanned: 0,
        attrs_written: 0,
        edges_written: 0,
        warnings: vec![
            "enrich_git_history: built without `git-enrich` feature — recompile `cfdb-cli` with `--features git-enrich` to populate git-history facts (RFC addendum §A2.2 row 1 / issue #105)"
                .into(),
        ],
    }
}

/// Feature-on path — requires a `workspace_root` on the store. If the store
/// was built without one (most test sites and tool-free callers), return a
/// `ran: false` degraded report so the caller sees the configuration gap
/// rather than silent Nulls.
#[cfg(feature = "git-enrich")]
fn enrich_git_history_dispatch(
    store: &mut PetgraphStore,
    keyspace: &cfdb_core::schema::Keyspace,
) -> cfdb_core::enrich::EnrichReport {
    let Some(root) = store.workspace_root.clone() else {
        return cfdb_core::enrich::EnrichReport {
            verb: "enrich_git_history".into(),
            ran: false,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![
                "enrich_git_history: no workspace_root attached to PetgraphStore — construct via `PetgraphStore::new().with_workspace(root)` so the pass can open a git repository"
                    .into(),
            ],
        };
    };
    let state = store
        .keyspaces
        .get_mut(keyspace)
        .expect("keyspace presence checked by caller");
    crate::enrich::git_history::run(state, &root)
}

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
fn canonical_dump(state: &KeyspaceState) -> String {
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

#[cfg(test)]
mod tests;
