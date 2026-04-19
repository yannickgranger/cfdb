//! `cfdb extract --hir` HIR pipeline wiring — only compiled when
//! the `hir` feature is enabled (Issue #86 / slice 4).
//!
//! Default `cargo build -p cfdb-cli` does NOT include this module,
//! keeping the 90-150s `ra-ap-*` cold compile cost out of every
//! CLI build per RFC-032 §3 lines 221-227. Enable with:
//!
//! ```text
//! cargo build -p cfdb-cli --features hir
//! ```

use std::path::Path;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::schema::Keyspace;
use cfdb_hir_extractor::emit::{CallSiteEmitter, EmitStats};
use cfdb_hir_extractor::{build_hir_database, extract_call_sites, extract_entry_points, HirError};
use cfdb_hir_petgraph_adapter::PetgraphAdapter;
use cfdb_petgraph::PetgraphStore;

/// Run the HIR pipeline on `workspace_root` and ingest the resulting
/// `:CallSite` / `CALLS` / `INVOKES_AT` / `:EntryPoint` / `EXPOSES`
/// facts into `store` under `keyspace`.
pub fn extract_and_ingest_hir(
    store: &mut PetgraphStore,
    keyspace: &Keyspace,
    workspace_root: &Path,
) -> Result<EmitStats, HirExtractError> {
    eprintln!(
        "extract --hir: loading HIR database for {}",
        workspace_root.display()
    );
    let (db, vfs) = build_hir_database(workspace_root).map_err(HirExtractError::Hir)?;

    eprintln!("extract --hir: resolving call sites");
    let (mut nodes, mut edges) = extract_call_sites(&db, &vfs).map_err(HirExtractError::Hir)?;

    eprintln!("extract --hir: scanning entry points");
    let (mut ep_nodes, mut ep_edges) =
        extract_entry_points(&db, &vfs).map_err(HirExtractError::Hir)?;

    // Combine the two fact batches so the adapter sees one ingest.
    // Stable ordering is already guaranteed by each extractor's
    // internal sort — concatenation preserves per-label groups.
    let mut combined_nodes: Vec<Node> = Vec::with_capacity(nodes.len() + ep_nodes.len());
    combined_nodes.append(&mut nodes);
    combined_nodes.append(&mut ep_nodes);

    let mut combined_edges: Vec<Edge> = Vec::with_capacity(edges.len() + ep_edges.len());
    combined_edges.append(&mut edges);
    combined_edges.append(&mut ep_edges);

    let mut adapter = PetgraphAdapter::new(store, keyspace.clone());
    let stats = adapter
        .ingest_resolved_call_sites(combined_nodes, combined_edges)
        .map_err(HirExtractError::Store)?;

    eprintln!(
        "extract --hir: {} :CallSite, {} CALLS, {} INVOKES_AT, {} :EntryPoint, {} EXPOSES",
        stats.call_sites_emitted,
        stats.calls_edges_emitted,
        stats.invokes_at_edges_emitted,
        stats.entry_points_emitted,
        stats.exposes_edges_emitted,
    );

    Ok(stats)
}

/// Error type for the HIR pipeline wrapper — narrows `HirError` +
/// `StoreError` to a single variant the CLI maps to
/// [`crate::CfdbCliError`].
#[derive(Debug, thiserror::Error)]
pub enum HirExtractError {
    #[error("hir: {0}")]
    Hir(#[source] HirError),

    #[error("store: {0}")]
    Store(#[source] cfdb_core::store::StoreError),
}
