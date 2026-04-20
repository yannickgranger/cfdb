// signature-divergent fixture — `trading_adapter` crate.
//
// Sister crate to `trading_port`. The bounded-context heuristic assigns
// every :Item here to context `trading_adapter` (crate prefix). The
// items below share last-segment qnames with items in `trading_port`;
// `signature_divergent` tells a Context Homonym (DIVERGENT signature)
// from a Shared Kernel (IDENTICAL signature).

// Divergent-enum side — extra `Pending` variant. Enum item signatures
// are not yet emitted by render_fn_signature (v0.2 scope: fn / method
// only), but the enum divergence shapes the `valuation` method below
// (the adapter splits realized vs unrealized P&L as two f64s).
pub enum OrderStatus {
    Filled,
    Rejected,
    Pending,
}

pub struct Position;

impl Position {
    // Returns a TUPLE — the adapter-side semantic is (realized,
    // unrealized). Same last-segment qname (`valuation`) as the port
    // impl, different signature. `signature_divergent` MUST return
    // `true` for this pair.
    pub fn valuation(&self) -> (f64, f64) {
        (0.0, 0.0)
    }
}

pub struct OrderBook;

impl OrderBook {
    // Identical signature to the port impl — Shared Kernel.
    // `signature_divergent` MUST return `false` for this pair.
    pub fn place_order(&self, _order: Order) -> Result<(), OrderError> {
        Ok(())
    }
}

pub struct Order;
pub struct OrderError;
