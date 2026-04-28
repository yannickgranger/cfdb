//! `cfdb-recall` — per-crate recall gate for the cfdb-extractor.
//!
//! Usage:
//!
//! ```text
//! cfdb-recall --workspace <cargo-workspace-root> \
//!             --crate <lib-crate-name> [--crate <other>]... \
//!             [--audit-list <path/to/recall-audit.toml>] \
//!             [--threshold 0.95] \
//!             [--gaps-file <path/to/KNOWN_GAPS.md>]
//! ```
//!
//! Exit codes:
//!   0 — every named crate is ≥ threshold (or vacuously passes)
//!   1 — at least one crate is below threshold
//!   2 — usage error, extractor error, or rustdoc build error
//!
//! The binary is deliberately small: it's a CLI wrapper around the pure
//! `cfdb_recall::compute_recall` function and the two adapters. Anything
//! business-rule-shaped lives in the library and is tested there; this
//! file only handles I/O orchestration and exit-code semantics.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use cfdb_recall::adapters::{extractor, ground_truth};
use cfdb_recall::{
    compute_recall, threshold_for_crate, AuditList, PublicItem, RecallReport,
    RECALL_THRESHOLD_TOTAL,
};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Measure cfdb-extractor recall against cargo public-api ground truth."
)]
struct Cli {
    /// Cargo workspace root containing the crates to measure. Passed
    /// verbatim to `cfdb-extractor::extract_workspace`.
    #[arg(long)]
    workspace: PathBuf,

    /// Library crate to measure. Can be repeated. Each name must match
    /// a workspace member that builds rustdoc JSON cleanly.
    #[arg(long = "crate", value_name = "CRATE")]
    crates: Vec<String>,

    /// Audit list file — TOML with a `[[audit]]` array-of-tables, each
    /// entry having `qname` and `reason` fields. See `recall-audit.toml`
    /// next to this crate for the schema.
    #[arg(long)]
    audit_list: Option<PathBuf>,

    /// Recall threshold in the range [0.0, 1.0]. If omitted, the
    /// per-crate threshold is sourced from `threshold_for_crate` in
    /// `cfdb_recall::thresholds` (defaults to
    /// `RECALL_THRESHOLD_PER_CRATE`). Raising the floor requires editing
    /// the constant in `crates/cfdb-recall/src/thresholds.rs` and a
    /// reviewed PR. The PR-time slim build still uses
    /// `DEFAULT_THRESHOLD` (RFC-029 §13 Item 2 = 0.95).
    #[arg(long)]
    threshold: Option<f64>,

    /// Where to write the human-readable gap report. If omitted, no file
    /// is written; the summary still goes to stdout.
    #[arg(long)]
    gaps_file: Option<PathBuf>,

    /// Where to write the machine-readable per-crate + aggregate report
    /// as JSON. Consumed by the nightly Gitea status workflow (#340) to
    /// drive per-crate `recall/<crate>` and aggregate `recall/total`
    /// commit statuses, and uploaded as the `recall-ratios.json`
    /// workflow artifact (AC-2). If omitted, no file is written.
    ///
    /// Schema:
    /// ```json
    /// {
    ///   "schema_version": 1,
    ///   "crates": [
    ///     {
    ///       "name": "cfdb-core",
    ///       "recall": 0.97,
    ///       "threshold": 0.85,
    ///       "passes": true,
    ///       "matched": 97,
    ///       "adjusted_denominator": 100,
    ///       "missing_count": 3
    ///     }
    ///   ],
    ///   "total": {
    ///     "recall": 0.93,
    ///     "threshold": 0.90,
    ///     "passes": true,
    ///     "matched": 350,
    ///     "adjusted_denominator": 376
    ///   }
    /// }
    /// ```
    /// `recall` is `null` for crates with a vacuous (empty) denominator.
    #[arg(long)]
    json_out: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // ── Load audit list (optional) ───────────────────────────
    let audit = match cli.audit_list.as_ref() {
        None => AuditList::new(),
        Some(path) => match load_audit_list(path) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("cfdb-recall: failed to load audit list {path:?}: {e}");
                return ExitCode::from(2);
            }
        },
    };

    // ── Extract items from the workspace ─────────────────────
    let extracted_by_crate = match extractor::extract_and_project(&cli.workspace) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "cfdb-recall: extractor failed on workspace {:?}: {e}",
                cli.workspace
            );
            return ExitCode::from(2);
        }
    };

    // ── Run the gate for each named crate ────────────────────
    let mut reports: Vec<RecallReport> = Vec::new();
    let mut any_failed = false;
    for crate_name in &cli.crates {
        let manifest = cli
            .workspace
            .join("crates")
            .join(crate_name)
            .join("Cargo.toml");
        let public = match ground_truth::build_public_api_for_manifest(&manifest) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("cfdb-recall: ground-truth build failed for {crate_name}: {e}");
                return ExitCode::from(2);
            }
        };
        // The extractor stores the `crate` node prop as the raw Cargo
        // package name (hyphens preserved). Qnames inside the crate
        // normalize hyphens to underscores because that is what rustc's
        // module system does — but the top-level crate key does not.
        let extracted = extracted_by_crate
            .get(crate_name)
            .cloned()
            .unwrap_or_default();
        // Per-crate threshold dispatch (#340): if `--threshold` is
        // omitted, source the floor from `threshold_for_crate` so each
        // crate gets its own const-driven floor. Explicit `--threshold`
        // overrides the dispatch (preserves the v0.1 PR-time
        // 0.95 invocation contract).
        let threshold = cli
            .threshold
            .unwrap_or_else(|| threshold_for_crate(crate_name));
        let report = compute_recall(crate_name, &public, &extracted, &audit, threshold);
        print_report(&report);
        if !report.passes() {
            any_failed = true;
        }
        reports.push(report);
    }

    // ── Write KNOWN_GAPS.md if requested ─────────────────────
    if let Some(path) = cli.gaps_file.as_ref() {
        if let Err(e) = write_gaps_file(path, &reports) {
            eprintln!("cfdb-recall: failed to write gaps file {path:?}: {e}");
            return ExitCode::from(2);
        }
    }

    // ── Write recall-ratios.json if requested (#340) ─────────
    if let Some(path) = cli.json_out.as_ref() {
        if let Err(e) = write_json_out(path, &reports) {
            eprintln!("cfdb-recall: failed to write json-out {path:?}: {e}");
            return ExitCode::from(2);
        }
    }

    if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

fn load_audit_list(path: &std::path::Path) -> Result<AuditList, Box<dyn std::error::Error>> {
    // Minimal TOML-free format: one qname per line, with `#` comments.
    // Keeping this dependency-free avoids pulling `toml` into the crate
    // just for a file we expect to stay under 50 lines.
    let text = std::fs::read_to_string(path)?;
    let items: BTreeSet<PublicItem> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(PublicItem::new)
        .collect();
    Ok(AuditList::from_items(items))
}

fn print_report(report: &RecallReport) {
    let recall_pct = match report.recall() {
        None => "vacuous (0 denominator)".to_string(),
        Some(r) => format!("{:.2}%", r * 100.0),
    };
    let verdict = if report.passes() { "PASS" } else { "FAIL" };
    println!(
        "── {crate} ────────────────────────────────────",
        crate = report.crate_name
    );
    println!("  verdict             : {verdict}");
    println!("  recall              : {recall_pct}");
    println!("  threshold           : {:.2}%", report.threshold * 100.0);
    println!("  total public items  : {}", report.total_public);
    println!("  audited items       : {}", report.audited.len());
    println!("  adjusted denominator: {}", report.adjusted_denominator);
    println!("  matched (numerator) : {}", report.matched);
    println!("  missing             : {}", report.missing.len());
    if !report.missing.is_empty() {
        let head: Vec<&str> = report
            .missing
            .iter()
            .take(10)
            .map(|it| it.qname.as_str())
            .collect();
        println!("  missing (first 10)  : {head:?}");
    }
}

fn write_gaps_file(path: &std::path::Path, reports: &[RecallReport]) -> Result<(), std::io::Error> {
    let mut md = String::new();
    md.push_str("# cfdb recall — KNOWN GAPS\n\n");
    md.push_str("Generated by `cfdb-recall` (RFC-029 §13 acceptance gate Item 2).\n");
    md.push_str(
        "Each entry here is an item that `cargo public-api` reports as part of the \
         public surface but `cfdb-extractor` did not emit. Entries either belong on \
         the audit list (macro-generated) or represent a real syn ceiling that should \
         be fixed or moved to v0.2 / `ra-ap-hir`.\n\n",
    );

    for report in reports {
        md.push_str(&format!("## `{}`\n\n", report.crate_name));
        let recall_pct = match report.recall() {
            None => "vacuous".to_string(),
            Some(r) => format!("{:.2}%", r * 100.0),
        };
        md.push_str(&format!(
            "- recall: **{recall_pct}** (threshold {:.2}%)\n",
            report.threshold * 100.0
        ));
        md.push_str(&format!(
            "- public items: {} (audited {}, adjusted denominator {})\n",
            report.total_public,
            report.audited.len(),
            report.adjusted_denominator
        ));
        md.push_str(&format!(
            "- matched: {} / {}\n\n",
            report.matched, report.adjusted_denominator
        ));

        if !report.missing.is_empty() {
            md.push_str("### Missing (gate-failing)\n\n");
            for item in &report.missing {
                md.push_str(&format!("- `{}`\n", item.qname));
            }
            md.push('\n');
        }

        if !report.audited.is_empty() {
            md.push_str("### Audited (carved out)\n\n");
            for item in &report.audited {
                md.push_str(&format!("- `{}`\n", item.qname));
            }
            md.push('\n');
        }
    }

    std::fs::write(path, md)
}

/// Write the per-crate + aggregate report as JSON for the nightly Gitea
/// status workflow (#340 AC-2 / AC-3). The aggregate threshold is sourced
/// from [`RECALL_THRESHOLD_TOTAL`] in `cfdb_recall::thresholds`; per-crate
/// thresholds are sourced from each [`RecallReport`]'s `threshold` field
/// (which the workflow seeds from [`threshold_for_crate`]).
///
/// The schema is intentionally flat and version-tagged
/// (`schema_version: 1`). Bumping it requires a coordinated edit to the
/// workflow's jq parser.
fn write_json_out(path: &std::path::Path, reports: &[RecallReport]) -> Result<(), std::io::Error> {
    let crates: Vec<serde_json::Value> = reports
        .iter()
        .map(|r| {
            let recall = r.recall();
            serde_json::json!({
                "name": r.crate_name,
                // null when the denominator is empty (vacuous pass) — the
                // workflow distinguishes this from a real failure.
                "recall": recall,
                "threshold": r.threshold,
                "passes": r.passes(),
                "matched": r.matched,
                "adjusted_denominator": r.adjusted_denominator,
                "total_public": r.total_public,
                "missing_count": r.missing.len(),
                "audited_count": r.audited.len(),
            })
        })
        .collect();

    // Aggregate: sum-of-numerators / sum-of-denominators. Crates with a
    // vacuous (zero) denominator are skipped — they neither help nor
    // hurt the aggregate ratio.
    let agg_matched: usize = reports.iter().map(|r| r.matched).sum();
    let agg_denom: usize = reports.iter().map(|r| r.adjusted_denominator).sum();
    let agg_recall: Option<f64> = if agg_denom == 0 {
        None
    } else {
        Some(agg_matched as f64 / agg_denom as f64)
    };
    let agg_passes = match agg_recall {
        None => true, // vacuous — no surface to measure against
        Some(r) => r >= RECALL_THRESHOLD_TOTAL,
    };

    let doc = serde_json::json!({
        "schema_version": 1,
        "crates": crates,
        "total": {
            "recall": agg_recall,
            "threshold": RECALL_THRESHOLD_TOTAL,
            "passes": agg_passes,
            "matched": agg_matched,
            "adjusted_denominator": agg_denom,
        }
    });

    let bytes = serde_json::to_vec_pretty(&doc).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}
