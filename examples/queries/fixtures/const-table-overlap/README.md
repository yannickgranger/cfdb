# const-table-overlap fixture

Synthetic multi-crate workspace exercising the
`examples/queries/const-table-overlap.cypher` v0.2 verdict ladder
(RFC-040 §3.4, slice 4 issue #326 + slice-4 follow-up issue #332).
Each crate sits in a distinct bounded context (inferred from
crate-prefix heuristic) and declares one or more
`pub const X: &[&str]` / `pub const X: [u32; N]` literal tables.

| Crate | Const | Element type | Entries (decl. order) | Expected verdict |
|---|---|---|---|---|
| `kraken_normalize` | `FIAT` | `&str` | `["USD","EUR","GBP"]` | DUPLICATE — pair with `oanda_pricing::FIAT` |
| `oanda_pricing` | `FIAT` | `&str` | `["GBP","USD","EUR"]` | DUPLICATE — same set, different declaration order; `entries_hash` MUST match |
| `binance_exchange` | `STABLES` | `&str` | `["USDC","USDT"]` | clean — no overlap |
| `metric_client` | `PORTS` | `u32` | `[443,80]` | clean — different element_type from any str pair, no overlap |
| `kraken_session_ports` | `PORTS` | `u32` | `[10,20,30]` | SUBSET — pair with `oanda_session_ports::PORTS`; INTERSECTION_HIGH — pair with `mt5_jaccard_ports::PORTS` |
| `oanda_session_ports` | `PORTS` | `u32` | `[20,10]` | SUBSET — strict subset of `kraken_session_ports::PORTS` |
| `mt5_jaccard_ports` | `PORTS` | `u32` | `[20,30,40]` | INTERSECTION_HIGH — jaccard 0.5 with `kraken_session_ports::PORTS`, neither subset |

## Why declaration order differs across the duplicate pair

The DUPLICATE branch joins on `entries_hash`, which is sha256 over the
canonical-sorted entry sequence (RFC §3.1). Two consts with the same set
but DIFFERENT declaration orders MUST produce identical hashes — the
`oanda_pricing::FIAT` declaration is intentionally
`["GBP","USD","EUR"]` (sorted-alpha by accident, not by reading
`kraken_normalize::FIAT`'s order) so the test pins the
order-invariance contract.

## Why `oanda_session_ports::PORTS` is declared `[20, 10]`

The SUBSET branch operates on `entries_normalized` (canonical-sorted
JSON, RFC-040 §3.4), not declaration order. Declaring the const as
`[20, 10]` (descending) pins that the `entries_subset` UDF and the
`overlap_verdict` precedence-decoder operate on the canonicalised
form: `[10, 20]` ⊂ `[10, 20, 30]`.

## Why a same-element-type non-overlap is included

`binance_exchange::STABLES` uses the same `element_type="str"` as the
DUPLICATE pair but holds a non-overlapping set (`USDC`, `USDT`). The
fixture verifies the rule does NOT cross-pair this with the `FIAT`
table — only entry-set equality / overlap (via `entries_hash`,
`entries_subset`, `entries_jaccard`) joins.

## Why a numeric clean const is included

`metric_client::PORTS = [443, 80]` carries `element_type="u32"`. Even
if its `entries_hash` collided with a `str` table (sha256 collisions
are practically zero, but the rule is semantically scoped), the
`a.element_type = b.element_type` filter MUST exclude the cross-type
join. The fixture exercises this filter.

## Why three numeric ports crates are needed

The SUBSET and INTERSECTION_HIGH branches need numeric pairs that do
NOT collide with the existing FIAT DUPLICATE pair (cross-type
filter) and do NOT collide with `metric_client::PORTS = [443, 80]`
(disjoint set, jaccard 0). The chosen `[10,20,30]` / `[20,10]` /
`[20,30,40]` triplet is engineered so:

- `kraken_session_ports::PORTS` ⟷ `oanda_session_ports::PORTS`
  fires `CONST_TABLE_SUBSET` (one is a strict subset of the other,
  hashes differ).
- `kraken_session_ports::PORTS` ⟷ `mt5_jaccard_ports::PORTS`
  fires `CONST_TABLE_INTERSECTION_HIGH` (jaccard 0.5, neither
  subset, hashes differ).
- `oanda_session_ports::PORTS` ⟷ `mt5_jaccard_ports::PORTS` falls
  below the 0.5 jaccard threshold and is filtered as
  `CONST_TABLE_NONE`.

## Tests pinned by this fixture

`crates/cfdb-cli/tests/const_table_overlap.rs`:

1. The `kraken_normalize::FIAT` ⟷ `oanda_pricing::FIAT` pair MUST
   surface under `CONST_TABLE_DUPLICATE` (one row).
2. `binance_exchange::STABLES` MUST NOT surface (no `&str` set
   overlap).
3. `metric_client::PORTS` MUST NOT surface (cross-type filter, no
   numeric overlap).
4. `kraken_session_ports::PORTS` ⟷ `oanda_session_ports::PORTS`
   MUST surface under `CONST_TABLE_SUBSET`.
5. `kraken_session_ports::PORTS` ⟷ `mt5_jaccard_ports::PORTS`
   MUST surface under `CONST_TABLE_INTERSECTION_HIGH`.
6. `oanda_session_ports::PORTS` ⟷ `mt5_jaccard_ports::PORTS` MUST
   NOT surface (below 0.5 jaccard, no subset).
7. Determinism: rule output is byte-stable across two extracts.
