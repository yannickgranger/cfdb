//! Integration test — dogfood cfdb-recall on `cfdb-core` against real
//! `rustdoc-json` + `public-api` ground truth.
//!
//! **Requires the `runner` feature.** The entire file is gated because
//! every scenario calls [`ground_truth::build_public_api_for_manifest`],
//! which is itself gated behind `runner` to keep rustdoc-json out of the
//! slim-library compile path. Run with
//! `cargo test -p cfdb-recall --features runner` (or `--all-features`).
#![cfg(feature = "runner")]

//!
//! This is the BDD layer for RFC-029 §13 acceptance gate Item 2. Unlike
//! the library unit tests in `src/lib.rs` and `src/adapters/*.rs`, this
//! test executes the ENTIRE pipeline: it runs `cargo +nightly rustdoc
//! --output-format=json` on a real library crate, parses the result with
//! the real `public-api` crate, runs `cfdb-extractor` on a real Cargo
//! workspace, and computes recall with the real pure function. No
//! synthetic inputs. If any layer regresses, this test catches it.
//!
//! ## Why `cfdb-core` is the fixture
//!
//! `cfdb-core` is:
//! - a library crate (so `cargo public-api` has a surface to measure),
//! - inside the same Cargo workspace as `cfdb-recall` (so the extractor
//!   can reach it with the same workspace path this crate uses),
//! - small (~7 files, parses in ~5 seconds),
//! - real production code (not a synthetic fixture whose recall is
//!   gamed — if the extractor has a gap, it shows up here honestly).
//!
//! Dogfooding is deliberate. A synthetic fixture would be faster but
//! would hide the extractor's real behavior on Rust code with traits,
//! impls, re-exports, and generics.
//!
//! ## Runtime cost
//!
//! Invoking `rustdoc-json` builds the target crate in a separate target
//! directory under `~/.cache/cargo-public-api` (or similar). First run is
//! ~10–30 seconds; warm-cache runs are ~5 seconds. The test is NOT
//! `#[ignore]`-gated because Gate 1 (Intent) requires real-infrastructure
//! BDD to run without opt-in flags — otherwise the gate is a lie.
//!
//! ## Nightly requirement
//!
//! `rustdoc --output-format=json` is nightly-only. CI must install a
//! nightly toolchain before invoking `cargo test -p cfdb-recall`. This is
//! an explicit constraint of the chosen ground-truth source.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use cfdb_recall::{
    adapters::{extractor, ground_truth},
    compute_recall, AuditList, PublicItem, DEFAULT_THRESHOLD,
};

/// Resolve the cfdb workspace root from this test crate's location.
/// `CARGO_MANIFEST_DIR` points at `.../cfdb/crates/cfdb-recall`; the
/// workspace root is two levels up.
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

// ── Shared pipeline outputs ──────────────────────────────────────────
//
// All three scenarios build the SAME rustdoc-json + public-api snapshot
// of cfdb-core and run cfdb-extractor over the SAME workspace. `cargo
// test` runs these three functions in parallel by default, and the
// underlying `rustdoc_json::Builder::default().build()` writes to the
// shared `target/doc/cfdb_core.json` path — so N parallel invocations
// race on the file, yielding truncated JSON on readers that catch the
// file mid-write ("EOF while parsing an object" / JsonParse panics).
//
// `OnceLock::get_or_init` serialises this naturally: the first thread
// runs the rustdoc build + extractor once, the other threads block on
// the same cell until the single result is committed, then read it.
// Single rustdoc invocation per test binary, deterministic outcome,
// 3× faster than the racey parallel version.

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

// ── Scenario 1: end-to-end pipeline against cfdb-core ────────────────

/// Given the cfdb workspace and cfdb-core's manifest,
/// When the recall harness runs the real extractor and the real
///   `rustdoc-json` + `public-api` pipeline,
/// Then it produces a non-trivial `RecallReport` whose numbers reflect
///   the actual intersection of the two item sets.
///
/// The assertion is deliberately loose (recall > 0.50, ≥ 1 matched item)
/// because the strict 95% gate is a policy the binary enforces at CI
/// time with an audit list — not a property of `cfdb-core` specifically.
/// The role of this test is to prove the PLUMBING works end-to-end, so a
/// plumbing regression (the extractor returning empty, the public-api
/// parser dropping everything, the qname normalization diverging) is
/// caught regardless of where cfdb-core's real recall lands.
#[test]
fn full_pipeline_against_cfdb_core() {
    // Cached across the three parallel tests — see "Shared pipeline
    // outputs" note at the top of this file. `extracted` is cloned
    // from the shared cache because each test may want a local mut copy.
    let extracted = cached_cfdb_core_extracted();
    let public = cached_public_set();

    // Compute recall with the default threshold and an empty audit list.
    let report = compute_recall(
        "cfdb-core",
        public,
        &extracted,
        &AuditList::new(),
        DEFAULT_THRESHOLD,
    );

    // Print the full report so a human running `cargo test -- --nocapture`
    // can see real numbers. Also written to stderr so proof files capture
    // them when the test runs in CI.
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

    // Plumbing assertions — these verify the pipeline actually worked,
    // not that any particular recall number holds.
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

    // Sanity bound: if recall is below 50%, either cfdb-core has an \
    // extraordinary amount of macro-generated surface (it does not) or \
    // the harness has a real bug. The strict 95% gate is enforced by \
    // the CLI with an audit list; this test just catches plumbing rot.
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

// ── Scenario 2: FAIL path exercised against real infra ─────────────

/// Given a real-pipeline snapshot of cfdb-core's public item set,
///   AND the corresponding real extracted set,
///   AND a synthetic perturbation that drops one item from the extracted set,
/// When the harness runs with threshold = 1.0,
/// Then `passes()` returns false and `missing` contains exactly the
///   removed item.
///
/// The perturbation is how this scenario guarantees the failing path is
/// exercised without relying on cfdb-core having "naturally" missing
/// items. The pipeline inputs are all real — nothing synthetic flows
/// through `rustdoc-json`, `public-api`, or `cfdb-extractor`. The single
/// synthetic step is a `.remove()` call on the produced set, simulating
/// an extractor regression where one item stops being emitted. This is
/// the honest form of a BDD "unhappy path" at the integration level:
/// real data + one controlled perturbation = deterministic failure.
#[test]
fn gate_fails_cleanly_when_extracted_set_has_a_synthetic_gap() {
    // Cached — see "Shared pipeline outputs" note at the top of this file.
    let mut extracted = cached_cfdb_core_extracted();
    let public = cached_public_set();

    // Pick the first item that appears in BOTH sets — that is the one
    // whose removal from `extracted` is guaranteed to create a real
    // "missing" entry, rather than silently doing nothing.
    let victim: PublicItem = extracted
        .iter()
        .find(|it| public.contains(*it))
        .expect("extracted ∩ public is non-empty in the baseline pipeline run")
        .clone();
    assert!(
        extracted.remove(&victim),
        "precondition: victim must be removable from extracted set"
    );

    // Threshold 1.0 — any missing item fails the gate.
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

// ── Scenario 3: audit carve-out exercised against real infra ────────

/// Given the same real-pipeline snapshot as scenario 2,
///   AND the same synthetic perturbation (one dropped extracted item),
///   AND an audit list containing the dropped item,
/// When the harness runs with threshold = 1.0,
/// Then the adjusted denominator excludes the audited item and the gate
///   passes with an empty `missing` vector.
///
/// The shape of this test is a mirror of scenario 2: same inputs, same
/// perturbation, plus an audit list that carves out the victim. This
/// proves the carve-out path is load-bearing end-to-end on real data,
/// not just in the synthetic two-item setups of the unit tests.
#[test]
fn audit_list_carves_synthetic_gap_end_to_end() {
    // Cached — see "Shared pipeline outputs" note at the top of this file.
    let mut extracted = cached_cfdb_core_extracted();
    let public = cached_public_set();

    let victim: PublicItem = extracted
        .iter()
        .find(|it| public.contains(*it))
        .expect("extracted ∩ public is non-empty")
        .clone();
    assert!(extracted.remove(&victim));

    // Audit list carves the victim.
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
    // Denominator shrinks by one compared to a no-audit baseline.
    let baseline = compute_recall("cfdb-core", public, &extracted, &AuditList::new(), 1.0);
    assert_eq!(
        audited.adjusted_denominator,
        baseline.adjusted_denominator - 1,
        "denominator must drop by exactly 1 (the one audited item)"
    );
}
