# Spec: cfdb-cli

The `cfdb` binary — clap subcommand dispatcher that wraps `cfdb-extractor` + `cfdb-petgraph` + `cfdb-query` behind the 16-verb API surface ratified in RFC-029. Library types re-exported through `lib.rs` so integration tests can call command logic directly.

## CfdbCliError

Typed error enum returned by every cfdb-cli command handler. Wraps upstream errors (`ExtractError`, `StoreError`, `ParseError`, `std::io::Error`, `serde_json::Error`) as named variants plus a `Usage(String)` escape hatch for runtime-validation failures. Landed in PR #38 under RFC-031 §7.

## EnrichVerb

Selector for the four enrichment subcommands (`enrich-docs`, `enrich-metrics`, `enrich-history`, `enrich-concepts`). Lets one handler function service all four CLI variants without duplicating the load-store-print boilerplate.

## HirExtractError

Error returned by the `hir` feature's `extract_and_ingest_hir` composition (Issue #86 / slice 4). Wraps either a `cfdb_hir_extractor::HirError` or a `cfdb_core::store::StoreError`. Only compiled under `cfdb-cli`'s `hir` Cargo feature; default builds never see this type. Surfaced by `cfdb extract --hir --workspace <path>` and mapped to a `CfdbCliError::Usage` string at the CLI boundary.

## NoProducerDetected

Error returned by `cfdb extract` when no compiled-in `LanguageProducer` accepts the workspace — the `[]` arm of the dispatcher's match on `available_producers().filter(|p| p.detect(ws))` (RFC-041 §3.4 / Slice 41-C). Carries `workspace: String` (the path the user passed) + `compiled_in: Vec<&'static str>` (the names of producers that WERE compiled in, e.g. `["rust"]` on a default build, `[]` on a slim build). The structured payload lets the user diagnose without re-reading their feature flags: a slim build (`cargo install --no-default-features`) produces `compiled_in: []`, signalling that no `lang-*` feature was selected; a default build hitting an unsupported workspace produces `compiled_in: ["rust"]`, signalling the workspace is not a Rust workspace. Mapped via `#[from]` into `CfdbCliError::NoProducer`. Defined at `crates/cfdb-cli/src/lang.rs:75`.

## OutputFormat

Canonical `--format` flag enum used by every cfdb subcommand that accepts an output-format selector. Replaces the per-handler split (`enum DiffFormat`, `enum ClassifyFormat`, three raw `match format { ... }` blocks) that EPIC #273 Pattern 1 #4 surfaced as cfdb-internal split-brain. Variants are `Text`, `Json`, `SortedJsonl`, `Table` — wire strings (`"text"`, `"json"`, `"sorted-jsonl"`, `"table"`) are stable round-trip via `FromStr` ↔ `as_wire()`. Each handler narrows to its accepted subset via `OutputFormat::require_one_of(&[..], cmd)`, which produces the unified `"<cmd>: --format `<got>` not supported; expected `<a>` or `<b>`"` rejection message. Pure value type — no I/O, no schema impact (the enum lives in `cfdb-cli` only; wire strings are not part of `SchemaVersion`).

## PredicateRow

One row of a `cfdb check-predicate` result — mirrors the canonical three-column `(qname, line, reason)` format emitted by `cfdb violations` so consumer skills can parse both with the same code path (RFC-034 §3.5). `qname` is a fully-qualified name (or a file path, for `:File`-subject predicates); `line` is the 1-based source line number, or `0` for subjects that do not have a line (e.g. `:Crate`, `:File`); `reason` is the human-readable violation description from the predicate's `RETURN … AS reason` clause. Derives `Ord` so `PredicateRunReport::rows` can be sorted ascending by `(qname, line)` before serialization — determinism invariant §4.1. Landed in RFC-034 Slice 3 / #147.

## PredicateRunReport

Report of one `cfdb check-predicate` invocation — carries `predicate_name` (bare CLI name), `predicate_path` (absolute path of the loaded `.cypher` file), `row_count` (scalar used by the dispatch layer's exit-code contract — `> 0` → process exit 1), and `rows: Vec<PredicateRow>` sorted ascending by `(qname, line)`. Serialized to stdout when the caller passes `--format json`; the library-API return type for programmatic consumers (integration tests, future skill adapters) that read `rows` directly without parsing stdout. Landed in RFC-034 Slice 3 / #147.

## TriggerId

Editorial-drift trigger identifier used by the `cfdb check --trigger <ID>` verb (qbot-core council-4046 Phase 2 naming). A closed enum (currently just `T1`; `T3` reserved for issue #102). `TriggerId::variants()` is the single source of truth for valid values — the `FromStr` impl iterates it and the `UnknownTriggerId::Display` impl enumerates it, so the valid-values list in parse-error strings never diverges from the enum (global `CLAUDE.md` §7 MCP/CLI boundary-fix AC).

## UnknownTriggerId

Parse error for `TriggerId::from_str`. Carries the rejected input string so the `Display` impl can produce a `unknown TriggerId 'X' — valid values: T1, …` message whose valid-values list is derived live from `TriggerId::variants()` (no hardcoded enumeration). Returned by clap's `value_parser!(TriggerId)` wiring; the CLI dispatcher maps it to `CfdbCliError::Usage` at the boundary.
