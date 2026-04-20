//! **v0.2-9 accuracy gate** for `enrich_bounded_context` (issue #108 / AC-5).
//!
//! Measures the fraction of `:Item` nodes in three qbot-core ground-truth
//! crates whose extract-time-derived `bounded_context` matches the
//! human-expected label. Synthesis invariant I6 — this gate BLOCKS merge of
//! slice 43-E AND of the downstream classifier (#48).
//!
//! # Ground-truth mapping (hard-coded; crate-prefix heuristic for now)
//!
//! Each qbot-core crate → its expected bounded context as a domain expert
//! would label it. Today the mapping is exactly the `WELL_KNOWN_PREFIXES`
//! heuristic output (strip first `qbot-` prefix); if future qbot-core TOML
//! overrides declare something different, update this map in lockstep.
//!
//! The synthesis R1 §43-E sample list (`domain-strategy`, `ports-trading`,
//! `qbot-mcp`) was written against an older qbot-core crate layout. The
//! current layout renames these to `qbot-strategy`, `qbot-postgres-trading`,
//! `qbot-mcp` (confirmed against workspace membership as of
//! 2026-04-20). The test tracks today's names.
//!
//! # Skip policy
//!
//! The test is `#[ignore]`d by default because it requires qbot-core to be
//! cloned at `/var/mnt/workspaces/qbot-core` — a local filesystem dependency
//! that CI cannot satisfy without network access. Run locally with
//! `cargo test -p cfdb-cli --test v02_9_bounded_context_accuracy -- --ignored
//! --nocapture`. Reviewer sanity-check reads the emitted report in the PR
//! body.

use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::PropValue;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

const QBOT_CORE_PATH: &str = "/var/mnt/workspaces/qbot-core";

/// Ground-truth (crate_name, expected_bounded_context). 3 representative
/// crates spanning domain / infrastructure / entry-point layers — the same
/// axes synthesis R1 §43-E called for. All three exist in current
/// qbot-core and carry real `:Item` counts.
const GROUND_TRUTH: &[(&str, &str)] = &[
    ("qbot-strategy", "strategy"),
    ("qbot-postgres-trading", "postgres-trading"),
    ("qbot-mcp", "mcp"),
];

#[test]
#[ignore = "requires /var/mnt/workspaces/qbot-core checkout; run locally with --ignored"]
fn ac5_v02_9_accuracy_gate_on_qbot_core() {
    let qbot = PathBuf::from(QBOT_CORE_PATH);
    assert!(
        qbot.exists(),
        "v0.2-9: qbot-core workspace not found at {QBOT_CORE_PATH} — \
         clone yg/qbot-core there before running this test"
    );

    // Extract qbot-core + run the enrichment pass.
    let (nodes, edges) = cfdb_extractor::extract_workspace(&qbot).expect("extract qbot-core");
    let mut store = PetgraphStore::new().with_workspace(&qbot);
    let ks = Keyspace::new("qbot");
    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");
    let report = store.enrich_bounded_context(&ks).expect("enrich");
    assert!(
        report.ran,
        "pass must run on qbot-core: {:?}",
        report.warnings
    );

    // For each ground-truth crate, count (total_items, items_matching_expected).
    let (all_nodes, _) = store.export(&ks).expect("export");

    let mut rows: Vec<(&'static str, &'static str, usize, usize)> = Vec::new();
    let mut overall_total = 0usize;
    let mut overall_matches = 0usize;

    for (crate_name, expected_ctx) in GROUND_TRUTH {
        let (total, matches) = count_accuracy(&all_nodes, crate_name, expected_ctx);
        rows.push((*crate_name, *expected_ctx, total, matches));
        overall_total += total;
        overall_matches += matches;
    }

    // Emit the one-page report (AC-5 deliverable).
    eprintln!("\n=== v0.2-9 bounded_context accuracy gate — qbot-core ===");
    eprintln!(
        "{:<24} {:<18} {:>8} {:>8} {:>10}",
        "crate", "expected ctx", "items", "matches", "accuracy"
    );
    for (crate_name, expected, total, matches) in &rows {
        let accuracy = if *total == 0 {
            0.0
        } else {
            (*matches as f64) / (*total as f64)
        };
        eprintln!(
            "{:<24} {:<18} {:>8} {:>8} {:>9.2}%",
            crate_name,
            expected,
            total,
            matches,
            accuracy * 100.0
        );
    }
    let overall = (overall_matches as f64) / (overall_total as f64);
    eprintln!(
        "{:<24} {:<18} {:>8} {:>8} {:>9.2}%",
        "OVERALL",
        "—",
        overall_total,
        overall_matches,
        overall * 100.0
    );
    eprintln!();

    assert!(
        overall >= 0.95,
        "v0.2-9 GATE FAILED: overall accuracy {:.2}% < 95% target. \
         Iterate on heuristic or add TOML overrides in qbot-core until \
         gate passes.",
        overall * 100.0
    );
}

fn count_accuracy(
    nodes: &[cfdb_core::fact::Node],
    crate_name: &str,
    expected_ctx: &str,
) -> (usize, usize) {
    let items: Vec<&cfdb_core::fact::Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .filter(|n| {
            n.props
                .get("crate")
                .and_then(PropValue::as_str)
                .is_some_and(|c| c == crate_name)
        })
        .collect();
    let total = items.len();
    let matches = items
        .iter()
        .filter(|n| {
            n.props
                .get("bounded_context")
                .and_then(PropValue::as_str)
                .is_some_and(|v| v == expected_ctx)
        })
        .count();
    (total, matches)
}

