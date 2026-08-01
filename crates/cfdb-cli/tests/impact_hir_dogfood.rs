//! Impact dogfood over cfdb's own HIR-resolved call graph — RFC-047a slice
//! 47-A (#489), §3.3.
//!
//! ## Why a separate, feature-gated file (not in `impact_seed_binding.rs`)
//!
//! `impact_seed_binding.rs` (47-0) pins the canonical query's *shape* against
//! a hand-injected fixture, isolating it from extractor stability. This file
//! is the complementary *dogfood*: it runs the same `cfdb_query::impact_query`
//! composer against a keyspace built from cfdb's OWN resolved `Item→Item
//! CALLS` graph, proving the blast-radius query works end-to-end on real data.
//!
//! Resolved cross-crate `CALLS` edges exist only on the HIR path
//! (`cfdb_hir_extractor`); the syn-based `extract_workspace` emits only
//! `INVOKES_AT` call sites + stub nodes (RFC-047a §3.3). The HIR path pulls
//! `ra_ap_*`, whose cold compile is 90–150s — over CI's 5-min budget. So this
//! file is gated `#![cfg(feature = "integration-live")]`: a default
//! `cargo test -p cfdb-cli` neither compiles it nor pulls `ra_ap_*`. Run it
//! explicitly:
//!
//! ```text
//! cargo test -p cfdb-cli --features integration-live --test impact_hir_dogfood
//! ```
//!
//! (The issue's `Tests:` block names `#[cfg_attr(not(feature =
//! "integration-live"), ignore)]`; that form would still COMPILE the file —
//! and pull `ra_ap_*` — into every CI test build, defeating its own stated
//! budget rationale. The file-level `#![cfg(...)]` is the repo's established
//! idiom for HIR-heavy tests, e.g. `classifier_taxonomy.rs` /
//! `pattern_c_canonical_bypass.rs`, and is the mechanism that actually keeps
//! `ra_ap_*` out of default CI.)
#![cfg(feature = "integration-live")]

use std::collections::BTreeSet;

use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_hir_extractor::emit::CallSiteEmitter;
use cfdb_hir_extractor::{build_hir_database, extract_call_sites};
use cfdb_hir_petgraph_adapter::PetgraphAdapter;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::impact_query;

mod common;

/// A foundational cfdb-core qname helper with many resolved callers across the
/// workspace. Its qname is the DEFINING-module path (`…::node_id::…`), not the
/// `cfdb_core::qname` re-export — the HIR extractor keys items by definition
/// site (`call_site_emitter::naming::function_qname`).
const SEED: &str = "cfdb_core::qname::node_id::item_node_id";

/// A stable DIRECT caller: `resolve_callee_to_item` calls
/// `item_node_id(callee_path)` (`attr_call_resolution.rs`). Its presence in the
/// blast radius proves a real cross-crate (cfdb-core ← cfdb-petgraph) resolved
/// `Item→Item CALLS` edge — exactly what the syn `extract_workspace` cannot
/// produce (RFC-047a §3.3).
const KNOWN_DIRECT_CALLER: &str =
    "cfdb_petgraph::enrich::attr_call_resolution::resolve_callee_to_item";

/// Build a keyspace populated with cfdb-self's resolved `Item→Item CALLS`
/// graph, in-process (no shell-out) — the §3.3-prescribed path:
/// `build_hir_database` → `extract_call_sites` → ingest via the adapter.
fn resolved_calls_keyspace() -> (PetgraphStore, Keyspace) {
    let root = common::workspace_root();
    // `true` = the production `--hir` policy (`proc_macros = !no_proc_macro`,
    // default-on; `extract.rs`). It enables the sysroot proc-macro server so
    // ra-analyzer's type inference resolves cfdb's full workspace the same way
    // `cfdb extract --hir` does. The syn-only path (`false`) takes a different
    // inference route that hits an upstream `ra_ap_hir_ty` cast ICE on cfdb's
    // tree — so matching production here is load-bearing, not cosmetic.
    let (db, vfs, _proc_macro_client, targets) =
        build_hir_database(&root, true).expect("build HIR database for cfdb-self");
    let (nodes, edges) =
        extract_call_sites(&db, &vfs, &root, &targets).expect("resolve cfdb-self call sites");

    let keyspace = Keyspace::new("impact-hir-dogfood");
    let mut store = PetgraphStore::new();
    let mut adapter = PetgraphAdapter::new(&mut store, keyspace.clone());
    adapter
        .ingest_resolved_call_sites(nodes, edges)
        .expect("ingest resolved call sites into the dogfood keyspace");
    (store, keyspace)
}

/// Collect the affected-set (transitive callers) of a single seed qname.
fn blast_radius(store: &PetgraphStore, ks: &Keyspace, seed: &str) -> BTreeSet<String> {
    let query = impact_query(&[seed], None);
    store
        .execute(ks, &query)
        .expect("execute impact query against the HIR keyspace")
        .rows
        .iter()
        .filter_map(|row| {
            row.get("qname")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect()
}

/// The dogfood: the canonical `impact_query` over cfdb's OWN HIR-resolved
/// `Item→Item CALLS` graph returns the real cross-crate blast radius of a
/// changed cfdb-core leaf.
///
/// Cost: this builds cfdb-self's full resolved call graph in-process
/// (`build_hir_database` + `extract_call_sites`) — ~26 min wall-clock and
/// ~6.5 GB RSS, producing ~246k resolved `CALLS` edges. That is why it is
/// `#![cfg(feature = "integration-live")]`-gated and excluded from default CI.
#[test]
fn impact_over_hir_calls_finds_cross_crate_blast_radius() {
    let (store, ks) = resolved_calls_keyspace();
    let blast = blast_radius(&store, &ks, SEED);

    // The seed is a foundational helper — it must have resolved callers. An
    // empty set here means the HIR path produced no `Item→Item CALLS` (the
    // 47-A pre-condition), not that the impact query is wrong.
    assert!(
        !blast.is_empty(),
        "seed `{SEED}` must have resolved callers in the HIR CALLS graph"
    );

    // A specific, stable DIRECT caller in cfdb-petgraph — proves a real
    // cross-crate resolved CALLS edge reached at depth 1.
    assert!(
        blast.contains(KNOWN_DIRECT_CALLER),
        "known direct cfdb-petgraph caller `{KNOWN_DIRECT_CALLER}` must be in the blast radius; got {blast:?}"
    );

    // Cross-crate TRANSITIVE reach: the unbounded reverse `*1..` traversal must
    // walk the call graph past one crate boundary into cfdb-cli — the capability
    // syn's `extract_workspace` cannot deliver (it emits no resolved CALLS).
    assert!(
        blast.iter().any(|q| q.starts_with("cfdb_cli::")),
        "the unbounded reverse traversal must reach cfdb-cli callers transitively; got {blast:?}"
    );
}
