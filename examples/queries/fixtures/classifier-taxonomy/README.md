# classifier-taxonomy — synthetic fixture for the §A2.1 six-class classifier (issue #48)

This fixture is a minimal Cargo workspace shaped to produce at least one
finding per `DebtClass` when run through the full cfdb pipeline:

```
cfdb extract --workspace <fixture> --db <db> --keyspace cls --hir
cfdb enrich-concepts     --db <db> --keyspace cls --workspace <fixture>
cfdb enrich-reachability --db <db> --keyspace cls
cfdb scope --db <db> --keyspace cls --context trading
```

## Crates and contexts

- `trading_domain_a`, `trading_domain_b`, `trading_cli` → bounded context `trading`
  (crate-prefix heuristic: `trading_*`)
- `portfolio_domain` → bounded context `portfolio` (crate-prefix: `portfolio_*`)

## Which shape produces which class

| Class | Produced by | Offending item |
|---|---|---|
| `DuplicatedFeature` | Identical struct name across two trading crates | `OrderBook` in `trading_domain_a` + `trading_domain_b` |
| `ContextHomonym` | Fn with same last-segment name in different contexts, divergent signatures | `value` on `trading_domain_a::Position` vs `portfolio_domain::Position` |
| `UnfinishedRefactor` | `#[deprecated]` attribute on a trading item | `OldSizer::compute` in `trading_domain_a` |
| `RandomScattering` | Two resolvers with shared prefix + divergent suffix, reachable from the CLI entry point | `compute_qty_from_bps` + `compute_qty_from_pct` in `trading_domain_a` |
| `CanonicalBypass` | `:Item` CANONICAL_FOR the `trading_concept` but unreachable from entry point | `Orphan::isolated` in `trading_domain_a` (declared canonical, no caller) |
| `Unwired` | fn item reachable_from_entry = false, not an entry point handler | `trading_domain_a::dead_function` |

## Notes

- The fixture is intentionally small (one file per crate). Scope tests
  assert presence of the class qname, not exact counts.
- `trading_cli` registers the clap entry point — without it, every fn is
  trivially unreachable and the Unwired / CanonicalBypass signals lose
  their discriminating power.
- The `.cfdb/concepts/trading_concept.toml` file declares
  `trading_domain_a` as the canonical crate for `trading_concept`, so
  items in that crate receive `:CANONICAL_FOR` edges.
