//! `GraphView` / `GraphReader` / `GraphBackend` — the per-keyspace graph
//! ports, split from the concrete petgraph storage engine.
//!
//! Siblings of [`crate::store::StoreBackend`] and [`crate::enrich::EnrichBackend`]:
//! `StoreBackend` is the storage contract, `EnrichBackend` the
//! enrichment-verb dispatch contract, and the traits here are the narrower
//! surfaces one keyspace exposes to code that must not depend on the
//! concrete storage representation (`cfdb-petgraph`'s
//! `StableDiGraph`/`NodeIndex`):
//!
//! - [`GraphView`] — id-based read/write surface an enrichment *pass* needs
//!   to walk and mutate a keyspace.
//! - [`GraphReader`] — handle-based, read-only surface the Cypher evaluator
//!   needs: label/edge-label vocabulary, ordered scans, node/edge
//!   dereference, adjacency, index-accelerated candidate lookup and the
//!   ingest diagnostics every query result carries.
//! - [`GraphBackend`] — the per-store factory that resolves a keyspace into
//!   either surface.
//!
//! All three are dyn-safe by design — no generics, no associated types — so
//! a `GraphBackend` implementor can hand out `&mut dyn GraphView` /
//! `&dyn GraphReader` without exposing its concrete graph representation.

use std::collections::BTreeMap;
use std::path::Path;

use crate::fact::{Edge, Node, PropValue};
use crate::query::{NodePattern, ParamBinding, Predicate};
use crate::result::Warning;
use crate::schema::{Direction, EdgeLabel, Keyspace, Label};
use crate::store::StoreError;

/// Opaque, storage-owned position of a node inside one keyspace.
///
/// Valid only for the [`GraphReader`] it was obtained from, for as long as
/// that reader is borrowed. `Ord`/`Hash` are over the raw value so that a
/// sort or set keyed by handles reproduces the storage's own index order —
/// the determinism (G1) tie-break every ordered read relies on. Code that
/// consumes a `GraphReader` never interprets the raw value; only the
/// storage engine constructs handles and maps them back.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeHandle(u32);

impl NodeHandle {
    /// Wrap a storage-side index. Storage-engine use only.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The storage-side index this handle wraps. Storage-engine use only.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Opaque, storage-owned position of an edge inside one keyspace. Same
/// contract as [`NodeHandle`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeHandle(u32);

impl EdgeHandle {
    /// Wrap a storage-side index. Storage-engine use only.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The storage-side index this handle wraps. Storage-engine use only.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// The per-keyspace read/write surface an enrichment pass needs.
///
/// Every method is id-based (`&str`), never keyed by a storage-internal
/// index — the whole point of this port is that a pass coded against it
/// cannot reach into (and cannot accidentally start depending on) the
/// concrete graph representation.
pub trait GraphView {
    /// Look up a node by id.
    fn node_by_id(&self, id: &str) -> Option<&Node>;

    /// Ids of every node carrying the given label, in a stable, deterministic
    /// order (G1 — implementors must preserve whatever ordering guarantee
    /// their underlying storage already provides; this port does not
    /// re-derive one).
    fn nodes_with_label(&self, label: &Label) -> Vec<String>;

    /// The `(edge label, other-endpoint id)` pairs reachable from `id` in the
    /// given [`Direction`]. `Direction::Undirected` returns the union of
    /// both directions.
    fn neighbors(&self, id: &str, dir: Direction) -> Vec<(EdgeLabel, String)>;

    /// Set a single attribute on the node with the given id. Returns `false`
    /// if `id` is unknown (no-op, not an error — mirrors how enrichment
    /// passes already treat a missing node today).
    ///
    /// Byte-faithful to the pre-port write path: does NOT reconcile any
    /// inverted index (`by_prop`) an implementor might maintain — only
    /// batch ingestion (`ingest_nodes`/`ingest_edges`) does that today. A
    /// pass that needs a property both readable via an index AND writable
    /// via `set_attr` must re-ingest the node, not rely on this method
    /// alone.
    fn set_attr(&mut self, id: &str, key: &str, value: PropValue) -> bool;

    /// Add or replace a batch of nodes.
    fn ingest_nodes(&mut self, nodes: Vec<Node>);

    /// Add a batch of edges. Endpoints referencing unknown ids are skipped
    /// (mirrors [`crate::store::StoreBackend::ingest_edges`]'s degrade-not-fail
    /// contract).
    fn ingest_edges(&mut self, edges: Vec<Edge>);
}

/// The per-keyspace, read-only, handle-based surface the Cypher evaluator
/// needs. Every method takes `&self`: a query cannot mutate a keyspace (G2)
/// by construction, not by convention.
///
/// Every ordered read returns handles in the storage's own stable order;
/// implementors wrap their existing ordered accessors rather than
/// re-deriving an order (G1).
pub trait GraphReader {
    /// Whether any node carries `label`.
    fn has_label(&self, label: &Label) -> bool;

    /// Every label present in the keyspace, sorted.
    fn labels(&self) -> Vec<Label>;

    /// Whether any edge carries `label`.
    fn has_edge_label(&self, label: &EdgeLabel) -> bool;

    /// Every edge label present in the keyspace, sorted.
    fn edge_labels(&self) -> Vec<EdgeLabel>;

    /// Handles of every node carrying `label`, in storage order.
    fn nodes_with_label(&self, label: &Label) -> Vec<NodeHandle>;

    /// Handles of every node in the keyspace, in storage order.
    fn all_nodes_sorted(&self) -> Vec<NodeHandle>;

    /// The node behind `h`, or `None` if the handle does not resolve.
    fn node(&self, h: NodeHandle) -> Option<&Node>;

    /// The edge behind `h`, or `None` if the handle does not resolve.
    fn edge(&self, h: EdgeHandle) -> Option<&Edge>;

    /// `(edge, target)` for every edge leaving `h`, in storage order.
    fn edges_out(&self, h: NodeHandle) -> Vec<(EdgeHandle, NodeHandle)>;

    /// `(edge, source)` for every edge entering `h`, in storage order.
    fn edges_in(&self, h: NodeHandle) -> Vec<(EdgeHandle, NodeHandle)>;

    /// Index-accelerated candidate set for a node pattern (RFC-035).
    ///
    /// `bound_var_prop(var, prop)` resolves a property of a variable the
    /// caller has already bound, so cross-pattern equality hints can narrow
    /// the candidate set. `None` means no usable index hint — the caller
    /// falls back to a scan. `Some(vec![])` means the pattern is provably
    /// empty under the index — a result, not a fallback.
    fn index_candidates(
        &self,
        np: &NodePattern,
        where_clause: Option<&Predicate>,
        params: &BTreeMap<String, ParamBinding>,
        bound_var_prop: &dyn Fn(&str, &str) -> Option<PropValue>,
    ) -> Option<Vec<NodeHandle>>;

    /// Whether `(label, tag)` is both declared indexed and non-empty — the
    /// guard that decides if a coupling-shaped pattern can be hoisted out
    /// of a per-row loop.
    fn indexed_prop_is_populated(&self, label: &Label, tag: &str) -> bool;

    /// The ingest-time diagnostics of this keyspace, in recorded order.
    /// Every query result carries them ahead of its own warnings.
    fn ingest_warnings(&self) -> Vec<Warning>;
}

/// The per-store factory that resolves a keyspace into a [`GraphView`] or a
/// [`GraphReader`].
///
/// `Send + Sync` mirrors [`crate::store::StoreBackend`] and
/// [`crate::enrich::EnrichBackend`] — required so a generic `EnrichEngine<S:
/// GraphBackend>` can itself be `Send + Sync` (its only field is `&mut S`,
/// so that bound is otherwise unprovable for an unconstrained `S`).
pub trait GraphBackend: Send + Sync {
    /// Resolve `keyspace` into its [`GraphView`]. `Err(StoreError::UnknownKeyspace)`
    /// if the store has never seen this keyspace.
    fn graph_view(&mut self, keyspace: &Keyspace) -> Result<&mut dyn GraphView, StoreError>;

    /// Resolve `keyspace` into its read-only [`GraphReader`].
    /// `Err(StoreError::UnknownKeyspace)` if the store has never seen this
    /// keyspace.
    fn graph_reader(&self, keyspace: &Keyspace) -> Result<&dyn GraphReader, StoreError>;

    /// The workspace root attached to this store, if any — enrichment passes
    /// that read source files (docs, git history, syn re-parse) need it.
    fn workspace_root(&self) -> Option<&Path>;
}
