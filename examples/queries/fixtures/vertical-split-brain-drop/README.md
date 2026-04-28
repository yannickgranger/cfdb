# vertical-split-brain-drop fixture

Ground-truth fixture for the `drop` divergence kind of Pattern B vertical-split-brain — the qbot-core #2651 shape that `examples/queries/vertical-split-brain-drop.cypher` targets.

## Shape

- **Wire**: `Cli` (`#[derive(Parser)]`) registers field `stop_atr_mult`
- **Layer K**: `compute_active_mult(stop_atr_mult: f64)` — its `:Param.name` matches the wire key
- **Layer K+1**: `compute_trail_layer(active_mult: f64)` — its `:Param.name` is `active_mult`, which the entry point never registers — **the drop**

Both layers are reachable from `Cli::handle` via `Engine::dispatch`, so the cypher rule's `(handler)-[:CALLS*1..8]->` two-resolver join binds.

## Expected result

Running the rule against this fixture's keyspace should return exactly **1 row** with `divergence_kind = 'drop'`, `wire_param = "stop_atr_mult"`, `divergent_key = "active_mult"`, and `matching_resolver` / `divergent_resolver` pointing at the two `compute_*` fns.

The on-disk fixture is **for human verification of the rule shape** — same convention as the sister `examples/queries/fixtures/vertical-split-brain/` fixture. It does not compile cleanly (the stand-in `Parser` trait + bare `#[arg]` attribute exist for HIR's syntactic scan only, not for `rustc`). HIR call resolution may not connect every CALLS edge end-to-end on a non-compiling tree, so the live `cfdb extract --hir` may not produce a fired row. The regression surface is `crates/cfdb-petgraph/tests/pattern_b_vertical_split_brain_drop.rs`, which uses the proven direct-fact-injection pattern.

## Usage (human verification)

```bash
cargo build --release -p cfdb-cli --bin cfdb --features hir
./target/release/cfdb extract --workspace examples/queries/fixtures/vertical-split-brain-drop \
  --db /tmp/vsb-drop-db --keyspace vsb_drop --hir
./target/release/cfdb query --db /tmp/vsb-drop-db --keyspace vsb_drop \
  "$(cat examples/queries/vertical-split-brain-drop.cypher)"
```

## Refs

- Issue #297 (Phase B — `drop` kind extension)
- `examples/queries/vertical-split-brain-drop.cypher`
- Sister fixture: `examples/queries/fixtures/vertical-split-brain/` (`fork` kind)
- qbot-core scar #2651 — original failure mode
