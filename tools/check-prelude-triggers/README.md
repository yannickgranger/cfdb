# check-prelude-triggers

Tier-1 mechanical C-trigger binary per RFC-034 v3.3 §4.2 (qbot-core EPIC #4074 W2.A).

Fires 5 deterministic triggers against a workspace diff:

| Trigger | Concept |
|---|---|
| C1 | cross-context change — diff touches ≥2 bounded contexts per `context-map.toml` |
| C3 | port trait signature — diff touches a file under `crates/ports*/src/` |
| C7 | financial-precision path — diff touches a crate listed in `financial-precision-crates.toml` |
| C8 | pipeline-stage cross — diff touches ≥2 stages per `pipeline-stages.toml` |
| C9 | workspace cardinality — workspace `Cargo.toml` is in the diff |

The binary is stateless: it reads argv-supplied paths and emits a versioned JSON envelope on stdout. See [`src/report.rs`](src/report.rs) for the envelope shape and [`_generated/`](_generated) for dogfood JSON captured against real qbot-core diffs.

## Usage

The canonical entry point is the `all` subcommand — runs all 5 triggers internally and emits one merged envelope:

```bash
check-prelude-triggers \
  --from-ref develop --to-ref work-branch-tip \
  all \
    --context-map qbot-core/.cfdb/context-map.toml \
    --financial-precision-crates qbot-core/.cfdb/financial-precision-crates.toml \
    --pipeline-stages qbot-core/.cfdb/pipeline-stages.toml \
    --workspace-root qbot-core \
    --changed-paths /tmp/changed-paths.txt
```

Per-trigger subcommands remain available for debugging / single-trigger runs:

```bash
check-prelude-triggers \
  --from-ref develop --to-ref work-branch-tip \
  c1-cross-context \
    --context-map qbot-core/.cfdb/context-map.toml \
    --changed-paths /tmp/changed-paths.txt
```

The `all` subcommand was added in cfdb #335 — `/discover` consumes one envelope per RFC-034 §4.2, so the consolidated form removes a 5-call + manual-merge step from skill-side wiring.

### Exit codes (RFC-034 §4.2 rust-systems Amendment 1)

- `0` — success; JSON envelope emitted on stdout (empty `triggers_fired` is valid)
- `1` — usage / argument error (clap parse failure, unknown `--schema-version`, stale refs when `--require-fresh` set)
- `2` — fatal runtime error (TOML parse, IO)

## When refs match

By default, the binary accepts `--from-ref == --to-ref`. The empty-diff envelope (`triggers_fired: []`) is honest output for that input and is the right shape for **issue-start snapshot capture** (e.g. an early `.triggers/<n>.json` taken before any impl work has happened).

That convenience is also a footgun: if a consumer captures the envelope at issue-start and never re-runs the binary against the work-branch tip, `/pre-council` and `/gate-contract` see an under-reporting envelope and may under-react to a money-path change. The W3.A first-run dogfood (qbot-core PR #4167, see `docs/dogfood/v3.3/first-run.md`) caught exactly this against issue #4055.

The `--require-fresh` flag opts a consumer into the stricter contract:

```bash
check-prelude-triggers --require-fresh \
  --from-ref develop --to-ref work-branch-tip \
  c9-workspace-cardinality \
    --workspace-root . \
    --changed-paths /tmp/changed.txt
```

When `--require-fresh` is set AND `--from-ref == --to-ref`, the binary:

- emits no envelope on stdout
- writes `error: from_ref equals to_ref; refresh required (RFC-034 §4.2 lower-bound semantic)` on stderr
- exits with code `1`

The flag is consumer opt-in for backward compatibility — issue-start snapshots remain a valid use case for archaeology / dogfood replay. `/ship` pre-flight is the canonical consumer that should always pass `--require-fresh`.

## Tests

```bash
cargo test -p check-prelude-triggers
```

22 tests across 5 trigger modules + lib helpers + integration tests for `--require-fresh` (1 per subcommand).
