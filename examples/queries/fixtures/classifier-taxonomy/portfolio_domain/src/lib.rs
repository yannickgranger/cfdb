//! `portfolio_domain` — distinct bounded context from `trading`.
//!
//! Participates in the ContextHomonym pair: `Position::value` has a
//! divergent signature (returns u64 vs trading's f64) across contexts.

/// Context-homonym counterpart to `trading_domain_a::Position`. The `value`
/// method's signature diverges — trading returns `f64`, portfolio returns
/// `u64`. `signature_divergent(a, b)` flags the pair as a ContextHomonym
/// under the v0.2-8 gate.
pub struct Position {
    pub shares: u64,
}

impl Position {
    pub fn value(&self, mark: u64) -> u64 {
        self.shares * mark
    }
}
