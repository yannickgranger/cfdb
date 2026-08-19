use std::path::Path;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::schema::Keyspace;
use cfdb_hir_extractor::emit::{CallSiteEmitter, EmitStats};
use cfdb_hir_extractor::{build_hir_database, extract_call_sites, extract_entry_points, HirError};
use cfdb_hir_petgraph_adapter::PetgraphAdapter;
use cfdb_petgraph::PetgraphStore;

pub fn extract_and_ingest_hir(
    store: &mut PetgraphStore,
    keyspace: &Keyspace,
    workspace_root: &Path,
    proc_macros: bool,
) -> Result<EmitStats, HirExtractError> {
    eprintln!(
        "extract --hir: loading HIR database for {}",
        workspace_root.display()
    );
    let (db, vfs, _proc_macro_client, targets) =
        build_hir_database(workspace_root, proc_macros).map_err(HirExtractError::Hir)?;
    eprintln!(
        "extract --hir: proc-macros {}",
        match (proc_macros, _proc_macro_client.is_some()) {
            (true, true) => "active",
            (true, false) => "requested but unavailable (syn-only fallback)",
            (false, _) => "disabled (--no-proc-macro)",
        }
    );
    eprintln!(
        "extract --hir: {} lib/bin target roots correlated (RFC-054 54-C)",
        targets.len()
    );

    eprintln!("extract --hir: resolving call sites");
    let (mut nodes, mut edges) =
        extract_call_sites(&db, &vfs, workspace_root, &targets).map_err(HirExtractError::Hir)?;

    eprintln!("extract --hir: scanning entry points");
    let (mut ep_nodes, mut ep_edges) =
        extract_entry_points(&db, &vfs, workspace_root, &targets).map_err(HirExtractError::Hir)?;

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

#[derive(Debug, thiserror::Error)]
pub enum HirExtractError {
    #[error("hir: {0}")]
    Hir(#[source] HirError),

    #[error("store: {0}")]
    Store(#[source] cfdb_core::store::StoreError),
}
