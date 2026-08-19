use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
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
    #[arg(
        help = "Cargo workspace root containing the crates to measure. Passed verbatim to `cfdb-extractor::extract_workspace`"
    )]
    #[arg(long)]
    workspace: PathBuf,

    #[arg(
        help = "Library crate to measure. Can be repeated. Each name must match a workspace member that builds rustdoc JSON cleanly"
    )]
    #[arg(long = "crate", value_name = "CRATE")]
    crates: Vec<String>,

    #[arg(
        help = "Audit list file — TOML with a `[[audit]]` array-of-tables, each entry having `qname` and `reason` fields. See `recall-audit.toml` next to this crate for the schema"
    )]
    #[arg(long)]
    audit_list: Option<PathBuf>,

    #[arg(
        help = "Recall threshold in the range [0.0, 1.0]. If omitted, the per-crate threshold is sourced from `threshold_for_crate` in `cfdb_recall::thresholds` (defaults to `RECALL_THRESHOLD_PER_CRATE`). Raising the floor requires editing the constant in `crates/cfdb-recall/src/thresholds.rs` and a reviewed PR. The PR-time slim build still uses `DEFAULT_THRESHOLD` (default: 0.95)"
    )]
    #[arg(long)]
    threshold: Option<f64>,

    #[arg(
        help = "Where to write the human-readable gap report. If omitted, no file is written; the summary still goes to stdout"
    )]
    #[arg(long)]
    gaps_file: Option<PathBuf>,

    #[arg(
        help = "Where to write the machine-readable per-crate + aggregate report as JSON. Consumed by the nightly Gitea status workflow to drive per-crate `recall/<crate>` and aggregate `recall/total` commit statuses, and uploaded as the `recall-ratios.json` workflow artifact. If omitted, no file is written",
        long_help = "Where to write the machine-readable per-crate + aggregate report as JSON. Consumed by the nightly Gitea status workflow to drive per-crate `recall/<crate>` and aggregate `recall/total` commit statuses, and uploaded as the `recall-ratios.json` workflow artifact. If omitted, no file is written.

Schema: ```json { \"schema_version\": 1, \"crates\": [ { \"name\": \"cfdb-core\", \"recall\": 0.97, \"threshold\": 0.85, \"passes\": true, \"matched\": 97, \"adjusted_denominator\": 100, \"missing_count\": 3 } ], \"total\": { \"recall\": 0.93, \"threshold\": 0.90, \"passes\": true, \"matched\": 350, \"adjusted_denominator\": 376 } } ``` `recall` is `null` for crates with a vacuous (empty) denominator."
    )]
    #[arg(long)]
    json_out: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let audit = match load_audit_or_default(cli.audit_list.as_deref()) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let extracted_by_crate = match extract_workspace_items(&cli.workspace) {
        Ok(m) => m,
        Err(code) => return code,
    };
    let (reports, any_failed) = match gather_crate_reports(&cli, &extracted_by_crate, &audit) {
        Ok(r) => r,
        Err(code) => return code,
    };
    if let Err(code) = emit_optional_outputs(&cli, &reports) {
        return code;
    }

    if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

fn load_audit_or_default(path: Option<&Path>) -> Result<AuditList, ExitCode> {
    let Some(path) = path else {
        return Ok(AuditList::new());
    };
    load_audit_list(path).map_err(|e| {
        eprintln!("cfdb-recall: failed to load audit list {path:?}: {e}");
        ExitCode::from(2)
    })
}

fn extract_workspace_items(
    workspace: &Path,
) -> Result<BTreeMap<String, BTreeSet<PublicItem>>, ExitCode> {
    extractor::extract_and_project(workspace).map_err(|e| {
        eprintln!("cfdb-recall: extractor failed on workspace {workspace:?}: {e}");
        ExitCode::from(2)
    })
}

fn gather_crate_reports(
    cli: &Cli,
    extracted_by_crate: &BTreeMap<String, BTreeSet<PublicItem>>,
    audit: &AuditList,
) -> Result<(Vec<RecallReport>, bool), ExitCode> {
    let mut reports: Vec<RecallReport> = Vec::new();
    let mut any_failed = false;
    for crate_name in &cli.crates {
        let report = build_crate_report(cli, crate_name, extracted_by_crate, audit)?;
        print_report(&report);
        if !report.passes() {
            any_failed = true;
        }
        reports.push(report);
    }
    Ok((reports, any_failed))
}

fn build_crate_report(
    cli: &Cli,
    crate_name: &str,
    extracted_by_crate: &BTreeMap<String, BTreeSet<PublicItem>>,
    audit: &AuditList,
) -> Result<RecallReport, ExitCode> {
    let manifest = cli
        .workspace
        .join("crates")
        .join(crate_name)
        .join("Cargo.toml");
    let public = ground_truth::build_public_api_for_manifest(&manifest).map_err(|e| {
        eprintln!("cfdb-recall: ground-truth build failed for {crate_name}: {e}");
        ExitCode::from(2)
    })?;
    let extracted = extracted_by_crate
        .get(crate_name)
        .cloned()
        .unwrap_or_default();
    let threshold = cli
        .threshold
        .unwrap_or_else(|| threshold_for_crate(crate_name));
    Ok(compute_recall(
        crate_name, &public, &extracted, audit, threshold,
    ))
}

fn emit_optional_outputs(cli: &Cli, reports: &[RecallReport]) -> Result<(), ExitCode> {
    if let Some(path) = cli.gaps_file.as_ref() {
        write_gaps_file(path, reports).map_err(|e| {
            eprintln!("cfdb-recall: failed to write gaps file {path:?}: {e}");
            ExitCode::from(2)
        })?;
    }
    if let Some(path) = cli.json_out.as_ref() {
        write_json_out(path, reports).map_err(|e| {
            eprintln!("cfdb-recall: failed to write json-out {path:?}: {e}");
            ExitCode::from(2)
        })?;
    }
    Ok(())
}

fn load_audit_list(path: &std::path::Path) -> Result<AuditList, Box<dyn std::error::Error>> {
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

fn write_json_out(path: &std::path::Path, reports: &[RecallReport]) -> Result<(), std::io::Error> {
    let crates: Vec<serde_json::Value> = reports
        .iter()
        .map(|r| {
            let recall = r.recall();
            serde_json::json!({
                "name": r.crate_name,
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

    let agg_matched: usize = reports.iter().map(|r| r.matched).sum();
    let agg_denom: usize = reports.iter().map(|r| r.adjusted_denominator).sum();
    let agg_recall: Option<f64> = if agg_denom == 0 {
        None
    } else {
        Some(agg_matched as f64 / agg_denom as f64)
    };
    let agg_passes = match agg_recall {
        None => true,
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
