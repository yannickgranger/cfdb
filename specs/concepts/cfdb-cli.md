# Spec: cfdb-cli

The `cfdb` binary — clap subcommand dispatcher that wraps `cfdb-extractor` + `cfdb-petgraph` + `cfdb-query` behind the 16-verb API surface ratified in RFC-029. Library types re-exported through `lib.rs` so integration tests can call command logic directly.

## CfdbCliError

Typed error enum returned by every cfdb-cli command handler. Wraps upstream errors (`ExtractError`, `StoreError`, `ParseError`, `std::io::Error`, `serde_json::Error`) as named variants plus a `Usage(String)` escape hatch for runtime-validation failures. Landed in PR #38 under RFC-031 §7.

## EnrichVerb

Selector for the four enrichment subcommands (`enrich-docs`, `enrich-metrics`, `enrich-history`, `enrich-concepts`). Lets one handler function service all four CLI variants without duplicating the load-store-print boilerplate.
