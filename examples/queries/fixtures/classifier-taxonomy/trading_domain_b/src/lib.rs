//! `trading_domain_b` — second half of the DuplicatedFeature pair.
//!
//! Same bounded context (`trading`) as `trading_domain_a`. The `OrderBook`
//! struct here is an independent definition — not a re-export — which is
//! the Pattern A shape the classifier flags as DuplicatedFeature.

/// Paired with `trading_domain_a::OrderBook` — same name, different crate,
/// SAME context. Classifier emits one DuplicatedFeature row per definition.
pub struct OrderBook {
    pub bids: Vec<u64>,
    pub asks: Vec<u64>,
    pub timestamp: u64,
}
