//! Self-dogfood test for `enrich_rfc_docs` (issue #107 — slice 43-D).
//!
//! Extracts cfdb's own source tree, runs `enrich_rfc_docs`, and asserts:
//!
//! - AC-3: `edges_written > 0` — cfdb's own `docs/RFC-*.md` files reference
//!   `EnrichBackend`, `StoreBackend`, `PetgraphStore`, etc.
//! - AC-4: spot check — the `:Item` named `EnrichBackend` has a
//!   `REFERENCED_BY` edge pointing at a `:RfcDoc` whose path is
//!   `docs/RFC-031-audit-cleanup.md`.
//!
//! Uses the library API (no CLI shell-out) so a failure surfaces as a
//! Rust stack trace inside `cargo test`. Workspace root resolved from
//! `CARGO_MANIFEST_DIR` for portability across worktrees + CI runners.

use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

#[test]
fn ac3_ac4_self_dogfood_enrich_backend_references_rfc_031() {
    let workspace = cfdb_workspace_root();

    let (nodes, edges) = cfdb_extractor::extract_workspace(&workspace).expect("extract cfdb");

    let mut store = PetgraphStore::new().with_workspace(&workspace);
    let ks = Keyspace::new("selfdog");
    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let report = store.enrich_rfc_docs(&ks).expect("enrich_rfc_docs");

    assert!(
        report.ran,
        "pass must actually run on a workspace with docs/: {:?}",
        report.warnings
    );
    assert!(
        report.edges_written > 0,
        "AC-3: cfdb's own RFCs reference :Item nodes — expected edges_written > 0, got {}",
        report.edges_written
    );
    assert!(
        report.facts_scanned > 0,
        "AC-3: expected ≥1 markdown file scanned under docs/"
    );

    // AC-4: EnrichBackend → docs/RFC-031-audit-cleanup.md spot-check.
    let (all_nodes, all_edges) = store.export(&ks).expect("export");
    let enrich_backend_id = all_nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && n.props
                    .get("name")
                    .and_then(PropValue::as_str)
                    .is_some_and(|v| v == "EnrichBackend")
        })
        .map(|n| n.id.clone())
        .expect("AC-4: :Item named EnrichBackend must exist in cfdb's own extract");

    let rfc_doc_ids: Vec<&str> = all_edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::REFERENCED_BY && e.src == enrich_backend_id)
        .map(|e| e.dst.as_str())
        .collect();
    assert!(
        !rfc_doc_ids.is_empty(),
        "AC-4: EnrichBackend must have at least one REFERENCED_BY edge"
    );

    let rfc_paths: Vec<String> = all_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::RFC_DOC && rfc_doc_ids.contains(&n.id.as_str()))
        .filter_map(|n| {
            n.props
                .get("path")
                .and_then(PropValue::as_str)
                .map(str::to_string)
        })
        .collect();
    assert!(
        rfc_paths
            .iter()
            .any(|p| p == "docs/RFC-031-audit-cleanup.md"),
        "AC-4: EnrichBackend should reference docs/RFC-031-audit-cleanup.md; got {rfc_paths:?}"
    );

    eprintln!(
        "self-dogfood: facts_scanned={} edges_written={} rfc_nodes≈{}",
        report.facts_scanned,
        report.edges_written,
        rfc_paths.len()
    );
}
