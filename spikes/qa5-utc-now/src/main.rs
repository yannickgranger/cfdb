//! QA-5 macro spike reconciler — issue #3623.
//!
//! Produces the classification artifact required by issue #3623 AC:
//!
//!   1. rg -n 'Utc::now' crates/ | wc -l  → denominator
//!   2. Each hit classified into (a) syn-visible prod, (b) extractor blind spot
//!      in prod, or (c) test-scope.
//!   3. Compute (a) / (a+b) — the syn-recall ratio on prod. If < 95%, the RFC
//!      §13 Item 2 Pattern D target is unmet and `ra-ap-hir` must be promoted
//!      from v0.2 into v0.1.
//!
//! The reconciler uses the `ignore` crate (the library backing ripgrep) to
//! produce the denominator in-process, and shells out to the already-built
//! `cfdb` binary for CallSite facts. The `ignore` walk honors `.gitignore`
//! and hidden-file rules the same way `rg` does, so the line count matches
//! `rg -n 'Utc::now' crates/ | wc -l`.
//!
//! Inputs (positional args):
//!   1. CFDB_BIN     — path to the built cfdb binary
//!   2. WORKTREE     — absolute path to the qbot-core worktree to analyze
//!   3. OUT_DIR      — where to write classification.md and per-file.tsv
//!
//! This binary is a one-shot spike, not a reusable tool. A reusable reconcile
//! subcommand can be added to `cfdb-cli` in v0.2 if we find ourselves running
//! this against more patterns than just `Utc::now`.

mod artifacts;
mod classify;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use regex::Regex;
use serde::Deserialize;

use crate::artifacts::{write_markdown, write_tsv};
use crate::classify::{classify_line, is_test_path, Subclass};

#[derive(Debug, Deserialize)]
struct QueryResult {
    rows: Vec<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    #[allow(dead_code)]
    warnings: serde_json::Value,
}

#[derive(Debug, Default, Clone)]
pub struct FileStats {
    pub rg_lines: u32,
    pub cs_prod: u32,
    pub cs_test: u32,
    // Subclass tallies of rg lines for this file. Diagnostic only — the
    // allocation logic treats all non-comment non-test lines uniformly as
    // "prod scope" and credits cfdb CallSites against the aggregate. The
    // subclass split is kept in the TSV so a reader can see how the (b)
    // residual is distributed across call / fn_ptr / serde_attr / string.
    pub sub_call: u32,
    pub sub_fn_ptr: u32,
    pub sub_serde_attr: u32,
    pub sub_string_lit: u32,
    pub sub_comment: u32,
    // Final bucket allocation for this file. (a) + (b) + (c) + comment == rg_lines.
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub comment: u32,
}

#[derive(Debug, Default)]
pub struct Totals {
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub comment: u32,
    pub denominator: u32,
    // Aggregate subclass distribution across all 1188 rg lines.
    pub sub_call: u32,
    pub sub_fn_ptr: u32,
    pub sub_serde_attr: u32,
    pub sub_string_lit: u32,
}

impl Totals {
    pub fn prod_scope(&self) -> u32 {
        self.a + self.b
    }
}

fn run_capture(cmd: &mut Command) -> String {
    let out = cmd
        .stderr(Stdio::inherit())
        .output()
        .expect("spawn command");
    if !out.status.success() {
        panic!(
            "command failed: {:?} (exit {})",
            cmd,
            out.status.code().unwrap_or(-1)
        );
    }
    String::from_utf8(out.stdout).expect("stdout utf8")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!(
            "usage: {} <cfdb-bin> <worktree-root> <out-dir>",
            args.first()
                .map(String::as_str)
                .unwrap_or("spike-qa5-utc-now")
        );
        std::process::exit(2);
    }
    let cfdb_bin = PathBuf::from(&args[1]);
    let worktree = PathBuf::from(&args[2])
        .canonicalize()
        .expect("canonicalize worktree");
    let out_dir = PathBuf::from(&args[3]);
    fs::create_dir_all(&out_dir).expect("create out dir");

    assert!(cfdb_bin.exists(), "cfdb binary not found at {:?}", cfdb_bin);

    // ---- Step 1: extract qbot-core into a fresh db ----
    //
    // Scratch lives under the system temp dir so the spike never pollutes
    // the committed tree with intermediate files. The two committed outputs
    // (classification.md + per-file.tsv) go into out_dir.
    let db_dir = tempdir_under(&std::env::temp_dir(), "qa5-spike-db");
    eprintln!("spike-qa5: extracting {}", worktree.display());
    let status = Command::new(&cfdb_bin)
        .args([
            "extract",
            "--workspace",
            worktree.to_str().unwrap(),
            "--db",
            db_dir.to_str().unwrap(),
            "--keyspace",
            "qbot-core",
        ])
        .status()
        .expect("spawn cfdb extract");
    assert!(status.success(), "cfdb extract failed");

    // ---- Step 2: query CallSites matching Utc::now ----
    eprintln!("spike-qa5: querying CallSites");
    let query = "MATCH (cs:CallSite) WHERE cs.callee_path =~ '.*Utc::now' \
                 RETURN cs.file, cs.is_test";
    let stdout = run_capture(Command::new(&cfdb_bin).args([
        "query",
        "--db",
        db_dir.to_str().unwrap(),
        "--keyspace",
        "qbot-core",
        query,
    ]));
    let result: QueryResult = serde_json::from_str(&stdout).expect("parse cfdb query json");
    eprintln!("spike-qa5: {} CallSite rows", result.rows.len());

    // ---- Step 3: ground-truth denominator via the `ignore` crate ----
    //
    // Emulates `rg -n 'Utc::now' crates/`: walks the `crates/` tree honoring
    // .gitignore + hidden-file defaults, reads each regular file, returns the
    // list of (path, line, content) tuples where the needle appears. Paths
    // are made relative to the worktree root so they match cfdb CallSite
    // props after the prefix-strip below.
    eprintln!("spike-qa5: walking crates/ for 'Utc::now' (ignore+regex)");
    let needle = Regex::new(r"Utc::now").unwrap();
    let crates_root = worktree.join("crates");
    let rg_lines = walk_and_match(&crates_root, &worktree, &needle);
    eprintln!("spike-qa5: {} matched lines", rg_lines.len());

    // ---- Step 4: build per-file stats ----
    let mut per_file: BTreeMap<String, FileStats> = BTreeMap::new();

    // rg subclass counts per file.
    for (path, _ln, content) in &rg_lines {
        let entry = per_file.entry(path.clone()).or_default();
        entry.rg_lines += 1;
        match classify_line(content) {
            Subclass::Call => entry.sub_call += 1,
            Subclass::FnPtr => entry.sub_fn_ptr += 1,
            Subclass::SerdeAttr => entry.sub_serde_attr += 1,
            Subclass::StringLit => entry.sub_string_lit += 1,
            Subclass::Comment => entry.sub_comment += 1,
        }
    }

    // cfdb CallSite counts per file — normalize to worktree-relative paths.
    let prefix = format!("{}/", worktree.display());
    for row in &result.rows {
        let file_raw = row
            .get("cs.file")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let rel = file_raw
            .strip_prefix(&prefix)
            .unwrap_or(file_raw)
            .to_string();
        let is_test = row
            .get("cs.is_test")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let entry = per_file.entry(rel).or_default();
        if is_test {
            entry.cs_test += 1;
        } else {
            entry.cs_prod += 1;
        }
    }

    // ---- Step 5: allocate buckets per file ----
    //
    // Prod-scope for a non-test-path file = all rg lines that are not
    // comments: `sub_call + sub_fn_ptr + sub_serde_attr + sub_string_lit`.
    // cfdb's `cs_prod` count is credited against that aggregate — the
    // extractor post-#3623 emits CallSites for fn-pointer refs and serde
    // default attributes with `kind` tagged, so all four subclasses are in
    // scope for recall measurement.
    let mut totals = Totals::default();
    for (path, s) in per_file.iter_mut() {
        totals.denominator += s.rg_lines;
        totals.sub_call += s.sub_call;
        totals.sub_fn_ptr += s.sub_fn_ptr;
        totals.sub_serde_attr += s.sub_serde_attr;
        totals.sub_string_lit += s.sub_string_lit;

        if is_test_path(path) {
            // All lines in a test-scope file are (c).
            s.c = s.rg_lines;
            totals.c += s.c;
            continue;
        }

        // Non-test-path file.
        let prod_scope = s.sub_call + s.sub_fn_ptr + s.sub_serde_attr + s.sub_string_lit;

        // cfdb test CallSites here (inline `#[cfg(test)] mod tests { ... }`
        // in a prod-pathed file) belong to (c). Credit them first against
        // the direct-call subclass since that's where `#[cfg(test)]` bodies
        // live. Non-call subclasses are never inside test modules in
        // practice.
        let call_assigned_test = s.cs_test.min(s.sub_call);
        s.c = call_assigned_test;
        totals.c += s.c;

        let prod_remaining = prod_scope - call_assigned_test;
        let a = s.cs_prod.min(prod_remaining);
        let b = prod_remaining - a;

        s.a = a;
        s.b = b;
        s.comment = s.sub_comment;
        totals.a += a;
        totals.b += b;
        totals.comment += s.comment;
    }

    // ---- Step 6: validate sum ----
    let sum = totals.a + totals.b + totals.c + totals.comment;
    assert_eq!(
        sum, totals.denominator,
        "bucket sum mismatch: (a={}) + (b={}) + (c={}) + (comment={}) = {} vs denominator {}",
        totals.a, totals.b, totals.c, totals.comment, sum, totals.denominator
    );

    // ---- Step 7: write artifacts ----
    let tsv = out_dir.join("qa-5-per-file.tsv");
    write_tsv(&tsv, &per_file);

    let md = out_dir.join("qa-5-utc-now-classification.md");
    write_markdown(&md, &totals);

    // ---- Step 8: summary to stdout (proof file) ----
    println!("== QA-5 spike: classification result ==");
    println!(
        "denominator (rg -n 'Utc::now' crates/ | wc -l) : {}",
        totals.denominator
    );
    println!();
    println!("Sub-class distribution of rg lines (diagnostic):");
    println!("  call              : {}", totals.sub_call);
    println!("  fn_ptr            : {}", totals.sub_fn_ptr);
    println!("  serde_attr        : {}", totals.sub_serde_attr);
    println!("  string_lit        : {}", totals.sub_string_lit);
    println!("  comment           : {}", totals.comment);
    println!();
    println!(
        "(a) syn-visible prod (cfdb CallSite found)     : {}",
        totals.a
    );
    println!(
        "(b) prod residual (extractor blind spot)       : {}",
        totals.b
    );
    println!(
        "(c) test-scope                                 : {}",
        totals.c
    );
    println!(
        "rg false positives (comments)                  : {}",
        totals.comment
    );
    println!();
    let prod_scope = totals.prod_scope();
    let pct = if prod_scope > 0 {
        totals.a as f64 / prod_scope as f64 * 100.0
    } else {
        0.0
    };
    println!(
        "(a) / prod_scope ({} / {}) = {:.2}%",
        totals.a, prod_scope, pct
    );
    println!();
    if pct >= 95.0 {
        println!("DECISION: syn-visible recall ≥95% — syn is SUFFICIENT for RFC §13");
        println!("          Item 2 Pattern D. Do NOT promote ra-ap-hir from v0.2");
        println!("          into v0.1. QA-5 spike PASSES.");
    } else {
        println!(
            "DECISION: syn recall BELOW 95% ({:.2}%) — promote ra-ap-hir into v0.1.",
            pct
        );
    }

    eprintln!("spike-qa5: wrote {}", md.display());
    eprintln!("spike-qa5: wrote {}", tsv.display());
}

fn walk_and_match(root: &Path, worktree: &Path, needle: &Regex) -> Vec<(String, u32, String)> {
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(true)
        .build();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("spike-qa5: walk error: {e}");
                continue;
            }
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel: String = path
            .strip_prefix(worktree)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());
        for (idx, line) in text.lines().enumerate() {
            if needle.is_match(line) {
                out.push((rel.clone(), (idx + 1) as u32, line.to_string()));
            }
        }
    }
    out
}

fn tempdir_under(parent: &Path, prefix: &str) -> PathBuf {
    // Simple deterministic temp dir — this binary is single-shot; we do not
    // need cryptographic randomness.
    let dir = parent.join(format!("{prefix}-scratch"));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean temp");
    }
    fs::create_dir_all(&dir).expect("mkdir temp");
    dir
}
