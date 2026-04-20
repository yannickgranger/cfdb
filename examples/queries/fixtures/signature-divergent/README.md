# signature-divergent fixture

Two-crate synthetic workspace exercising the `signature_divergent(a, b)`
UDF (issue #47). The two crates sit in distinct bounded contexts
(inferred from crate-prefix heuristic: `trading_port` and
`trading_adapter`) and intentionally declare items that share a
last-segment qname but diverge on `:Item.signature`.

| Item shared name | `trading_port` signature | `trading_adapter` signature | Expected verdict |
|---|---|---|---|
| `fn valuation` | `fn(&Self) -> f64` | `fn(&Self) -> (f64, f64)` | DIVERGENT — homonym candidate |
| `fn place_order` | `fn(&Self, Order) -> Result` | `fn(&Self, Order) -> Result` | IDENTICAL — shared kernel candidate |

`OrderStatus` is also modelled as divergent enums across the two crates
— the fixture exercises the #3618 evidence shape from the RFC gate
v0.2-8 spec — but the syn-based `render_fn_signature` only emits the
`signature` prop on fn / method items, so the `OrderStatus` variant
divergence is captured indirectly through the `valuation` method's
return-type drift rather than by comparing enum item signatures
directly. (Enum / struct signature rendering is v0.3 scope per the
spec; v0.2 ships the fn-level discriminator that is load-bearing for
the #48 classifier.)

The scar test at
`crates/cfdb-cli/tests/signature_divergent.rs` pins the expected
output rows: the `valuation` pair MUST surface under
`signature_divergent`, the `place_order` pair MUST NOT.

See `docs/udfs.md` for the UDF reference and
`docs/RFC-cfdb-v0.2-addendum-draft.md` §A1.5 v0.2-8 gate for the
classifier-level intent.
