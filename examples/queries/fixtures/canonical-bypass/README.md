# canonical-bypass fixture

Synthetic Cargo workspace that reproduces the four Pattern C verdicts
(RFC-029 v0.2 §A1.4) in a single extract. Used by the scar test at
[`crates/cfdb-cli/tests/pattern_c_canonical_bypass.rs`](../../../crates/cfdb-cli/tests/pattern_c_canonical_bypass.rs).

## Layout

```
canonical-bypass/
├── Cargo.toml                      # workspace manifest
├── .cfdb/concepts/ledger.toml      # declares `ledger` concept + canonical crate
├── ledger/src/lib.rs               # LedgerRepository + LedgerService
└── cli/src/lib.rs                  # :EntryPoint (#[tool] fn) + fan-out
```

## Concept declaration

`.cfdb/concepts/ledger.toml` names `ledger` as the concept and the
`ledger` crate as canonical. After `cfdb enrich-concepts` runs, every
`:Item` in the `ledger` crate carries a
`(:Item)-[:CANONICAL_FOR]->(:Concept {name:"ledger"})` edge.

## Expected query output per verdict

Parameters common to the bypass rules:

```json
{
  "concept": "ledger",
  "bypass_callee_name": "append",
  "canonical_callee_name": "append_idempotent",
  "caller_regex": ".*::LedgerService::.*"
}
```

| Rule file | Expected rows | Which service method |
|---|---|---|
| `canonical-bypass-caller.cypher` | 2 | `record_trade_safe`, `record_isolated` |
| `canonical-bypass-reachable.cypher` | 1 | `record_trade` |
| `canonical-bypass-dead.cypher` | 1 | `record_orphan` |
| `canonical-unreachable.cypher` | ≥1 | every `:Item` in the `ledger` crate with `reachable_from_entry=false` that carries `CANONICAL_FOR`; at minimum `record_isolated`, `record_orphan`, `build_entries`, `LedgerRepository`, `LedgerService`, and test helpers are candidates. The scar test asserts `record_isolated` is present. |

Note: CANONICAL_UNREACHABLE is crate-wide because `enrich_concepts`
emits CANONICAL_FOR edges per-crate (not per-item pattern). The
`LedgerRepository::append_idempotent` and `build_entries` items are both
unreached by the CLI and both carry CANONICAL_FOR, so they also
surface — which is the *correct* signal: a canonical declaration that
no entry point uses IS the shape the rule hunts for.

## Reproduced qbot-core issues

| Issue | Shape | Verdict |
|---|---|---|
| #3525 | `LedgerService::record_trade` invokes `.append()` instead of `.append_idempotent()` | BYPASS_REACHABLE |
| #3544 / #3545 / #3546 | `build_resolved_config` / `parse_params` scatter — a bypass caller stranded behind an unwired helper | BYPASS_DEAD (via `record_orphan`) |
| #1526 | `LiveTradingService` safety envelope exists but is wired around instead of through | CANONICAL_UNREACHABLE (via `record_isolated`) |

## Running locally

```
cd examples/queries/fixtures/canonical-bypass
cargo build --release -p cfdb-cli --features hir --manifest-path ../../../../Cargo.toml

../../../../target/release/cfdb extract --workspace . --db .cfdb/db --keyspace fixture --hir
../../../../target/release/cfdb enrich-concepts     --db .cfdb/db --keyspace fixture --workspace .
../../../../target/release/cfdb enrich-reachability --db .cfdb/db --keyspace fixture

# NOTE: `cfdb violations` does not yet accept `--params`; use `cfdb query`
# until that flag lands. Reading the rule via `$(cat ...)` passes the
# cypher in as the positional arg.
PARAMS='{"concept":"ledger","bypass_callee_name":"append","canonical_callee_name":"append_idempotent","caller_regex":".*::LedgerService::.*"}'
for rule in ../canonical-bypass-caller.cypher ../canonical-bypass-reachable.cypher ../canonical-bypass-dead.cypher ../canonical-unreachable.cypher; do
    echo "=== $rule ==="
    ../../../../target/release/cfdb query \
        --db .cfdb/db --keyspace fixture \
        --params "$PARAMS" \
        "$(cat "$rule")"
done
```
