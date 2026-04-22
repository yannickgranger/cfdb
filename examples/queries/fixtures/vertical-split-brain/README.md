# vertical-split-brain fixture

Synthetic Rust crate tree that reproduces the **Pattern B resolver-fork
vertical split-brain** shape documented in
`docs/RFC-cfdb.md` §A1.3 and targeted by issue #44.

## What this fixture reproduces

The target shape is qbot-core #2651 (compound-stop param drop): a user
invokes one entry point, two resolver methods for the same concept
(`StopLoss`) sit on reachable call chains, and each resolver accepts a
different input type with different defaults. When the entry point wires
only one of the two resolvers, the other becomes an unused parallel path
— and any downstream change has a 50% chance of touching the wrong one.

## Fixture layout

```
vertical-split-brain/
  Cargo.toml                   — workspace manifest
  vsb_fixture/
    Cargo.toml                 — member crate
    src/lib.rs                 — the whole scenario in one file
```

The fixture is a single-crate workspace. Everything compiles without
external dependencies (no axum, no clap, no rmcp, no tokio) so
`build_hir_database` can lower it in seconds.

## The scenario (one-file walkthrough)

- `struct Cli` carries `#[derive(Parser)]` → HIR emits an `:EntryPoint`
  with `kind = "cli_command"` that `EXPOSES` the `Cli` item. Per
  current HIR conventions `EXPOSES` points at the `Cli` struct itself;
  `Cli::handle` is reachable from `Cli` through `CALLS`.
- `impl Cli { pub fn handle(&self, engine: &Engine) { ... } }` — the
  command body. Calls `engine.build_stop(...)`.
- `struct Engine` with `build_stop(&self, raw: &RawStop)` that
  calls **both** resolvers in sequence — the Pattern B shape:
  - `StopLoss::from_bps(raw.bps)` — resolver A
  - `StopLoss::from_pct(raw.pct)` — resolver B
  - both reachable from the single `Cli` `:EntryPoint` through
    `Engine::build_stop`.
- `struct StopLoss { ... }` with two impl methods:
  - `pub fn stop_from_bps(bps: u32) -> StopLoss`  — concept prefix `stop_`, suffix `bps`
  - `pub fn stop_from_pct(pct: u32) -> StopLoss`  — concept prefix `stop_`, suffix `pct`
  - Both match the `^(\w+)_(from|to|for|as)_(\w+)$` resolver-name
    shape the MVP rule keys on (requires a non-empty concept prefix
    before the `from|to|for|as` keyword).
  - Shared concept prefix: `stop_from_` (via
    `regexp_extract(name, '^(\w+)_(?:from|to|for|as)_')`). Differ on
    suffix (`bps` vs `pct`).

## Expected cypher rule output

Running
`examples/queries/vertical-split-brain.cypher`
against the facts extracted from this fixture is expected to produce
exactly **one** row:

| column             | value |
|--------------------|-------|
| `entry_point`      | `Cli` |
| `entry_qname`      | (qname of the `Cli` struct) |
| `concept_prefix`   | `stop_from_` |
| `resolver_a_qname` | `vsb_fixture::StopLoss::stop_from_bps` |
| `resolver_b_qname` | `vsb_fixture::StopLoss::stop_from_pct` |
| `divergence_kind`  | `fork` |

The ordering invariant `a.qname < b.qname` pins `stop_from_bps` as
the A-side and `stop_from_pct` as the B-side deterministically.

## Why this fixture is intentionally simple

The fixture's job is to prove the *query rule detects the shape*, not
to benchmark the extractor. Adding more resolvers (a third `from_usd`,
a fourth `from_percent`) would multiply the expected-row count by
`n_choose_2` — useful later as a `scar_2651_resolver_count_scales`
test but a distraction for the baseline assertion.

Related bug reproductions targeted by later scar tests:
- qbot-core #3522 (pair-resolution) — two resolvers named
  `pair_from_alias` / `pair_from_symbol`
- qbot-core #3545 (`build_resolved_config` 3-way scatter) — three
  resolvers, expected row count = 3 (pairs)
- qbot-core #3654 (7 split resolution points) — seven resolvers,
  expected row count = 21 (pairs)

For this initial slice the fixture is the #2651 shape only. Sibling
scar tests extending the fixture live in the same integration-test
file (`crates/cfdb-petgraph/tests/pattern_b_vertical_split_brain.rs`).
