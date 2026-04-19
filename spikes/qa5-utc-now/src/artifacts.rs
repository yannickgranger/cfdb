//! Artifact writers — TSV (per-file) and Markdown (classification report).
//!
//! Separated from `main.rs` because the Markdown template is a large
//! string literal that dominated the orchestration logic. Keeping it
//! here lets `main.rs` read as "steps 1-8 of the spike", and this file
//! read as "this is the exact shape of the committed artifact".

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::{FileStats, Totals};

pub fn write_tsv(path: &Path, per_file: &BTreeMap<String, FileStats>) {
    let mut buf = String::new();
    buf.push_str(
        "path\trg\tcs_prod\tcs_test\tsub_call\tsub_fn_ptr\tsub_serde\tsub_string\tsub_comment\ta\tb\tc\tcomment\n",
    );
    for (p, s) in per_file {
        buf.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            p,
            s.rg_lines,
            s.cs_prod,
            s.cs_test,
            s.sub_call,
            s.sub_fn_ptr,
            s.sub_serde_attr,
            s.sub_string_lit,
            s.sub_comment,
            s.a,
            s.b,
            s.c,
            s.comment
        ));
    }
    fs::write(path, buf).expect("write tsv");
}

pub fn write_markdown(path: &Path, t: &Totals) {
    let prod_scope = t.prod_scope();
    let pct = if prod_scope > 0 {
        t.a as f64 / prod_scope as f64 * 100.0
    } else {
        0.0
    };
    let gate_verdict = if pct >= 95.0 {
        "**PASS** — syn is sufficient, no `ra-ap-hir` promotion"
    } else {
        "**FAIL** — promote `ra-ap-hir` into v0.1 scope"
    };

    let md = format!(
        r#"# QA-5 spike — Utc::now() classification

Issue: [#3623](https://agency.lab:3000/yg/qbot-core/issues/3623)
RFC: §13 Item 7 (QA-5 contingency), §10 syn ceiling
Produced by: `.concept-graph/cfdb/spikes/qa5-utc-now/` (spike-qa5-utc-now binary)

## Methodology

Denominator is the exact AC command:

```
rg -n 'Utc::now' crates/ | wc -l  = {denominator}
```

Every rg line is classified into exactly one of four buckets: `(a)`, `(b)`,
`(c)`, or a rg false positive (the occurrence is inside a `//` comment). The
per-file allocation is in `qa-5-per-file.tsv` alongside this file.

**Prod scope** = all rg lines in a prod-pathed file that are NOT in comments.
This is the `(a) + (b)` denominator for the recall gate. A prod-pathed file
is any `.rs` file under `crates/` whose path does not match the test-scope
path heuristic (`_tests.rs`, `/tests/`, `/benches/`, `/examples/`, `bdd_*`).

The cfdb-extractor run used to measure `(a)` is:

- binary: `.concept-graph/cfdb/target/release/cfdb` (feat/cfdb-v01-hardening)
- query: `MATCH (cs:CallSite) WHERE cs.callee_path =~ '.*Utc::now' RETURN cs.file, cs.is_test`
- extractor features:
  - `bccb307fb` — honors `#[path]`, walks macro bodies (`vec!`, `assert_eq!`, etc.)
  - QA-5 spike extension — emits `kind="fn_ptr"` CallSites for `ExprPath`
    arguments to `ExprCall`/`ExprMethodCall` (catches `.unwrap_or_else(Utc::now)`)
  - QA-5 spike extension — emits `kind="serde_default"` CallSites for
    `#[serde(default = "path")]` attributes on struct fields

## Sub-class distribution (diagnostic only)

rg matches do not distinguish "real call expression" from "path reference" or
"string-literal mention". The spike applies regex subclassification to the
matched lines so the reader can see what kinds of Utc::now usages exist in
the tree. These counts do NOT feed the (a)/(b) allocation directly — every
non-comment subclass is part of prod scope and cfdb's recall is measured
against the aggregate.

| Sub-class | Shape | Count |
|---|---|---:|
| call       | `Utc::now()` — plain call expression | **{sub_call}** |
| fn_ptr     | `.unwrap_or_else(Utc::now)` — path as fn-pointer arg | **{sub_fn_ptr}** |
| serde_attr | `#[serde(default = "Utc::now")]` — attribute callback | **{sub_serde}** |
| string_lit | Inside a string literal | **{sub_string}** |
| comment    | rg false positive — occurrence is inside `//` comment | {comment} |

## Result

| Bucket | Definition | Count |
|---|---|---:|
| (a) | prod scope lines covered by a cfdb CallSite (`syn` visible) | **{a}** |
| (b) | prod scope residual — extractor blind spot | **{b}** |
| (c) | test-scope (`_tests.rs`, `tests/`, `benches/`, `examples/`, inline `#[cfg(test)]`) | **{c}** |
| — | rg false positive (comment) | {comment} |
| **Σ** | must equal rg denominator | **{sum}** |

## Gate calculation — RFC §13 Item 2 (Q1=(b) Pattern D recall)

The AC predicates the syn-recall target on `(a) ≥ 95% of total prod`. The
honest denominator is `prod_scope = (a) + (b)`, which is every non-comment
non-test-file rg line. Post-QA-5-extension the extractor emits CallSites for
direct calls, macro-body calls, fn-pointer refs, and serde default attrs,
so ALL four prod sub-classes are in scope for recall measurement.

| Ratio | Value | Meaning |
|---|---:|---|
| `(a) / prod_scope` = `{a} / {prod_scope}` | **{pct:.2}%** | syn recall on all prod Utc::now usages |

**Gate verdict:** {gate_verdict}

## Decision — `ra-ap-hir` promotion

{decision_body}

## Reproducing

```bash
cd .concept-graph/cfdb && cargo build --release -p cfdb-cli
cd spikes/qa5-utc-now && cargo build --release
./target/release/spike-qa5-utc-now \
    ../../target/release/cfdb \
    $(git rev-parse --show-toplevel) \
    $(git rev-parse --show-toplevel)/.concept-graph/cfdb/spikes
```

The binary is pure and deterministic: it re-extracts qbot-core facts via the
`cfdb` binary and re-walks `crates/` via the `ignore` crate (the library
backing ripgrep, so line counts match `rg -n 'Utc::now' crates/ | wc -l`),
then writes this file and `qa-5-per-file.tsv` next to itself. Diff the
committed artifact against a re-run as a regression test.
"#,
        denominator = t.denominator,
        a = t.a,
        b = t.b,
        c = t.c,
        comment = t.comment,
        sum = t.a + t.b + t.c + t.comment,
        sub_call = t.sub_call,
        sub_fn_ptr = t.sub_fn_ptr,
        sub_serde = t.sub_serde_attr,
        sub_string = t.sub_string_lit,
        prod_scope = prod_scope,
        pct = pct,
        gate_verdict = gate_verdict,
        decision_body = decision_body(t, pct),
    );
    fs::write(path, md).expect("write md");
}

fn decision_body(t: &Totals, pct: f64) -> String {
    let mut out = String::new();
    if pct >= 95.0 {
        out.push_str(
            "syn-visible recall on prod scope is ≥ 95%. The RFC §13 Item 2 Pattern D \
target is MET by the v0.1 extractor (post-`bccb307fb` macro walking and \
`#[path]` honoring, plus QA-5 spike extension for fn-pointer refs and serde \
default attributes). **`ra-ap-hir` stays in v0.2**; do NOT promote it.\n\n",
        );
    } else {
        out.push_str(
            "syn-visible recall on prod scope is BELOW 95%. The RFC §13 Item 7 \
contingency triggers: `ra-ap-hir` MUST be promoted from v0.2 into v0.1 \
scope, and the Phase A cost estimate revised upward.\n\n",
        );
    }

    if t.b > 0 {
        out.push_str(&format!(
            "Residual `(b) = {}` prod lines remain uncovered. These are cfdb \
CallSite misses on walked files — worth investigating as v0.2 follow-ups but \
not load-bearing for the `ra-ap-hir` decision because the residual is \
comfortably inside the 5% tolerance.\n",
            t.b
        ));
    } else {
        out.push_str(
            "`(b) = 0` — after the QA-5 spike extractor extension, every prod-scope \
Utc::now line is covered by at least one cfdb CallSite. syn recall is 100%.\n",
        );
    }

    out
}
