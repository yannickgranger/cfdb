# Spec: cfdb-cli

The `cfdb` binary — clap subcommand dispatcher that wraps `cfdb-extractor` + `cfdb-petgraph` + `cfdb-query` behind the 16-verb API surface ratified in RFC-029. Library types re-exported through `lib.rs` so integration tests can call command logic directly.

## CfdbCliError

Typed error enum returned by every cfdb-cli command handler. Wraps upstream errors (`ExtractError`, `StoreError`, `ParseError`, `std::io::Error`, `serde_json::Error`) as named variants plus a `Usage(String)` escape hatch for runtime-validation failures. Landed in PR #38 under RFC-031 §7.

## EnrichVerb

Selector for the four enrichment subcommands (`enrich-docs`, `enrich-metrics`, `enrich-history`, `enrich-concepts`). Lets one handler function service all four CLI variants without duplicating the load-store-print boilerplate.

## HirExtractError

Error returned by the `hir` feature's `extract_and_ingest_hir` composition (Issue #86 / slice 4). Wraps either a `cfdb_hir_extractor::HirError` or a `cfdb_core::store::StoreError`. Only compiled under `cfdb-cli`'s `hir` Cargo feature; default builds never see this type. Surfaced by `cfdb extract --hir --workspace <path>` and mapped to a `CfdbCliError::Usage` string at the CLI boundary.

## TriggerId

Editorial-drift trigger identifier used by the `cfdb check --trigger <ID>` verb (qbot-core council-4046 Phase 2 naming). A closed enum (currently just `T1`; `T3` reserved for issue #102). `TriggerId::variants()` is the single source of truth for valid values — the `FromStr` impl iterates it and the `UnknownTriggerId::Display` impl enumerates it, so the valid-values list in parse-error strings never diverges from the enum (global `CLAUDE.md` §7 MCP/CLI boundary-fix AC).

## UnknownTriggerId

Parse error for `TriggerId::from_str`. Carries the rejected input string so the `Display` impl can produce a `unknown TriggerId 'X' — valid values: T1, …` message whose valid-values list is derived live from `TriggerId::variants()` (no hardcoded enumeration). Returned by clap's `value_parser!(TriggerId)` wiring; the CLI dispatcher maps it to `CfdbCliError::Usage` at the boundary.
