# Dogfood evidence — `check-prelude-triggers` v0.0.1

AC-7 target-dogfood capture per RFC-034 v3.3 §4.2 + cfdb issue #127.

**Consumer repo:** `qbot-core`
**Target commit:** `815c96c10` — `refactor(#3997,#4005,#4006): Phase 3 — signal_spec.rs dismantle`
**Diff footprint:** 13 files across `domain-strategy/`, `ports-strategy/`, `adapters/strategy-registry/`, `adapters/postgres/`, `use-cases/`
**Run date:** 2026-04-20

## Per-trigger results

| Trigger | Fired | Evidence file | Notes |
|---|---|---|---|
| C1 | ✅ | `c1.json` | 2 contexts touched: `infrastructure` + `signals` |
| C3 | ✅ | `c3.json` | 2 port paths: `ports-strategy/src/lib.rs`, `…/signal_spec_parser.rs` |
| C7 | ✅ | `c7.json` | 6 financial-precision paths (domain-strategy + ports-strategy) |
| C8 | ⚫ | `c8.json` | Only `signal` stage touched — correctly silent |
| C9 | ⚫ | `c9.json` | Workspace `Cargo.toml` not in diff — correctly silent |

Envelope fields verified on every file:
- `schema_version == "v1"` ✅
- `from_ref`, `to_ref` present ✅
- `triggers_fired` is a JSON array of uppercase strings (or empty) ✅
- `evidence.<TRIGGER_ID>` present for every ran trigger ✅
- **No per-trigger boolean sibling fields anywhere** (Forbidden move #3 compliance verified)

## Reproducing

```bash
git -C <qbot-core-worktree> diff --name-only 815c96c10^..815c96c10 > /tmp/cp.txt
BIN=target/release/check-prelude-triggers
$BIN --from-ref 815c96c10^ --to-ref 815c96c10 \
    c1-cross-context \
    --context-map <qbot-core>/.cfdb/context-map.toml \
    --changed-paths /tmp/cp.txt
# ...repeat for c3-port-signature, c7-financial-precision, c8-pipeline-stage, c9-workspace-cardinality
```
