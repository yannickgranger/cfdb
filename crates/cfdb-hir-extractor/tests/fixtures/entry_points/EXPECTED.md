# v0.2-1 coverage gate — expected entry points

Ground-truth manifest for `tests/v02_1_coverage.rs` (Issue #126). Each row
below is an `:EntryPoint` the HIR extractor MUST emit when run against
this fixture workspace. The test asserts ≥95% recall per kind; with the
closed set below that effectively requires **100% recall** (any miss
rounds below the threshold and fails the test).

Qnames follow the canonical `<crate_name>::<module_path>::<item_name>`
formula shared with production (`item_qname` in `cfdb-core`). Fixture
crate names use `_` rather than `-` so the source literal and emitted
qname match character-for-character.

## Expected entry points — closed set, 11 rows pinned by v0.2-1 coverage gate

The v0.2-1 coverage gate (`tests/v02_1_coverage.rs`) pins exactly these
11 rows from `cli_fx` / `mcp_fx` / `http_fx` / `cron_fx` / `ws_fx`. The
RFC-042 `test_bench_fx` fixture (added in slice 042-A) adds additional
expected emissions covered by `tests/test_bench_entry.rs`, NOT by the
v0.2-1 gate — keeping the v0.2-1 closed set stable for #126's recall math.

| # | Kind | Qname | Source | Key property |
|---|---|---|---|---|
| 1 | `mcp_tool` | `mcp_fx::echo` | `mcp_fx/src/lib.rs:14` | `#[tool]` bare |
| 2 | `mcp_tool` | `mcp_fx::ping` | `mcp_fx/src/lib.rs:21` | `#[rmcp::tool]` namespaced |
| 3 | `cli_command` | `cli_fx::RunCmd` | `cli_fx/src/lib.rs:18` | `#[derive(Parser)]` struct |
| 4 | `cli_command` | `cli_fx::Verb` | `cli_fx/src/lib.rs:25` | `#[derive(Subcommand)]` enum |
| 5 | `http_route` | `http_fx::list_users` | `http_fx/src/lib.rs:21` | axum `.route("/users", …)` |
| 6 | `http_route` | `http_fx::show_user` | `http_fx/src/lib.rs:26` | axum `.get("/users/:id", …)` |
| 7 | `http_route` | `http_fx::health` | `http_fx/src/lib.rs:64` | actix `web::resource("/health").route(web::get().to(…))` |
| 8 | `cron_job` | `cron_fx::register_minute_job` | `cron_fx/src/lib.rs:20` | `Job::new_async("0 * * * * *", …)` |
| 9 | `cron_job` | `cron_fx::install_hourly` | `cron_fx/src/lib.rs:26` | `Job::new("0 0 * * * *", …)` |
| 10 | `websocket` | `ws_fx::chat_handler` | `ws_fx/src/lib.rs:22` | `.on_upgrade(chat_handler)` — named fn |
| 11 | `websocket` | `ws_fx::mount_inline` | `ws_fx/src/lib.rs:32` | `.on_upgrade(|socket| { … })` — closure, falls back to enclosing fn |

## RFC-042 042-A — `test_bench_fx` expected entry points (8 rows)

Covered by `tests/test_bench_entry.rs` (not by the v0.2-1 gate). Assertions
match on `(kind, name)` rather than full qname to stay stable across
ra_ap_load_cargo's integration-test-target qname formula.

| # | Kind | Name | Source | Key property |
|---|---|---|---|---|
| 12 | `test` | `test_plain` | `test_bench_fx/tests/integration.rs:5` | `#[test]` attribute |
| 13 | `test` | `test_tokio` | `test_bench_fx/tests/integration.rs:9` | `#[tokio::test]` — last segment `test` |
| 14 | `test` | `test_helper` | `test_bench_fx/tests/integration.rs:13` | file-location fallback under `tests/` |
| 15 | `mcp_tool` | `helper_with_tool` | `test_bench_fx/tests/integration.rs:17` | `#[tool]` precedence over `tests/` location |
| 16 | `test` | `step_given` | `test_bench_fx/tests/bdd.rs:5` | `#[given]` (cucumber BDD) |
| 17 | `test` | `step_when` | `test_bench_fx/tests/bdd.rs:8` | `#[when]` (cucumber BDD) |
| 18 | `test` | `step_then` | `test_bench_fx/tests/bdd.rs:11` | `#[then]` (cucumber BDD) |
| 19 | `bench` | `bench_one` | `test_bench_fx/benches/bench.rs:5` | `#[bench]` attribute |
| 20 | `bench` | `bench_helper` | `test_bench_fx/benches/bench.rs:9` | file-location fallback under `benches/` |

Control row (must NOT emit):
- `test_bench_fx::library_fn` (`src/lib.rs`) — plain `pub fn`, lives in `src/`, no attribute. The file-location detector correctly rejects it (no `tests/` ancestor before its owning Cargo.toml).

## Recall math

- `mcp_tool`: 2 expected → `ceil(0.95 × 2) = 2` required (100%)
- `cli_command`: 2 expected → `ceil(0.95 × 2) = 2` required (100%)
- `http_route`: 3 expected → `ceil(0.95 × 3) = 3` required (100%)
- `cron_job`: 2 expected → `ceil(0.95 × 2) = 2` required (100%)
- `websocket`: 2 expected → `ceil(0.95 × 2) = 2` required (100%)

With these counts, 95% rounds to full recall for every kind. The gate
fails loudly on a single missing qname (AC-4) rather than degrading
silently.

## Control (must-NOT-emit) entries

The test also asserts none of these leak as entry points (regression
surface for false positives):

- `mcp_fx::unrelated_helper` — no `#[tool]` attribute
- `cli_fx::UnrelatedConfig` — no clap derive
- `http_fx::unrelated_handler` — never wired into a route registration
- `cron_fx::unrelated_setup` — no `Job::new*` call
- `ws_fx::unrelated_ws_helper` — no `.on_upgrade(...)` call

## Note on "compilable"

"Compilable" here means loadable by `ra_ap_load_cargo::load_workspace_at`
— which is what the HIR extractor actually consumes. This matches the
contract already established by `tests/entry_point.rs` and
`tests/http_route.rs`: stand-in types for framework surfaces,
attributes like `#[tool]` and `#[derive(Parser)]` left unresolved
because the scan is syntactic. Full `cargo check` / `cargo build` is
NOT a fixture requirement (and would require pulling real
`clap` / `rmcp` / `axum` / `actix-web` / `tokio_cron_scheduler`
crates — precisely what the fixture pattern is designed to avoid).
