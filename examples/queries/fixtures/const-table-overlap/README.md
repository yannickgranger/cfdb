# const-table-overlap fixture

Synthetic 3-crate workspace exercising the
`examples/queries/const-table-overlap.cypher` v0.1 DUPLICATE branch
(RFC-040 slice 4/5, issue #326). Each crate sits in a distinct bounded
context (inferred from crate-prefix heuristic) and declares one or more
`pub const X: &[&str]` / `pub const X: [u32; N]` literal tables.

| Crate | Const | Element type | Entries (decl. order) | Expected verdict |
|---|---|---|---|---|
| `kraken_normalize` | `FIAT` | `&str` | `["USD","EUR","GBP"]` | DUPLICATE — pair with `oanda_pricing::FIAT` |
| `oanda_pricing` | `FIAT` | `&str` | `["GBP","USD","EUR"]` | DUPLICATE — same set, different declaration order; `entries_hash` MUST match |
| `binance_exchange` | `STABLES` | `&str` | `["USDC","USDT"]` | clean — no overlap |
| `metric_client` | `PORTS` | `u32` | `[443,80]` | clean — different element_type from any str pair, no overlap |

## Why declaration order differs across the duplicate pair

The DUPLICATE branch joins on `entries_hash`, which is sha256 over the
canonical-sorted entry sequence (RFC §3.1). Two consts with the same set
but DIFFERENT declaration orders MUST produce identical hashes — the
`oanda_pricing::FIAT` declaration is intentionally
`["GBP","USD","EUR"]` (sorted-alpha by accident, not by reading
`kraken_normalize::FIAT`'s order) so the test pins the
order-invariance contract.

## Why a same-element-type non-overlap is included

`binance_exchange::STABLES` uses the same `element_type="str"` as the
DUPLICATE pair but holds a non-overlapping set (`USDC`, `USDT`). The
fixture verifies the rule does NOT cross-pair this with the `FIAT`
table — only entry-set equality (via `entries_hash`) joins.

## Why a numeric const is included

`metric_client::PORTS` carries `element_type="u32"`. Even if its
`entries_hash` collided with a `str` table (sha256 collisions are
practically zero, but the rule is semantically scoped), the
`a.element_type = b.element_type` filter MUST exclude the cross-type
join. The fixture exercises this filter.

## Tests pinned by this fixture

`crates/cfdb-cli/tests/const_table_overlap.rs`:

1. The `kraken_normalize::FIAT` ⟷ `oanda_pricing::FIAT` pair MUST surface
   under `CONST_TABLE_DUPLICATE` (one row).
2. `binance_exchange::STABLES` MUST NOT surface (no `&str` set overlap).
3. `metric_client::PORTS` MUST NOT surface (cross-type filter).

The SUBSET / INTERSECTION_HIGH branches are deferred — the
`entries_subset` / `entries_jaccard` UDFs do not yet exist in
cfdb-query. See the rule file's header comment for the contract and
the slice-4 PR follow-up issue tracking the UDF landing.
