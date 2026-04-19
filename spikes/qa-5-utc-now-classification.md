# QA-5 spike — Utc::now() classification

Issue: [#3623](https://agency.lab:3000/yg/qbot-core/issues/3623)
RFC: §13 Item 7 (QA-5 contingency), §10 syn ceiling
Produced by: `.concept-graph/cfdb/spikes/qa5-utc-now/` (spike-qa5-utc-now binary)

## Methodology

Denominator is the exact AC command:

```
rg -n 'Utc::now' crates/ | wc -l  = 1188
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
| call       | `Utc::now()` — plain call expression | **1107** |
| fn_ptr     | `.unwrap_or_else(Utc::now)` — path as fn-pointer arg | **53** |
| serde_attr | `#[serde(default = "Utc::now")]` — attribute callback | **1** |
| string_lit | Inside a string literal | **14** |
| comment    | rg false positive — occurrence is inside `//` comment | 9 |

## Result

| Bucket | Definition | Count |
|---|---|---:|
| (a) | prod scope lines covered by a cfdb CallSite (`syn` visible) | **165** |
| (b) | prod scope residual — extractor blind spot | **2** |
| (c) | test-scope (`_tests.rs`, `tests/`, `benches/`, `examples/`, inline `#[cfg(test)]`) | **1012** |
| — | rg false positive (comment) | 9 |
| **Σ** | must equal rg denominator | **1188** |

## Gate calculation — RFC §13 Item 2 (Q1=(b) Pattern D recall)

The AC predicates the syn-recall target on `(a) ≥ 95% of total prod`. The
honest denominator is `prod_scope = (a) + (b)`, which is every non-comment
non-test-file rg line. Post-QA-5-extension the extractor emits CallSites for
direct calls, macro-body calls, fn-pointer refs, and serde default attrs,
so ALL four prod sub-classes are in scope for recall measurement.

| Ratio | Value | Meaning |
|---|---:|---|
| `(a) / prod_scope` = `165 / 167` | **98.80%** | syn recall on all prod Utc::now usages |

**Gate verdict:** **PASS** — syn is sufficient, no `ra-ap-hir` promotion

## Decision — `ra-ap-hir` promotion

syn-visible recall on prod scope is ≥ 95%. The RFC §13 Item 2 Pattern D target is MET by the v0.1 extractor (post-`bccb307fb` macro walking and `#[path]` honoring, plus QA-5 spike extension for fn-pointer refs and serde default attributes). **`ra-ap-hir` stays in v0.2**; do NOT promote it.

Residual `(b) = 2` prod lines remain uncovered. These are cfdb CallSite misses on walked files — worth investigating as v0.2 follow-ups but not load-bearing for the `ra-ap-hir` decision because the residual is comfortably inside the 5% tolerance.


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
