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
use cfdb_recall::{compute_recall, AuditList, PublicItem, RecallReport, DEFAULT_THRESHOLD};

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

    /// Recall threshold in the range [0.0, 1.0]. Defaults to 0.95 per
    /// RFC-029 §13 Item 2. Raising above 0.95 requires editing the
    /// constant in `lib.rs` and a reviewed PR.
    #[arg(long, default_value_t = DEFAULT_THRESHOLD)]
    threshold: f64,

    /// Where to write the human-readable gap report. If omitted, no file
    /// is written; the summary still goes to stdout.
    #[arg(long)]
    gaps_file: Option<PathBuf>,
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
        let report = compute_recall(crate_name, &public, &extracted, &audit, cli.threshold);
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
