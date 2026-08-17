//! `GraphView` / `GraphBackend` — the enrichment-pass port, split from the
//! concrete petgraph storage engine per RFC-056.
//!
//! Sibling of [`crate::store::StoreBackend`] and [`crate::enrich::EnrichBackend`]:
//! `StoreBackend` is the CLI/query-facing storage contract, `EnrichBackend`
//! is the enrichment-verb dispatch contract, and `GraphView`/`GraphBackend`
//! is the narrower id-based read/write surface an individual enrichment
//! *pass* needs to walk and mutate one keyspace's graph without depending
//! on the concrete storage representation (`cfdb-petgraph`'s
//! `StableDiGraph`/`NodeIndex`).
//!
//! `GraphView` is per-keyspace (an enrichment pass resolves one keyspace
//! once, per the guard pattern every `EnrichBackend` verb already follows);
//! `GraphBackend` is the per-store factory that resolves a keyspace into a
//! `GraphView`. Both are dyn-safe by design — no generics, no associated
//! types — so a `GraphBackend` implementor can hand out `&mut dyn GraphView`
//! without exposing its concrete graph representation.

use std::path::Path;

use crate::fact::{Edge, Node, PropValue};
use crate::schema::{Direction, EdgeLabel, Keyspace, Label};
use crate::store::StoreError;

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

/// The per-store factory that resolves a keyspace into a [`GraphView`].
///
/// `Send + Sync` mirrors [`crate::store::StoreBackend`] and
/// [`crate::enrich::EnrichBackend`] — required so a generic `EnrichEngine<S:
/// GraphBackend>` can itself be `Send + Sync` (its only field is `&mut S`,
/// so that bound is otherwise unprovable for an unconstrained `S`).
pub trait GraphBackend: Send + Sync {
    /// Resolve `keyspace` into its [`GraphView`]. `Err(StoreError::UnknownKeyspace)`
    /// if the store has never seen this keyspace.
    fn graph_view(&mut self, keyspace: &Keyspace) -> Result<&mut dyn GraphView, StoreError>;

    /// The workspace root attached to this store, if any — enrichment passes
    /// that read source files (docs, git history, syn re-parse) need it.
    fn workspace_root(&self) -> Option<&Path>;
}
