#![cfg(feature = "runner")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use cfdb_recall::{
    adapters::{extractor, ground_truth},
    compute_recall, AuditList, PublicItem, DEFAULT_THRESHOLD,
};

fn cfdb_workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("cfdb workspace root — two parents up from cfdb-recall/")
        .to_path_buf()
}

fn cfdb_core_manifest() -> PathBuf {
    cfdb_workspace_root().join("crates/cfdb-core/Cargo.toml")
}

fn cached_public_set() -> &'static BTreeSet<PublicItem> {
    static CACHE: OnceLock<BTreeSet<PublicItem>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let manifest = cfdb_core_manifest();
        ground_truth::build_public_api_for_manifest(&manifest)
            .expect("rustdoc-json + public-api succeed on cfdb-core")
    })
}

fn cached_extracted_by_crate() -> &'static BTreeMap<String, BTreeSet<PublicItem>> {
    static CACHE: OnceLock<BTreeMap<String, BTreeSet<PublicItem>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let workspace = cfdb_workspace_root();
        extractor::extract_and_project(&workspace)
            .expect("cfdb-extractor succeeds on cfdb workspace")
    })
}

fn cached_cfdb_core_extracted() -> BTreeSet<PublicItem> {
    cached_extracted_by_crate()
        .get("cfdb-core")
        .cloned()
        .unwrap_or_default()
}

#[test]
fn full_pipeline_against_cfdb_core() {
    let extracted = cached_cfdb_core_extracted();
    let public = cached_public_set();

    let report = compute_recall(
        "cfdb-core",
        public,
        &extracted,
        &AuditList::new(),
        DEFAULT_THRESHOLD,
    );

    eprintln!("── cfdb-core recall report ──────────────────");
    eprintln!("  total public items  : {}", report.total_public);
    eprintln!("  adjusted denominator: {}", report.adjusted_denominator);
    eprintln!("  matched             : {}", report.matched);
    eprintln!("  missing count       : {}", report.missing.len());
    if let Some(r) = report.recall() {
        eprintln!("  recall              : {:.2}%", r * 100.0);
    } else {
        eprintln!("  recall              : vacuous (empty denominator)");
    }
    if !report.missing.is_empty() {
        eprintln!("  first 15 missing items:");
        for item in report.missing.iter().take(15) {
            eprintln!("    - {}", item.qname);
        }
    }

    assert!(
        report.total_public > 0,
        "public-api must find at least one item in cfdb-core \
         (cfdb-core is a non-empty library) — got zero, pipeline broken"
    );
    assert!(
        !extracted.is_empty(),
        "cfdb-extractor must emit at least one item for cfdb-core — \
         got zero, pipeline broken"
    );
    assert!(
        report.matched > 0,
        "qname normalization must produce at least one set intersection — \
         got zero, naming convention divergence between extractor and public-api"
    );

    let recall = report
        .recall()
        .expect("cfdb-core has items, denominator must be > 0");
    assert!(
        recall >= 0.50,
        "cfdb-core recall unexpectedly low at {:.2}% — pipeline may be \
         broken or cfdb-extractor has regressed. missing count = {}",
        recall * 100.0,
        report.missing.len()
    );
}

#[test]
fn gate_fails_cleanly_when_extracted_set_has_a_synthetic_gap() {
    let mut extracted = cached_cfdb_core_extracted();
    let public = cached_public_set();

    let victim: PublicItem = extracted
        .iter()
        .find(|it| public.contains(*it))
        .expect("extracted ∩ public is non-empty in the baseline pipeline run")
        .clone();
    assert!(
        extracted.remove(&victim),
        "precondition: victim must be removable from extracted set"
    );

    let report = compute_recall("cfdb-core", public, &extracted, &AuditList::new(), 1.0);

    assert!(
        !report.passes(),
        "gate must reject a run with recall < 1.0 at threshold 1.0; got recall {:?}",
        report.recall()
    );
    assert_eq!(
        report.missing.len(),
        1,
        "missing count must be exactly the number of synthetic gaps injected (1)"
    );
    assert_eq!(
        report.missing[0], victim,
        "the reported missing item must be the synthetic victim"
    );
}

#[test]
fn audit_list_carves_synthetic_gap_end_to_end() {
    let mut extracted = cached_cfdb_core_extracted();
    let public = cached_public_set();

    let victim: PublicItem = extracted
        .iter()
        .find(|it| public.contains(*it))
        .expect("extracted ∩ public is non-empty")
        .clone();
    assert!(extracted.remove(&victim));

    let audit = AuditList::from_items([victim.clone()]);

    let audited = compute_recall("cfdb-core", public, &extracted, &audit, 1.0);

    assert!(
        audited.passes(),
        "after carve-out, gate must pass at threshold 1.0; got recall {:?}",
        audited.recall()
    );
    assert!(
        audited.missing.is_empty(),
        "after carve-out, missing vector must be empty; got {:?}",
        audited.missing
    );
    assert_eq!(
        audited.audited,
        vec![victim.clone()],
        "the audited list must be exactly the carved-out victim"
    );
    let baseline = compute_recall("cfdb-core", public, &extracted, &AuditList::new(), 1.0);
    assert_eq!(
        audited.adjusted_denominator,
        baseline.adjusted_denominator - 1,
        "denominator must drop by exactly 1 (the one audited item)"
    );
}
