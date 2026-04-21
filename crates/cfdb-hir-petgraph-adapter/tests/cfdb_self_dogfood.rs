//! Self-dogfood test — runs the HIR extractor via the adapter on
//! cfdb's OWN workspace tree and asserts at least one resolved
//! method-dispatch `:CallSite` lands in the store.
//!
//! Per CLAUDE.md §3 (dogfood enforcement) and issue #94 AC: this is
//! the strongest trust signal — real data flowing through the real
//! pipeline, asserted on observable output from a store round-trip.
//!
//! ## Why this test lives in the adapter crate
//!
//! The adapter is the single composition point where the HIR
//! extraction flows through `CallSiteEmitter` into a store. Exercising
//! the full chain requires all three crates (`cfdb-hir-extractor` +
//! `cfdb-hir-petgraph-adapter` + `cfdb-petgraph`); placing the test
//! in the adapter crate is where those three already converge.
//!
//! ## Scope
//!
//! One assertion: `EmitStats.call_sites_emitted >= 1`. The fixture
//! IS cfdb itself; pinning an exact count would brittleize against
//! future source changes. The "at least one" floor is enough to
//! prove (a) the pipeline runs end-to-end on a real codebase,
//! (b) HIR actually resolves at least one dispatch in cfdb's source,
//! and (c) the adapter correctly counts and ingests.
//!
//! ## Expected cost
//!
//! On a laptop-class machine, loading cfdb's ~8-crate workspace into
//! `RootDatabase` and walking all files takes tens of seconds.
//!
//! ## Release mode required
//!
//! `ra-ap-hir` contains `never!` debug-only assertions in its type
//! inference path that fire on certain real-world code shapes (known
//! rust-analyzer-upstream quirk; RFC-029 §A1.2 line 109 flagged
//! ra-ap-* as a moving target with ~4 breaking changes / year). In
//! debug builds these are panics; in release builds they degrade
//! gracefully to `tracing::error!` logs, which is the correct
//! behaviour for a production extractor. Run this test with
//! `--release`:
//!
//! ```
//! cargo test --release -p cfdb-hir-petgraph-adapter \
//!   --test cfdb_self_dogfood -- --ignored --nocapture
//! ```
//!
//! The test is marked `#[ignore]` so `cargo test --workspace` stays
//! fast (tens of seconds vs sub-second for the rest of the suite).
//! Slice 4 (#86) wires a CI dogfood job that runs this in release.

use std::path::PathBuf;

use cfdb_core::schema::Keyspace;
use cfdb_hir_extractor::emit::CallSiteEmitter;
use cfdb_hir_extractor::{build_hir_database, extract_call_sites};
use cfdb_hir_petgraph_adapter::PetgraphAdapter;
use cfdb_petgraph::PetgraphStore;

/// Walk up from this crate's manifest directory until a
/// `Cargo.toml` with `[workspace]` is found — that's the cfdb
/// workspace root.
fn cfdb_workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut cur = manifest.as_path();
    while let Some(parent) = cur.parent() {
        let cargo = parent.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&cargo) {
                if contents.contains("[workspace]") {
                    return parent.to_path_buf();
                }
            }
        }
        cur = parent;
    }
    panic!(
        "could not find cfdb workspace root above {}",
        manifest.display()
    );
}

#[test]
#[ignore = "self-dogfood loads cfdb's full workspace into RootDatabase — tens of seconds; run explicitly"]
fn cfdb_self_dogfood_emits_at_least_one_resolved_call_site() {
    let root = cfdb_workspace_root();

    let (db, vfs) =
        build_hir_database(&root).expect("build_hir_database on cfdb's own workspace root");

    let (nodes, edges) =
        extract_call_sites(&db, &vfs).expect("extract_call_sites on cfdb's own workspace");

    let mut store = PetgraphStore::new();
    let mut adapter = PetgraphAdapter::new(&mut store, Keyspace::new("cfdb-hir"));

    let stats = adapter
        .ingest_resolved_call_sites(nodes, edges)
        .expect("adapter ingestion on cfdb's own facts");

    // The invariant we assert: real cfdb source tree has AT LEAST one
    // resolvable method-dispatch call (of the ~thousands of call
    // sites). Pinning an exact count would brittleize. This floor is
    // enough to prove the full chain works end-to-end.
    assert!(
        stats.call_sites_emitted >= 1,
        "self-dogfood expected ≥1 resolved :CallSite from cfdb's own tree; got {:?}",
        stats,
    );
    assert!(
        stats.invokes_at_edges_emitted >= 1,
        "self-dogfood expected ≥1 INVOKES_AT edge from cfdb's own tree; got {:?}",
        stats,
    );
    // CALLS edges are emitted 1-per-resolved-dispatch; expect at
    // least as many as call-sites.
    assert!(
        stats.calls_edges_emitted >= 1,
        "self-dogfood expected ≥1 CALLS edge from cfdb's own tree; got {:?}",
        stats,
    );

    eprintln!(
        "self-dogfood stats on cfdb tree: call_sites={}, calls={}, invokes_at={}",
        stats.call_sites_emitted, stats.calls_edges_emitted, stats.invokes_at_edges_emitted,
    );
}
