# Spec: cfdb-petgraph

The `StoreBackend` implementation backed by `petgraph::StableDiGraph`. The only concrete graph store shipped with cfdb v0.1. Depends on `cfdb-core`; no other workspace dependency.

## PetgraphStore

The concrete `StoreBackend` implementor. Holds one `StableDiGraph` per keyspace, keyed by `Keyspace`. The five determinism guarantees (G1–G5 in RFC-029 §6) are implemented here.

## KeyspaceFile

The on-disk persistence envelope for a serialised keyspace. Wraps the canonical JSON dump with a schema-version header so the loader can detect version mismatches before touching the graph.

## IndexSpec

Parsed `.cfdb/indexes.toml` — the set of `(Label, prop)` or `(Label, computed-key)` pairs that the build pass (RFC-035 slice 2) should materialise into an inverted posting-list index at ingest time. Owned by `cfdb-petgraph` per RFC-035 R1 B1 — backend-optimisation artefact, not a stable abstraction over `cfdb-core`. Missing `.cfdb/indexes.toml` yields an empty spec (no error).

## IndexEntry

A single `[[index]]` TOML row. Two shapes — plain prop (`label` + `prop` + `notes`) or computed key (`label` + `computed` + `notes`). The `notes` string is required and documents the rationale per RFC-035 R1 R2; an entry missing it is rejected at parse time.

## ComputedKey

The closed `const`-sized allowlist of pure functions that may be used as a computed index key. v0.1 ships only `LastSegment` (`last_segment(qname)`). Each variant wraps a canonical `cfdb-core::qname::*` helper (RFC-035 §3.3); extending the allowlist is an RFC-gated change per RFC-035 §3.4.

## UnknownComputedKey

Error raised when an `indexes.toml` `computed = "…"` string is not in the `ComputedKey` allowlist. Carries the offending string verbatim so the parse error can name the rejected key.

## IndexSpecLoadError

Error returned by `IndexSpec::from_path` and `IndexSpec::from_toml_str`. Distinguishes filesystem errors (`Io`) from TOML parse failures (`Toml`) including missing required fields, both-set `prop`+`computed`, and unknown computed keys.

## ExplainRow

One observability row emitted by `PetgraphStore::execute_explained` (RFC-035 slice 7 / #186). Carries the rendered `(var:Label)` pattern string and a `hit: ExplainHit` tag naming whether the evaluator's `candidate_nodes` invocation was satisfied through the `by_prop` fast path or fell back to a full label scan. Stable side-band from `QueryResult` — no explain rows leak into the canonical dump or the keyspace wire format, preserving the RFC-035 §4 determinism invariant. The renderer (`format_line`) is the stable contract consumed by `cfdb scope --explain` dogfood tests.

## ExplainHit

The closed two-variant enum tagging one `ExplainRow`. `Indexed` means the slice-5/6 `by_prop` fast path fired; `Fallback` means the evaluator used `nodes_with_label` (or `all_nodes_sorted` for label-less patterns). Dogfood tests grep on the arrow-form rendering (`→ indexed` / `→ fallback`) so both variants are load-bearing test primitives for self-dogfood + target-dogfood hit-rate measurements.

## AstSignals

Per-function AST-derived signal pair: `{ unwrap_count, cyclomatic }`. Produced by `cfdb_petgraph::enrich::metrics::ast_signals` (issue #203 / RFC-036 §3.3) when the `quality-metrics` feature is active. `unwrap_count` counts `.unwrap()` + `.expect()` method calls in the function body; `cyclomatic` is McCabe complexity (branches + 1) counting `if` / `match` (N arms → N−1) / loops / `?` / `&&` / `||`. Stateless full re-walk of every distinct source file referenced by a `:Item{kind:"Fn"}.file` prop — no incremental-parse mode. DIP constraint (RFC-036 CP6): parses syn directly from within cfdb-petgraph; dep direction `cfdb-petgraph → cfdb-extractor` is forbidden.

## Config (enrich::metrics)

Per-run configuration for `enrich_metrics`. Currently one field: `coverage_json: Option<PathBuf>` naming a `cargo llvm-cov --json` output file. `None` leaves `:Item.test_coverage` unpopulated; `Some` populates per-qname from the file's `summary.lines.percent` block. `Default::default()` yields `coverage_json: None` — matches the G6 invariant (test_coverage toolchain-version-scoped, excluded from G1 canonical-dump sha256).

## compute_for_block / compute_for_item (enrich::metrics::ast_signals)

Pure-function entry points exposed for unit testability. `compute_for_block(&syn::Block) -> AstSignals` walks a single block; `compute_for_item(&syn::File, name) -> Option<AstSignals>` locates the first `fn`/method/trait-default with matching `ident` (including nested `impl` / `mod`) and returns its signals. Neither touches the filesystem; `scan_workspace` (crate-private) is the orchestration entry point that reads files.

## compute_dup_cluster_ids / hash_cluster (enrich::metrics::clustering)

`compute_dup_cluster_ids(&[FnItem]) -> BTreeMap<qname, cluster_id>` groups items by `signature_hash` and emits `dup_cluster_id = sha256(lex_sorted(member_qnames).join("\n"))` for clusters of size ≥ 2 (RFC-036 §3.3 CP5). Singletons carry no id — consumers interpret absence as "no structural duplicate found." `hash_cluster(&[String]) -> String` is the inner sha256-hex helper, extracted for unit testability independent of the grouping loop. Output map iteration is `BTreeMap`-ordered for determinism.

## parse_llvm_cov_json (enrich::metrics::coverage)

`parse_llvm_cov_json(&str) -> Result<BTreeMap<qname, ratio>, String>` consumes the `cargo llvm-cov --json` export format, extracting `data[].functions[].summary.lines.percent` and dividing by 100 to yield a [0.0, 1.0] ratio. Subfeature-gated on `llvm-cov`. Unrecognised JSON returns `Err(msg)`; `load_from_path` (crate-private) wraps this with a filesystem read and converts errors into `warnings` on the `EnrichReport`.
