# dogfood-enrich

RFC-039 self-dogfood harness for the 7 enrichment passes.

This is a CI tool, not a user-facing verb. It reads a `.cfdb/queries/self-enrich-<pass>.cypher` template, substitutes the threshold const (for ratio passes), invokes `cfdb violations` against a materialized tempfile, and maps the result to a 0 / 30 / 1 exit code.

```
dogfood-enrich --pass <name> --db <dir> --keyspace <ks> [--cfdb-bin <path>] [--workspace <path>]
  → exit 0   zero violation rows (invariant holds)
  → exit 30  at least one violation row (invariant violated)
  → exit 1   runtime error: unknown pass, missing template, missing feature (I5.1),
             subprocess fail, JSON parse error
```

`--pass` accepts one of the 7 RFC-039 passes:
- `enrich-deprecation`, `enrich-rfc-docs`, `enrich-bounded-context`, `enrich-concepts` (default-feature, PR-time)
- `enrich-reachability` (`hir`), `enrich-metrics` (`quality-metrics`), `enrich-git-history` (`git-enrich`) (nightly)

## Why a separate binary?

Per RFC-039 §3.5 council ratification:
- **Not a `cfdb` subcommand:** CCP — CI-only policy thresholds change for different reasons than user-facing verbs (extract / scope / violations).
- **Not in `cfdb-cli`:** SAP — `cfdb-cli` is highly efferent (Ce ≫ Ca); placing CI policy there would couple unrelated change-reasons.
- **Standalone leaf binary:** `Ca = 0`, depends only on `cfdb-core` (for `EnrichReport` wire-form parsing) + `clap`/`serde`/`serde_json`/`tempfile`/`thiserror`. Mirrors `tools/check-prelude-triggers/`. Does NOT link `cfdb-cli` as a library (subprocess invocation only).

## Threshold consts

`src/thresholds.rs`. Tightening is a separate reviewed PR per `CLAUDE.md` §6 row 5. **No baseline file. No allowlist file.** A PR proposing one is rejected on sight.

Each of the 7 passes carries a `pub const … _THRESHOLD: Option<u32>`. Four are `Some(N)` (ratio passes); three are `None` (hard-equality sentinels).

| Pass | Const | Value |
|---|---|---|
| `enrich-deprecation` | `DEPRECATION_THRESHOLD` | `None` (hard equality) |
| `enrich-rfc-docs` | `RFC_DOCS_THRESHOLD` | `None` (hard equality) |
| `enrich-bounded-context` | `BC_COVERAGE_THRESHOLD` | `Some(95)` |
| `enrich-concepts` | `CONCEPTS_THRESHOLD` | `None` (hard equality) |
| `enrich-reachability` | `REACHABILITY_THRESHOLD` | `Some(80)` |
| `enrich-metrics` | `METRICS_COVERAGE_THRESHOLD` | `Some(95)` |
| `enrich-git-history` | `GIT_COVERAGE_THRESHOLD` | `Some(95)` |

## I5.1 feature-presence guard

Before running the dogfood sentinel, the binary invokes `cfdb enrich-<pass>` and inspects `EnrichReport.ran` in the JSON output. When `ran == false` (the off-feature dispatch path at `crates/cfdb-petgraph/src/enrich_backend.rs:178-262`), the harness exits 1 with a "feature missing" message — NOT with the sentinel result, because a binary built without the feature would silently report 100% null coverage and look like a real regression.

## See also

- `docs/RFC-039-dogfood-enrichment-passes.md` (ratified 4/4 at R2 via PR #341)
- `ci/dogfood-determinism.sh` — the determinism harness
- `.cfdb/queries/self-enrich-*.cypher` — the templates (added by Issues #343–#349)
