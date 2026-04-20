// signature-divergent fixture — `trading_port` crate.
//
// Models the canonical-port side of the split. The bounded-context
// heuristic assigns every :Item here to context `trading_port` (crate
// prefix before the first underscore). The sibling `trading_adapter`
// crate declares items with the same last-segment qnames; the
// `signature_divergent` UDF distinguishes Shared Kernel (signatures
// match) from Context Homonym (signatures diverge).

// Divergent-enum side: two variants only.
pub enum OrderStatus {
    Filled,
    Rejected,
}

// A struct with a method whose signature DIVERGES between crates
// (return type `f64` here vs `(f64, f64)` in trading_adapter). #3618
// evidence shape.
pub struct Position;

impl Position {
    // Returns a single f64 — the port-side semantic is "unit valuation".
    pub fn valuation(&self) -> f64 {
        0.0
    }
}

// A struct with a method whose signature MATCHES between crates.
// Modeled as a Shared Kernel — the same wire-level protocol for
// placing an order is intentionally shared between port and adapter.
pub struct OrderBook;

impl OrderBook {
    // Matches the adapter-side signature byte-for-byte.
    pub fn place_order(&self, _order: Order) -> Result<(), OrderError> {
        Ok(())
    }
}

pub struct Order;
pub struct OrderError;
